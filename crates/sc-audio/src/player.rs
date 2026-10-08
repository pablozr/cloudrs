//! The player: a dedicated engine thread that owns the output, decodes ahead
//! and reports what happens.

use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use crate::EQ_BANDS;
use crate::decode::Decoder;
use crate::equalizer::Equalizer;
use crate::fetch::Stream;
use crate::limiter::Limiter;
use crate::loudness::{
    Meter, db_to_gain, gain_at_track_start, ramp_gain, step_toward, target_gain_db,
};
use crate::output::{self, Fault, Output};
use crate::resample::Resampler;
use crate::timeline::Timeline;
use crate::{Error, Result, Source};

/// What the player is told to do.
#[derive(Debug, Clone)]
pub enum Command {
    /// Stop what is playing and start this source.
    Load(Source),
    /// Stop what is playing and get this source ready, paused at `at`, so
    /// `Play` starts at once (a Jam starts everyone together). Answers with
    /// `State(Paused)` and a `Position` once the samples are buffered.
    Prepare {
        source: Source,
        at: Duration,
    },
    Play,
    Pause,
    /// Jump to this position in the current source.
    Seek(Duration),
    /// 0.0 to 1.0; up to 2.0 while the volume boost is on.
    SetVolume(f32),
    /// Allow the volume above 100% (up to 200%), with a limiter after the
    /// equalizer, and let normalization raise quiet tracks. Off at start.
    SetVolumeBoost(bool),
    /// Play on this device (a cpal id from [`crate::output_devices`]); `None`
    /// follows the system default. Keeps the playback state and position.
    SetDevice(Option<String>),
    Stop,
    /// Open this source in the background so it follows the current one
    /// without a gap; replaces an earlier preload. Ignored with nothing loaded.
    Preload(Source),
    /// Forget the preloaded source, if any.
    CancelPreload,
    /// The equalizer's gain in dB for each of the [`EQ_BANDS`] bands, 31 Hz to
    /// 16 kHz; `None` turns it off.
    SetEqualizer(Option<[f32; EQ_BANDS]>),
    /// Steer every track toward a common loudness (it only turns loud tracks
    /// down). Off at start.
    SetNormalize(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Idle,
    Loading,
    Playing,
    Paused,
    /// The source played to its end.
    Ended,
}

/// What the player reports.
#[derive(Debug, Clone)]
pub enum Event {
    State(PlaybackState),
    /// Position in the current source, about ten times per second while playing.
    Position(Duration),
    /// Something went wrong with the current source; the player is idle again.
    Error(String),
    /// The output device went away; the player moved to the system default and paused.
    DeviceLost,
    /// The chosen device is not available; the system default plays instead.
    DeviceMissing,
    /// The preloaded source started right after the previous one ended;
    /// positions now refer to it.
    NextStarted,
}

/// Handle to the engine thread. Dropping it stops playback.
pub struct Player {
    commands: flume::Sender<Command>,
    events: flume::Receiver<Event>,
}

const POSITION_EVERY: Duration = Duration::from_millis(100);
const IDLE_WAIT: Duration = Duration::from_millis(20);
/// How often to try the default device again while there is none.
const REOPEN_EVERY: Duration = Duration::from_secs(1);

impl Player {
    /// Opens an audio device and starts the engine thread. `device` is a cpal
    /// id from [`crate::output_devices`]; `None`, or a device that is gone,
    /// gives the system default (and, for a gone one, `Event::DeviceMissing`).
    pub fn spawn(device: Option<String>) -> Result<Self> {
        let (commands, command_rx) = flume::unbounded();
        let (event_tx, events) = flume::unbounded();
        let (ready_tx, ready_rx) = flume::bounded(1);
        thread::Builder::new()
            .name("cloudrs-audio".into())
            .spawn(move || match output::open(device.as_deref()) {
                Ok((output, missing)) => {
                    let _ = ready_tx.send(Ok(()));
                    let wanted = if missing { None } else { device };
                    let engine = Engine::new(output, wanted, event_tx);
                    if missing {
                        engine.emit(Event::DeviceMissing);
                    }
                    engine.run(&command_rx);
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            })
            .map_err(|e| Error::Output(e.to_string()))?;
        ready_rx.recv().map_err(|_| Error::EngineStopped)??;
        Ok(Self { commands, events })
    }

    pub fn send(&self, command: Command) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| Error::EngineStopped)
    }

