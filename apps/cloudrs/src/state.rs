//! View state as plain data (ADR 0005): no GPUI types, so recorded core
//! events are enough to test it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sc_core::{ArtKey, Event, PlayState, Playback, QueueSnapshot, Repeat, TrackId, TrackSummary};

/// Artwork files by track, as the core reports them.
pub type ArtworkMap = HashMap<TrackId, Arc<Path>>;

/// The queue as the core last reported it: the one source of truth.
#[derive(Debug, Default)]
pub struct QueueState {
    pub snapshot: QueueSnapshot,
}

impl QueueState {
    /// Returns whether the queue changed, so only real changes re-render.
    pub fn apply(&mut self, event: &Event) -> bool {
        let Event::Queue(next) = event else {
            return false;
        };
        if self.snapshot == *next {
            return false;
        }
        self.snapshot.clone_from(next);
        true
    }

    /// Whether `event` brings the cover of a queued track, so an open panel
    /// must redraw (restored, autoplay and older-search tracks included).
    pub fn shows_artwork_of(&self, event: &Event) -> bool {
        let Event::Artwork {
            key: ArtKey::Track(track),
            ..
        } = event
        else {
            return false;
        };
        self.snapshot.tracks.iter().any(|t| t.id == *track)
    }

    /// The current track cannot be removed (the core refuses it too).
    pub fn can_remove(&self, index: usize) -> bool {
        self.snapshot.current != Some(index)
    }
}

/// Off, then all, then one: the order of the repeat button.
pub fn next_repeat(repeat: Repeat) -> Repeat {
    match repeat {
        Repeat::Off => Repeat::All,
        Repeat::All => Repeat::One,
        Repeat::One => Repeat::Off,
    }
}

/// What the player bar shows.
#[derive(Debug)]
pub struct PlayerState {
    pub track: Option<TrackSummary>,
    pub artwork: Option<Arc<Path>>,
    /// Converted once per track, shared with the painted waveform.
    pub waveform: Option<Arc<[f32]>>,
    pub playback: Playback,
    pub shuffle: bool,
    pub repeat: Repeat,
}

impl PlayerState {
    pub fn new() -> Self {
        Self {
            track: None,
            artwork: None,
            waveform: None,
            shuffle: false,
            repeat: Repeat::Off,
            // The core starts at full volume and only reports it on change.
            playback: Playback {
                volume: 1.0,
                ..Playback::default()
            },
        }
    }

    /// Applies a core event; `artwork` already includes the event's own file.
    pub fn apply(&mut self, event: &Event, artwork: &ArtworkMap) {
        match event {
            Event::NowPlaying(track) => {
                self.artwork = artwork.get(&track.id).cloned();
                self.waveform = None;
                self.track = Some(track.clone());
            }
            Event::Waveform { track, bars } if self.is_current(*track) => {
                self.waveform = Some(Arc::from(bars.as_slice()));
            }
            Event::Artwork {
                key: ArtKey::Track(track),
                ..
            } if self.is_current(*track) => {
                self.artwork = artwork.get(track).cloned();
            }
            Event::Playback(playback) => self.playback = *playback,
            Event::Queue(queue) => {
                self.shuffle = queue.shuffle;
                self.repeat = queue.repeat;
            }
            _ => {}
        }
    }

    fn is_current(&self, id: TrackId) -> bool {
        self.track.as_ref().is_some_and(|t| t.id == id)
    }

    pub fn playing(&self) -> bool {
        self.playback.state == PlayState::Playing
    }

    /// Played fraction of the track, 0..=1.
    pub fn progress(&self) -> f32 {
        let total = self.playback.duration.as_secs_f32();
        if total <= 0.0 {
            return 0.0;
        }
        (self.playback.position.as_secs_f32() / total).clamp(0.0, 1.0)
    }
}

/// Where a click at `fraction` (0..=1) of the waveform seeks to.
pub fn seek_target(fraction: f32, duration: Duration) -> Duration {
    duration.mul_f32(fraction.clamp(0.0, 1.0))
}

