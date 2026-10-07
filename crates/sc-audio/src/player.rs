//! The player: a dedicated engine thread that owns the output, decodes ahead
//! and reports what happens.

use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use crate::decode::Decoder;
use crate::fetch::Stream;
use crate::output::{self, Output};
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
}

/// Handle to the engine thread. Dropping it stops playback.
pub struct Player {
    commands: flume::Sender<Command>,
    events: flume::Receiver<Event>,
}

const POSITION_EVERY: Duration = Duration::from_millis(100);
const IDLE_WAIT: Duration = Duration::from_millis(20);

impl Player {
    /// Opens the default audio device and starts the engine thread.
    pub fn spawn() -> Result<Self> {
        let (commands, command_rx) = flume::unbounded();
        let (event_tx, events) = flume::unbounded();
        let (ready_tx, ready_rx) = flume::bounded(1);
        thread::Builder::new()
            .name("cloudrs-audio".into())
            .spawn(move || match output::open() {
                Ok(output) => {
                    let _ = ready_tx.send(Ok(()));
                    Engine::new(output, event_tx).run(&command_rx);
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
}

impl Engine {
    fn new(output: Output, events: flume::Sender<Event>) -> Self {
        Self {
            output,
            events,
            state: PlaybackState::Idle,
            track: None,
            decoded: Vec::new(),
            pending: Vec::new(),
            pending_at: 0,
            last_position: Instant::now(),
            ended_at: Duration::ZERO,
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
            Command::Stop => {
                self.clear();
                self.set_state(PlaybackState::Idle);
            }
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
        let played = Duration::from_secs_f64(frames as f64 / f64::from(self.output.sample_rate));
        let start = self.track.as_ref().map_or(self.ended_at, |t| t.start);
        self.emit(Event::Position(start + played));
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