    /// Events, in order. Poll with `try_recv` from the UI, or block with `recv`.
    pub fn events(&self) -> &flume::Receiver<Event> {
        &self.events
    }

    /// The command sender and event receiver, for a layer that talks to the
    /// player only through channels. The engine stops when the sender drops.
    pub fn into_channels(self) -> (flume::Sender<Command>, flume::Receiver<Event>) {
        (self.commands, self.events)
    }
}

struct Track {
    stream: Stream,
    /// Position of the first sample the decoder returns (the last seek target).
    start: Duration,
    decoder: Decoder,
    resampler: Resampler,
    /// Measures the source's loudness; `None` for a format it cannot measure.
    meter: Option<Meter>,
    finished_decoding: bool,
    /// What the processing chain still held at the end has been written out.
    tail_flushed: bool,
}

struct Engine {
    output: Output,
    /// The chosen device's id; `None` follows the system default.
    wanted: Option<String>,
    events: flume::Sender<Event>,
    state: PlaybackState,
    track: Option<Track>,
    decoded: Vec<f32>,
    /// Converted samples waiting for room in the ring buffer.
    pending: Vec<f32>,
    pending_at: usize,
    last_position: Instant,
    /// Start of the track that just ended, for its final position report.
    ended_at: Duration,
    /// Set while the output is dead and no device could be opened yet: when
    /// to try again.
    reopen_at: Option<Instant>,
    /// The preloaded source, ready to follow the current track.
    next: Option<Track>,
    /// The helper thread opening the preload.
    preloading: Option<flume::Receiver<Result<Track>>>,
    timeline: Timeline,
    /// The equalizer's gains; `None` (or all zero) means no equalizer.
    eq_gains: Option<[f32; EQ_BANDS]>,
    eq: Option<Equalizer>,
    normalize: bool,
    /// The normalization gain in dB; it carries across tracks.
    gain_db: f32,
    /// The linear gain the last sample went out with, so a change ramps.
    stage_gain: f32,
    /// The volume asked for, 0.0 to 2.0 with the boost.
    volume: f32,
    boost_on: bool,
    /// The part of the volume above 100%, applied here, before the limiter.
    boost: f32,
    /// In the chain only while the boost is on.
    limiter: Option<Limiter>,
}

impl Engine {
    fn new(output: Output, wanted: Option<String>, events: flume::Sender<Event>) -> Self {
        Self {
            output,
            wanted,
            events,
            state: PlaybackState::Idle,
            track: None,
            decoded: Vec::new(),
            pending: Vec::new(),
            pending_at: 0,
            last_position: Instant::now(),
            ended_at: Duration::ZERO,
            reopen_at: None,
            next: None,
            preloading: None,
            timeline: Timeline::default(),
            eq_gains: None,
            eq: None,
            normalize: false,
            gain_db: 0.0,
            stage_gain: 1.0,
            volume: 1.0,
            boost_on: false,
            boost: 1.0,
            limiter: None,
        }
    }

