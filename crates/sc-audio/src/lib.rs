//! The cloudrs audio engine.
//!
//! ```text
//! [fetch thread]   HLS segments (or one progressive file), read ahead ~30 s
//!       │ bytes
//! [engine thread]  symphonia demux + decode → f32, resample, channel map
//!       │ samples (lock-free ring buffer)
//! [cpal callback]  volume, pause, position → sound card
//! ```
//!
//! The crate knows nothing about SoundCloud: it plays a [`Source`] URL.
//! `sc-core` resolves that URL with `sc-api` and hands it over.

mod decode;
mod error;
mod fetch;
mod output;
mod player;
mod resample;

pub use decode::Decoder;
pub use error::{Error, Result};
pub use fetch::{HlsReader, Opened, open};
pub use player::{Command, Event, PlaybackState, Player};

/// How a stream is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// An HLS media (or master) playlist.
    Hls,
    /// A single file over HTTP.
    Progressive,
}

/// Something to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub kind: SourceKind,
}

impl Source {
    /// Guesses the kind from the URL (`.m3u8` means HLS).
    pub fn from_url(url: impl Into<String>) -> Self {
        let url = url.into();
        let path = url.split(['?', '#']).next().unwrap_or_default();
        let kind = if path.ends_with(".m3u8") || url.contains("/playlist/") {
            SourceKind::Hls
        } else {
            SourceKind::Progressive
        };
        Self { url, kind }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_the_source_kind() {
        assert_eq!(
            Source::from_url("https://cf-hls-media.sndcdn.com/playlist/x/aac_160k/y.m3u8?Policy=z")
                .kind,
            SourceKind::Hls
        );
        assert_eq!(
            Source::from_url("https://cf-media.sndcdn.com/abc.128.mp3?Policy=z").kind,
            SourceKind::Progressive
        );
    }
}
