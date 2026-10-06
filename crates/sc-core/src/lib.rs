//! The cloudrs application core (ADR 0004).
//!
//! The UI sends [`Command`]s and renders [`Event`]s. Everything else (talking
//! to SoundCloud, driving the audio engine, caching artwork, debouncing the
//! search) happens here, on the core's own thread. The UI never sees `sc-api`
//! or `sc-audio` types.

mod artwork;
mod core;
mod types;
mod waveform;

use std::path::PathBuf;
use std::time::Duration;

pub use types::{PlayState, Playback, Problem, TrackId, TrackSummary};

/// What the UI asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// The search text changed. The core waits 300 ms for more typing; an
    /// empty query clears the results.
    Search(String),
    /// Fetch the next page of the current results.
    LoadMore,
    /// Play a track from the latest results.
    Play(TrackId),
    /// Play whatever a pasted soundcloud.com track URL points to.
    PlayUrl(String),
    /// Pause if playing, otherwise play (restarting a finished track).
    TogglePlay,
    Seek(Duration),
    /// 0.0 to 1.0.
    SetVolume(f32),
}

/// What the UI renders.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A search for this query started.
    Searching { query: String },
    /// Results for `query`. `append` adds a page to the previous results.
    Results {
        query: String,
        tracks: Vec<TrackSummary>,
        append: bool,
        has_more: bool,
    },
    /// This track is now the current one (it may still be loading).
    NowPlaying(TrackSummary),
    /// Bars of the current track's waveform, 0.0 to 1.0.
    Waveform { track: TrackId, bars: Vec<f32> },
    /// The artwork of a track is available at `path`.
    Artwork { track: TrackId, path: PathBuf },
    /// Player state, sent on every change and about ten times per second while playing.
    Playback(Playback),
    /// A search page (`append: false` is the first page, `true` a "load more")
    /// could not be fetched. A failed `append` page is dropped: the core will
    /// not offer it again, so the UI should stop asking for more.
    SearchFailed {
        query: String,
        append: bool,
        problem: Problem,
    },
    /// Something the person should know about (playing, pasted links, audio).
    Problem(Problem),
}

/// The UI's side of the core.
pub struct CoreHandle {
    commands: flume::Sender<Command>,
    events: flume::Receiver<Event>,
}

impl CoreHandle {
    /// Sends a command. Returns `false` if the core has stopped.
    pub fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }

    /// Events in order. The UI awaits them with `recv_async`.
    pub fn events(&self) -> &flume::Receiver<Event> {
        &self.events
    }
}

/// Settings the composition root decides.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Folder for cached artwork, created if missing.
    pub cache_dir: PathBuf,
}

/// Starts the core with the real SoundCloud client and the default audio device.
pub fn start(config: CoreConfig) -> Result<CoreHandle, StartError> {
    let api = sc_api::ScClient::new(sc_api::ClientConfig::default())?;
    let audio = sc_audio::Player::spawn()?.into_channels();
    Ok(spawn(api, audio, config))
}

/// Starts the core with any API and any audio link (tests use fakes).
pub fn spawn<A>(
    api: A,
    audio: (
        flume::Sender<sc_audio::Command>,
        flume::Receiver<sc_audio::Event>,
    ),
    config: CoreConfig,
) -> CoreHandle
where
    A: sc_api::SoundCloudApi + 'static,
{
    let (commands, command_rx) = flume::unbounded();
    let (event_tx, events) = flume::unbounded();
    core::run_on_thread(api, audio, config, command_rx, event_tx);
    CoreHandle { commands, events }
}

/// Why the core could not start.
#[derive(Debug)]
pub enum StartError {
    /// The HTTP client could not be built.
    Network(String),
    /// No usable audio output device.
    Audio(String),
}

impl From<sc_api::Error> for StartError {
    fn from(error: sc_api::Error) -> Self {
        Self::Network(error.to_string())
    }
}

impl From<sc_audio::Error> for StartError {
    fn from(error: sc_audio::Error) -> Self {
        Self::Audio(error.to_string())
    }
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(error) => write!(f, "network setup failed: {error}"),
            Self::Audio(error) => write!(f, "audio setup failed: {error}"),
        }
    }
}

impl std::error::Error for StartError {}
