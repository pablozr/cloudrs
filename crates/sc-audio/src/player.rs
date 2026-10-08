//! The player: a dedicated engine thread that owns the output, decodes ahead
//! and reports what happens.

use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use crate::decode::Decoder;
use crate::fetch::Stream;
use crate::output::{self, Fault, Output};
use crate::resample::Resampler;
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
    /// 0.0 to 1.0.
    SetVolume(f32),
    /// Play on this device (a cpal id from [`crate::output_devices`]); `None`
    /// follows the system default. Keeps the playback state and position.
    SetDevice(Option<String>),
    Stop,
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
    finished_decoding: bool,
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
            if let Err(error) = self.feed() {
                self.fail(error);
            }
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
                self.clear();
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
                self.clear();
                self.set_state(PlaybackState::Loading);
                match Stream::open(&source).and_then(|stream| track_at(stream, at, &self.output)) {
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
            Command::SetVolume(volume) => self.output.shared.set_volume(volume),
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
                self.clear();
                self.set_state(PlaybackState::Idle);
            }
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
        let old = &self.output;
        let frames = old.shared.frames_played.load(Ordering::Relaxed);
        let resume = self
            .track
            .as_ref()
            .map(|t| position_at(t.start, frames, old.sample_rate));
        new.shared.set_volume(old.shared.volume());
        self.pending.clear();
        self.pending_at = 0;
        self.output = new;
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
        let shared = &self.output.shared;
        shared.flush.store(true, Ordering::Release);
        // The callback clears the flag on its next run (a few ms).
        let deadline = Instant::now() + Duration::from_millis(250);
        while shared.flush.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn fail(&mut self, error: Error) {
        tracing::warn!(%error, "playback failed");
        self.clear();
        self.emit(Event::Error(error.to_string()));
        self.set_state(PlaybackState::Idle);
    }

    /// Decodes and pushes samples until the ring buffer is full.
    fn feed(&mut self) -> Result<()> {
        let Some(track) = self.track.as_mut() else {
            return Ok(());
        };
        loop {
            if self.pending_at >= self.pending.len() {
                if track.finished_decoding {
                    break;
                }
                self.pending.clear();
                self.pending_at = 0;
                if !track.decoder.next_chunk(&mut self.decoded)? {
                    track.finished_decoding = true;
                    break;
                }
                let (rate, channels) = (track.decoder.sample_rate(), track.decoder.channels());
                if !track.resampler.matches(rate, channels) {
                    track.resampler = Resampler::new(
                        rate,
                        channels,
                        self.output.sample_rate,
                        self.output.channels,
                    );
                }
                track.resampler.process(&self.decoded, &mut self.pending);
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
        }
        let drained = self.output.producer.slots() == self.output.capacity;
        if track.finished_decoding && self.pending_at >= self.pending.len() && drained {
            self.ended_at = track.start;
            self.track = None;
            self.report_position();
            self.set_state(PlaybackState::Ended);
        }
        Ok(())
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
        self.emit(Event::Position(position_at(
            start,
            frames,
            self.output.sample_rate,
        )));
    }
}

/// Position in a track that started at `start` once `frames` have reached the device.
fn position_at(start: Duration, frames: u64, rate: u32) -> Duration {
    start + Duration::from_secs_f64(frames as f64 / f64::from(rate))
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

fn open_track(source: &Source, output: &Output) -> Result<Track> {
    track_at(Stream::open(source)?, Duration::ZERO, output)
}

fn seek_track(track: Track, at: Duration, output: &Output) -> Result<Track> {
    track_at(track.stream, at, output)
}

fn track_at(stream: Stream, at: Duration, output: &Output) -> Result<Track> {
    let (opened, skip) = stream.read_from(at)?;
    let mut decoder = Decoder::new(opened.reader, opened.extension.as_deref())?;
    decoder.skip(skip);
    let resampler = Resampler::new(
        decoder.sample_rate(),
        decoder.channels(),
        output.sample_rate,
        output.channels,
    );
    Ok(Track {
        stream,
        start: at,
        decoder,
        resampler,
        finished_decoding: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn position_is_the_start_plus_the_frames_played() {
        let at = position_at(Duration::from_secs(10), 96_000, 48_000);
        assert_eq!(at, Duration::from_secs(12));
    }
}
