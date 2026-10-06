//! The core's own types: what the UI sees instead of `sc-api` models.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackId(pub u64);

/// What a list row or the player bar needs to show a track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSummary {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    pub duration: Duration,
    /// SoundCloud only allows a 30-second preview (GO+).
    pub preview_only: bool,
}

impl TrackSummary {
    pub(crate) fn from_api(track: &sc_api::models::Track) -> Self {
        Self {
            id: TrackId(track.id),
            title: track.title.clone(),
            artist: track
                .user
                .as_ref()
                .map(|user| user.username.clone())
                .unwrap_or_default(),
            duration: Duration::from_millis(track.full_duration.unwrap_or(track.duration)),
            preview_only: track.is_preview_only(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlayState {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Ended,
}

/// The player bar's state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Playback {
    pub state: PlayState,
    pub position: Duration,
    pub duration: Duration,
    pub volume: f32,
}

/// Something the person should know. The UI turns each into text with i18n.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// SoundCloud could not be reached.
    Offline,
    /// SoundCloud asked us to slow down.
    RateLimited,
    /// The track, user or link does not exist or is private.
    NotFound,
    /// The pasted link is not a SoundCloud track.
    NotATrack,
    /// Only a GO+ preview exists for this track.
    PreviewOnly,
    /// The track exists but cannot be played here (region, encrypted stream, format).
    CannotPlay,
    /// The audio engine failed; the detail is for logs, not for the UI.
    Audio(String),
}

impl Problem {
    pub(crate) fn from_api(error: &sc_api::Error) -> Self {
        use sc_api::Error as E;
        match error {
            E::Network(_) | E::ClientIdNotFound | E::Unauthorized => Self::Offline,
            E::RateLimited { .. } => Self::RateLimited,
            E::NotFound => Self::NotFound,
            E::NoPlayableStream("only a preview is available") => Self::PreviewOnly,
            E::NoPlayableStream(_) | E::GeoBlocked => Self::CannotPlay,
            E::Status(_) | E::Decode(_) => Self::Offline,
        }
    }
}

/// What happens when a track ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    /// Play the same track again.
    One,
    /// Go back to the first track after the last.
    All,
}

/// The queue as the UI shows it. The single source of truth for what plays next.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueSnapshot {
    pub tracks: Vec<TrackSummary>,
    /// Index into `tracks` of the track that is playing (or loaded).
    pub current: Option<usize>,
    pub shuffle: bool,
    pub repeat: Repeat,
}