/// `03:45`, or `1:02:05` for a long mix.
pub fn format_time(time: Duration) -> String {
    let seconds = time.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// `842`, `1.2K`, `3.4M`: a count short enough for a meta line.
pub fn compact_count(count: u64) -> String {
    let (unit, suffix) = match count {
        0..=999 => return count.to_string(),
        1_000..=999_499 => (1_000.0, "K"),
        _ => (1_000_000.0, "M"),
    };
    let value = count as f64 / unit;
    // One decimal below 10, none above (`12K`), and `1K` rather than `1.0K`.
    if value < 10.0 && (value * 10.0).round() % 10.0 != 0.0 {
        format!("{value:.1}{suffix}")
    } else {
        format!("{value:.0}{suffix}")
    }
}

/// Whether pasted search text is a soundcloud.com link (`http(s)://`, with an
/// optional `www.` or `m.`) that should be played instead of searched.
pub fn is_soundcloud_url(text: &str) -> bool {
    let text = text.trim();
    let Some(rest) = strip_prefix_ci(text, "https://").or_else(|| strip_prefix_ci(text, "http://"))
    else {
        return false;
    };
    let rest = strip_prefix_ci(rest, "www.")
        .or_else(|| strip_prefix_ci(rest, "m."))
        .unwrap_or(rest);
    strip_prefix_ci(rest, "soundcloud.com/").is_some_and(|path| !path.is_empty())
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sc_core::Problem;

    use super::*;

    fn track(id: u64) -> TrackSummary {
        TrackSummary {
            id: TrackId(id),
            title: format!("Track {id}"),
            artist: "Artist".into(),
            duration: Duration::from_secs(200),
            artist_id: None,
            preview_only: false,
        }
    }

    fn playback(state: PlayState, position: u64, duration: u64) -> Event {
        Event::Playback(Playback {
            state,
            position: Duration::from_secs(position),
            duration: Duration::from_secs(duration),
            volume: 0.5,
        })
    }

    #[test]
    fn the_player_follows_the_current_track() {
        let mut artwork = ArtworkMap::new();
        artwork.insert(TrackId(1), Arc::from(Path::new("/cache/1.jpg")));
        let mut player = PlayerState::new();

        player.apply(&Event::NowPlaying(track(1)), &artwork);
        assert_eq!(player.track, Some(track(1)));
        assert!(player.artwork.is_some(), "cached cover is picked up");

        player.apply(
            &Event::Waveform {
                track: TrackId(1),
                bars: vec![0.2, 0.9],
            },
            &artwork,
        );
        assert_eq!(player.waveform.as_deref(), Some(&[0.2, 0.9][..]));

        // Late data for the previous track is ignored.
        player.apply(
            &Event::Waveform {
                track: TrackId(7),
                bars: vec![1.0],
            },
            &artwork,
        );
        assert_eq!(player.waveform.as_deref(), Some(&[0.2, 0.9][..]));

        player.apply(&Event::NowPlaying(track(2)), &artwork);
        assert!(player.waveform.is_none());
        assert!(player.artwork.is_none());
    }

    #[test]
    fn the_player_tracks_progress_and_volume() {
        let mut player = PlayerState::new();
        assert_eq!(player.playback.volume, 1.0);
        assert_eq!(player.progress(), 0.0, "no duration yet");

        player.apply(&playback(PlayState::Playing, 50, 200), &ArtworkMap::new());
        assert!(player.playing());
        assert_eq!(player.progress(), 0.25);
        assert_eq!(player.playback.volume, 0.5);
    }

    fn snapshot(ids: &[u64], current: Option<usize>) -> Event {
        Event::Queue(QueueSnapshot {
            tracks: ids.iter().map(|id| track(*id)).collect(),
            current,
            shuffle: false,
            repeat: Repeat::Off,
        })
    }

    #[test]
    fn the_queue_follows_the_core_and_only_reports_real_changes() {
        let mut queue = QueueState::default();
        assert!(!queue.apply(&Event::Problem(Problem::Offline)));
        assert!(queue.apply(&snapshot(&[1, 2, 3], Some(0))));
        assert!(
            !queue.apply(&snapshot(&[1, 2, 3], Some(0))),
            "same snapshot"
        );
        assert!(queue.apply(&snapshot(&[1, 2, 3], Some(1))));
        assert_eq!(queue.snapshot.current, Some(1));
        assert_eq!(queue.snapshot.tracks.len(), 3);
    }

    #[test]
    fn a_cover_of_a_queued_track_redraws_the_panel() {
        let mut queue = QueueState::default();
        queue.apply(&snapshot(&[1, 2], Some(0)));
        let artwork = |id: u64| Event::Artwork {
            key: ArtKey::Track(TrackId(id)),
            path: PathBuf::from("/cache/x.jpg"),
        };
        assert!(queue.shows_artwork_of(&artwork(2)));
        assert!(!queue.shows_artwork_of(&artwork(9)));
        assert!(!queue.shows_artwork_of(&Event::Problem(Problem::Offline)));
    }

    #[test]
    fn the_current_track_cannot_be_removed_from_the_queue() {
        let mut queue = QueueState::default();
        queue.apply(&snapshot(&[1, 2, 3], Some(1)));
        assert!(!queue.can_remove(1));
        assert!(queue.can_remove(0));
        assert!(queue.can_remove(2));
    }

    #[test]
    fn repeat_cycles_off_all_one() {
        assert_eq!(next_repeat(Repeat::Off), Repeat::All);
        assert_eq!(next_repeat(Repeat::All), Repeat::One);
        assert_eq!(next_repeat(Repeat::One), Repeat::Off);
    }

    #[test]
    fn the_player_keeps_the_shuffle_and_repeat_flags() {
        let mut player = PlayerState::new();
        let event = Event::Queue(QueueSnapshot {
            shuffle: true,
            repeat: Repeat::One,
            ..QueueSnapshot::default()
        });
        player.apply(&event, &ArtworkMap::new());
        assert!(player.shuffle);
        assert_eq!(player.repeat, Repeat::One);
    }

    #[test]
    fn seeking_maps_the_fraction_onto_the_duration() {
        let total = Duration::from_secs(200);
        assert_eq!(seek_target(0.25, total), Duration::from_secs(50));
        assert_eq!(seek_target(2.0, total), total);
        assert_eq!(seek_target(-1.0, total), Duration::ZERO);
    }

    #[test]
    fn time_is_formatted_for_tracks_and_long_mixes() {
        assert_eq!(format_time(Duration::from_secs(0)), "00:00");
        assert_eq!(format_time(Duration::from_secs(225)), "03:45");
        assert_eq!(format_time(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn counts_are_shortened() {
        for (count, text) in [
            (0, "0"),
            (999, "999"),
            (1_000, "1K"),
            (1_234, "1.2K"),
            (12_400, "12K"),
            (999_499, "999K"),
            (999_999, "1M"),
            (3_400_000, "3.4M"),
        ] {
            assert_eq!(compact_count(count), text, "{count}");
        }
    }

    #[test]
    fn soundcloud_links_are_recognised() {
        for url in [
            "https://soundcloud.com/artist/track",
            "http://soundcloud.com/artist",
            "https://www.soundcloud.com/artist/track",
            "https://m.soundcloud.com/artist/track?si=abc",
            "  https://soundcloud.com/artist/track  ",
            "HTTPS://SoundCloud.com/artist",
        ] {
            assert!(is_soundcloud_url(url), "{url}");
        }
    }

    #[test]
    fn other_text_is_a_search() {
        for text in [
            "",
            "soundcloud.com/artist/track",
            "daft punk",
            "https://soundcloud.com/",
            "https://soundcloud.com",
            "https://evilsoundcloud.com/artist",
            "https://soundcloud.com.evil.com/artist",
            "https://soundcloud.com@evil.com/artist",
            "https://example.com/soundcloud.com/artist",
            "ftp://soundcloud.com/artist",
            "https://é",
        ] {
            assert!(!is_soundcloud_url(text), "{text}");
        }
    }
}