    fn run(mut self, commands: &flume::Receiver<Command>) {
        loop {
            let busy = self.track.as_ref().is_some_and(|t| !t.finished_decoding)
                && self.output.producer.slots() > 0;
            let wait = if busy { Duration::ZERO } else { IDLE_WAIT };
            match commands.recv_timeout(wait) {
                Ok(command) => self.handle(command),
                Err(flume::RecvTimeoutError::Timeout) => {}
                Err(flume::RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(command) = commands.try_recv() {
                self.handle(command);
            }
            self.check_output();
            self.poll_preload();
            if let Err(error) = self.feed() {
                self.fail(error);
            }
            self.check_handover();
            self.report_progress();
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn set_state(&mut self, state: PlaybackState) {
        // Nothing can play while there is no device.
        let state = if self.reopen_at.is_some() && state == PlaybackState::Playing {
            PlaybackState::Paused
        } else {
            state
        };
        if self.state != state {
            self.state = state;
            self.output
                .shared
                .paused
                .store(state != PlaybackState::Playing, Ordering::Relaxed);
            self.emit(Event::State(state));
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Load(source) => {
                self.drop_preload();
                self.clear();
                self.gain_db = gain_at_track_start(self.gain_db);
                self.set_state(PlaybackState::Loading);
                match open_track(&source, &self.output) {
                    Ok(track) => {
                        self.track = Some(track);
                        self.set_state(PlaybackState::Playing);
                    }
                    Err(error) => self.fail(error),
                }
            }
            Command::Prepare { source, at } => {
                self.drop_preload();
                self.clear();
                self.gain_db = gain_at_track_start(self.gain_db);
                self.set_state(PlaybackState::Loading);
                let (rate, channels) = (self.output.sample_rate, self.output.channels);
                match Stream::open(&source).and_then(|stream| track_at(stream, at, rate, channels))
                {
                    Ok(track) => {
                        self.track = Some(track);
                        self.set_state(PlaybackState::Paused);
                        if let Err(error) = self.feed() {
                            return self.fail(error);
                        }
                        self.report_position();
                    }
                    Err(error) => self.fail(error),
                }
            }
            Command::Play if self.track.is_some() => self.set_state(PlaybackState::Playing),
            Command::Play => {}
            Command::Pause if self.state == PlaybackState::Playing => {
                self.set_state(PlaybackState::Paused);
            }
            Command::Pause => {}
            Command::Seek(at) => {
                if let Some(track) = self.track.take() {
                    self.complete_handover();
                    self.clear();
                    match seek_track(track, at, &self.output) {
                        Ok(track) => {
                            self.track = Some(track);
                            self.report_position();
                        }
                        Err(error) => self.fail(error),
                    }
                }
            }
            Command::SetVolume(volume) => {
                self.volume = volume;
                self.apply_volume();
            }
            Command::SetVolumeBoost(on) => self.set_boost(on),
            Command::SetDevice(device) => {
                if device != self.wanted {
                    self.wanted = device;
                    if let Err(error) = self.reopen() {
                        tracing::warn!(%error, "could not open the chosen output device");
                        self.wanted = None;
                        self.emit(Event::DeviceMissing);
                    }
                }
            }
            Command::Stop => {
                self.drop_preload();
                self.clear();
                self.set_state(PlaybackState::Idle);
            }
            Command::Preload(source) => self.start_preload(source),
            Command::CancelPreload => self.drop_preload(),
            Command::SetEqualizer(gains) => {
                self.eq_gains = gains;
                self.rebuild_eq();
            }
            Command::SetNormalize(on) => self.normalize = on,
        }
    }

    /// Splits the volume between the callback (at most 1) and the engine.
    fn apply_volume(&mut self) {
        let (callback, engine) = split_volume(self.volume, self.boost_on);
        self.output.shared.set_volume(callback);
        self.boost = engine;
    }

    fn set_boost(&mut self, on: bool) {
        if on == self.boost_on {
            return;
        }
        self.boost_on = on;
        if on {
            self.rebuild_limiter();
        } else {
            // The frames the limiter holds still belong to the song.
            if let (Some(mut limiter), Some(_)) = (self.limiter.take(), &self.track) {
                limiter.drain(&mut self.pending);
            }
            // A raise only exists with the boost: drop it now, not at 2 dB/s.
            self.gain_db = self.gain_db.min(0.0);
        }
        self.apply_volume();
    }

    fn rebuild_limiter(&mut self) {
        self.limiter = self
            .boost_on
            .then(|| Limiter::new(self.output.sample_rate, self.output.channels));
    }

    /// Builds the equalizer for the output's format from `eq_gains`.
    fn rebuild_eq(&mut self) {
        self.eq = self
            .eq_gains
            .filter(|gains| gains.iter().any(|&g| g != 0.0))
            .map(|gains| Equalizer::new(self.output.sample_rate, self.output.channels, gains));
    }

    fn drop_preload(&mut self) {
        self.next = None;
        self.preloading = None;
    }

    /// Opens `source` on a short-lived thread: `Stream::open` blocks on the
    /// network and the engine thread must keep feeding the ring buffer.
    fn start_preload(&mut self, source: Source) {
        if self.track.is_none() {
            tracing::debug!("preload ignored: nothing is loaded");
            return;
        }
        self.drop_preload();
        let (rate, channels) = (self.output.sample_rate, self.output.channels);
        let (tx, rx) = flume::bounded(1);
        let spawned = thread::Builder::new()
            .name("cloudrs-preload".into())
            .spawn(move || {
                let track = Stream::open(&source)
                    .and_then(|stream| track_at(stream, Duration::ZERO, rate, channels));
                let _ = tx.send(track);
            });
        match spawned {
            Ok(_) => self.preloading = Some(rx),
            Err(error) => tracing::warn!(%error, "could not start the preload thread"),
        }
    }

    /// Takes the preloaded track once its thread is done.
    fn poll_preload(&mut self) {
        let Some(rx) = &self.preloading else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(flume::TryRecvError::Empty) => return,
            Err(flume::TryRecvError::Disconnected) => {
                self.preloading = None;
                return;
            }
        };
        self.preloading = None;
        match result {
            Ok(mut track) => {
                track.resampler = self.resampler_for(&track.decoder);
                self.next = Some(track);
            }
            Err(error) => tracing::warn!(%error, "preload failed"),
        }
    }

    fn resampler_for(&self, decoder: &Decoder) -> Resampler {
        Resampler::new(
            decoder.sample_rate(),
            decoder.channels(),
            self.output.sample_rate,
            self.output.channels,
        )
    }

    /// Emits `NextStarted` if a handover was still pending.
    fn complete_handover(&mut self) {
        if self.timeline.finish() {
            self.emit(Event::NextStarted);
        }
    }

    /// Once the device has played up to the handover point, the next track is
    /// the one playing.
    fn check_handover(&mut self) {
        let played = self.output.shared.frames_played.load(Ordering::Relaxed);
        if self.timeline.crossed(played) {
            self.emit(Event::NextStarted);
            self.report_position();
        }
    }

    /// Reacts to what the stream's error callback reported, and retries the
    /// default device while there is none.
    fn check_output(&mut self) {
        let fault = self.output.shared.take_fault();
        let retry = self.reopen_at.is_some_and(|at| Instant::now() >= at);
        if fault != Fault::None || retry {
            self.recover(fault);
        }
    }

    /// Moves to the system default device. A change of default with the old
    /// device still around keeps playing; a loss pauses and says so.
    fn recover(&mut self, fault: Fault) {
        let retrying = self.reopen_at.is_some();
        let old_present = fault == Fault::Invalidated
            && !retrying
            && self
                .output
                .device_id
                .as_deref()
                .is_some_and(output::is_present);
        let plan = plan_recovery(fault, old_present, self.state == PlaybackState::Playing);
        if !plan.keep_playing && self.state == PlaybackState::Playing {
            self.set_state(PlaybackState::Paused);
        }
        if plan.lost {
            self.wanted = None;
        }
        if let Err(error) = self.reopen() {
            tracing::warn!(%error, "no audio output to move to");
            self.reopen_at = Some(Instant::now() + REOPEN_EVERY);
        }
        if plan.lost && !retrying {
            self.emit(Event::DeviceLost);
        }
    }

    /// Opens the wanted device (the default when it is gone) and moves to it.
    fn reopen(&mut self) -> Result<()> {
        let (output, missing) = output::open(self.wanted.as_deref())?;
        self.reopen_at = None;
        self.switch_output(output);
        if missing {
            self.wanted = None;
            self.emit(Event::DeviceMissing);
        }
        Ok(())
    }

    /// Replaces the output, rebuilding the current track for its sample rate
    /// at the position reached. The old output is not flushed: its callback
    /// may never run again.
    fn switch_output(&mut self, new: Output) {
        self.complete_handover();
        let frames = self.output.shared.frames_played.load(Ordering::Relaxed);
        let resume = self.track.as_ref().map(|t| {
            self.timeline
                .position(t.start, frames, self.output.sample_rate)
        });
        new.shared.set_volume(self.output.shared.volume());
        self.pending.clear();
        self.pending_at = 0;
        self.output = new;
        self.rebuild_eq();
        self.rebuild_limiter();
        self.timeline.reset();
        if let Some(mut next) = self.next.take() {
            next.resampler = self.resampler_for(&next.decoder);
            self.next = Some(next);
        }
        if let (Some(track), Some(at)) = (self.track.take(), resume) {
            match seek_track(track, at, &self.output) {
                Ok(track) => self.track = Some(track),
                Err(error) => self.fail(error),
            }
        }
        // `set_state` does nothing when the state did not change.
        self.output
            .shared
            .paused
            .store(self.state != PlaybackState::Playing, Ordering::Relaxed);
        if resume.is_some() {
            self.report_position();
        }
    }

    /// Drops the current track and everything buffered for it.
    fn clear(&mut self) {
        self.track = None;
        self.pending.clear();
        self.pending_at = 0;
        if let Some(eq) = &mut self.eq {
            eq.reset();
        }
        if let Some(limiter) = &mut self.limiter {
            limiter.reset();
        }
        let shared = &self.output.shared;
        shared.flush.store(true, Ordering::Release);
        // The callback clears the flag on its next run (a few ms).
        let deadline = Instant::now() + Duration::from_millis(250);
        while shared.flush.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        self.timeline.reset();
    }

    fn fail(&mut self, error: Error) {
        tracing::warn!(%error, "playback failed");
        self.drop_preload();
        self.clear();
        self.emit(Event::Error(error.to_string()));
        self.set_state(PlaybackState::Idle);
    }

    /// Decodes and pushes samples until the ring buffer is full. When the
    /// current track is fully written and the next one is ready, the next one
    /// follows in the same buffer, with no gap.
    fn feed(&mut self) -> Result<()> {
        loop {
            let Some(track) = self.track.as_mut() else {
                return Ok(());
            };
            if self.pending_at >= self.pending.len() {
                if track.finished_decoding {
                    if self.hand_over() || self.flush_tail() {
                        continue;
                    }
                    break;
                }
                self.pending.clear();
                self.pending_at = 0;
                if !track.decoder.next_chunk(&mut self.decoded)? {
                    track.finished_decoding = true;
                    continue;
                }
                let (rate, channels) = (track.decoder.sample_rate(), track.decoder.channels());
                if track
                    .meter
                    .as_ref()
                    .is_none_or(|m| !m.matches(rate, channels))
                {
                    track.meter = Meter::new(rate, channels);
                }
                if self.normalize
                    && let Some(meter) = &mut track.meter
                {
                    meter.add(&self.decoded);
                }
                if !track.resampler.matches(rate, channels) {
                    track.resampler = Resampler::new(
                        rate,
                        channels,
                        self.output.sample_rate,
                        self.output.channels,
                    );
                }
                track.resampler.process(&self.decoded, &mut self.pending);
                let lufs = track.meter.as_ref().and_then(Meter::lufs);
                self.run_stages(lufs);
            }
            let room = self.output.producer.slots();
            if room == 0 {
                break;
            }
            let n = room.min(self.pending.len() - self.pending_at);
            let samples = &self.pending[self.pending_at..self.pending_at + n];
            if let Ok(chunk) = self.output.producer.write_chunk_uninit(n) {
                chunk.fill_from_iter(samples.iter().copied());
            }
            self.pending_at += n;
            self.timeline.wrote(n);
        }
        let drained = self.output.producer.slots() == self.output.capacity;
        if let Some(track) = &self.track
            && track.finished_decoding
            && self.pending_at >= self.pending.len()
            && self.next.is_none()
            && drained
        {
            self.ended_at = track.start;
            self.track = None;
            // A preload still opening is too late: the core loads the next one.
            self.drop_preload();
            self.report_position();
            self.set_state(PlaybackState::Ended);
        }
        Ok(())
    }

    /// Runs the processing chain over `pending`: the normalization gain and
    /// the boost, then the equalizer, then the limiter. `lufs` is the loudness measured so far for the track.
    fn run_stages(&mut self, lufs: Option<f64>) {
        let channels = self.output.channels;
        let frames = self.pending.len() / channels.max(1);
        if frames == 0 {
            return;
        }
        let target = if self.normalize {
            lufs.map_or(self.gain_db, |l| target_gain_db(l, self.boost_on))
        } else {
            0.0
        };
        let seconds = frames as f32 / self.output.sample_rate as f32;
        self.gain_db = step_toward(self.gain_db, target, seconds);
        let gain = db_to_gain(self.gain_db) * self.boost;
        ramp_gain(&mut self.pending, channels, self.stage_gain, gain);
        self.stage_gain = gain;
        if let Some(eq) = &mut self.eq {
            eq.process(&mut self.pending);
        }
        if let Some(limiter) = &mut self.limiter {
            limiter.process(&mut self.pending);
        }
    }

    /// At the end of the last track, with nothing after it: puts what the
    /// limiter still holds into `pending` so it is played before `Ended`.
    /// True when there is something new to write.
    fn flush_tail(&mut self) -> bool {
        let Some(track) = self.track.as_mut() else {
            return false;
        };
        if track.tail_flushed {
            return false;
        }
        track.tail_flushed = true;
        self.pending.clear();
        self.pending_at = 0;
        if let Some(limiter) = &mut self.limiter {
            limiter.drain(&mut self.pending);
        }
        true
    }

    /// Makes the preloaded track the current one, carrying the resampler over
    /// when the format is the same so the join is continuous.
    fn hand_over(&mut self) -> bool {
        let Some(mut next) = self.next.take() else {
            return false;
        };
        let Some(old) = self.track.take() else {
            return false;
        };
        self.timeline
            .begin_handover(old.start, self.output.channels);
        self.gain_db = gain_at_track_start(self.gain_db);
        if old
            .resampler
            .matches(next.decoder.sample_rate(), next.decoder.channels())
        {
            next.resampler = old.resampler;
        }
        self.track = Some(next);
        true
    }

    fn report_progress(&mut self) {
        if self.state == PlaybackState::Playing && self.last_position.elapsed() >= POSITION_EVERY {
            self.report_position();
        }
    }

    fn report_position(&mut self) {
        self.last_position = Instant::now();
        let frames = self.output.shared.frames_played.load(Ordering::Relaxed);
        let start = self.track.as_ref().map_or(self.ended_at, |t| t.start);
        self.emit(Event::Position(self.timeline.position(
            start,
            frames,
            self.output.sample_rate,
        )));
    }
}

struct Recovery {
    /// A voluntary change of default: nothing needs to stop.
    keep_playing: bool,
    /// The device is gone: tell the user.
    lost: bool,
}

fn plan_recovery(fault: Fault, old_present: bool, playing: bool) -> Recovery {
    let lost = fault == Fault::Lost || !old_present;
    Recovery {
        keep_playing: playing && !lost,
        lost,
    }
}

/// Splits a volume into the part the callback applies (at most 1) and the part
/// the engine applies before the limiter. Without the boost the volume stops
/// at 100%.
fn split_volume(volume: f32, boost: bool) -> (f32, f32) {
    let volume = if volume.is_nan() { 1.0 } else { volume };
    let volume = volume.clamp(0.0, if boost { 2.0 } else { 1.0 });
    (volume.min(1.0), volume.max(1.0))
}

fn open_track(source: &Source, output: &Output) -> Result<Track> {
    let stream = Stream::open(source)?;
    track_at(stream, Duration::ZERO, output.sample_rate, output.channels)
}

/// Rebuilds the track at `at`, keeping what the meter has heard.
fn seek_track(track: Track, at: Duration, output: &Output) -> Result<Track> {
    let mut sought = track_at(track.stream, at, output.sample_rate, output.channels)?;
    if let Some(meter) = track.meter
        && meter.matches(sought.decoder.sample_rate(), sought.decoder.channels())
    {
        sought.meter = Some(meter);
    }
    Ok(sought)
}

fn track_at(stream: Stream, at: Duration, out_rate: u32, out_channels: usize) -> Result<Track> {
    let (opened, skip) = stream.read_from(at)?;
    let mut decoder = Decoder::new(opened.reader, opened.extension.as_deref())?;
    decoder.skip(skip);
    let resampler = Resampler::new(
        decoder.sample_rate(),
        decoder.channels(),
        out_rate,
        out_channels,
    );
    Ok(Track {
        stream,
        start: at,
        meter: Meter::new(decoder.sample_rate(), decoder.channels()),
        decoder,
        resampler,
        finished_decoding: false,
        tail_flushed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_volume_is_split_between_the_callback_and_the_engine() {
        assert_eq!(split_volume(0.5, false), (0.5, 1.0));
        assert_eq!(split_volume(1.5, true), (1.0, 1.5));
        assert_eq!(split_volume(1.5, false), (1.0, 1.0));
        assert_eq!(split_volume(2.5, true), (1.0, 2.0));
        assert_eq!(split_volume(-1.0, true), (0.0, 1.0));
        assert_eq!(split_volume(f32::NAN, true), (1.0, 1.0));
    }

    #[test]
    fn a_lost_device_pauses_and_tells() {
        let plan = plan_recovery(Fault::Lost, false, true);
        assert!(plan.lost && !plan.keep_playing);
    }

    #[test]
    fn a_new_default_with_the_old_device_present_keeps_playing_quietly() {
        let plan = plan_recovery(Fault::Invalidated, true, true);
        assert!(!plan.lost && plan.keep_playing);
    }

    #[test]
    fn an_invalidated_stream_whose_device_vanished_is_a_loss() {
        let plan = plan_recovery(Fault::Invalidated, false, true);
        assert!(plan.lost && !plan.keep_playing);
    }

    #[test]
    fn a_paused_player_stays_paused() {
        assert!(!plan_recovery(Fault::Invalidated, true, false).keep_playing);
    }

    /// The preload thread hands a whole track to the engine thread.
    #[test]
    fn a_track_can_cross_threads() {
        fn assert_send<T: Send>() {}
        assert_send::<Track>();
    }
}
