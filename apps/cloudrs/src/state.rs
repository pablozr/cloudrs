//! View state as plain data (ADR 0005): no GPUI types, so recorded core
//! events are enough to test it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sc_core::{Event, PlayState, Playback, QueueSnapshot, Repeat, TrackId, TrackSummary};

/// Artwork files by track, as the core reports them.
pub type ArtworkMap = HashMap<TrackId, Arc<Path>>;

/// How close to the end of the list (in rows) the next page is requested.
const LOAD_MORE_MARGIN: usize = 5;

/// What the results area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing searched yet, or the query was cleared.
    Empty,
    Searching,
    Ready,
    /// The search failed; the person can try again.
    Failed,
}

#[derive(Debug)]
pub struct ResultsState {
    pub phase: Phase,
    /// The query the core is working on or showed last.
    pub query: String,
    pub tracks: Vec<TrackSummary>,
    pub has_more: bool,
    pub loading_more: bool,
    /// Never cleared: the core sends each artwork once, so a repeated search
    /// would otherwise come back without covers.
    pub artwork: ArtworkMap,
    /// The track the player is on, for the active row.
    pub current: Option<TrackId>,
    /// The player is playing (the equalizer moves), not paused or loading.
    pub playing: bool,
}

impl ResultsState {
    pub fn new() -> Self {
        Self {
            phase: Phase::Empty,
            query: String::new(),
            tracks: Vec::new(),
            has_more: false,
            loading_more: false,
            artwork: HashMap::new(),
            current: None,
            playing: false,
        }
    }

    /// Applies a core event. Returns whether anything the list shows changed,
    /// so playback ticks never re-render it.
    pub fn apply(&mut self, event: &Event) -> bool {
        match event {
            Event::Searching { query } => {
                self.query.clone_from(query);
                self.phase = Phase::Searching;
                self.loading_more = false;
                true
            }
            Event::Results {
                query,
                tracks,
                append,
                has_more,
            } => {
                if *append {
                    self.tracks.extend(tracks.iter().cloned());
                } else {
                    self.query.clone_from(query);
                    self.tracks.clone_from(tracks);
                    self.phase = if query.is_empty() {
                        Phase::Empty
                    } else {
                        Phase::Ready
                    };
                }
                self.has_more = *has_more;
                self.loading_more = false;
                true
            }
            Event::SearchFailed { query, append, .. } => {
                if *append {
                    // The core drops the next page when loading it fails and will
                    // not offer it again, so stop asking.
                    self.loading_more = false;
                    self.has_more = false;
                    return true;
                }
                if self.phase != Phase::Searching || *query != self.query {
                    return false;
                }
                self.phase = Phase::Failed;
                true
            }
            Event::NowPlaying(track) => {
                self.current = Some(track.id);
                self.playing = false;
                true
            }
            Event::Playback(playback) => {
                let playing = playback.state == PlayState::Playing;
                std::mem::replace(&mut self.playing, playing) != playing
            }
            Event::Artwork { track, path } => {
                self.artwork.insert(*track, Arc::from(path.as_path()));
                self.tracks.iter().any(|t| t.id == *track)
            }
            // Playback problems only drive the toast, never the results.
            Event::Problem(_) | Event::Waveform { .. } | Event::Queue(_) => false,
        }
    }

    /// True once per page: when the rows on screen end near the end of the
    /// list and the core has more. Marks the page as loading.
    pub fn take_load_more(&mut self, visible_end: usize) -> bool {
        let near_end = visible_end + LOAD_MORE_MARGIN >= self.tracks.len();
        if self.phase != Phase::Ready || !self.has_more || self.loading_more || !near_end {
            return false;
        }
        self.loading_more = true;
        true
    }
}

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
            Event::Artwork { track, .. } if self.is_current(*track) => {
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
            preview_only: false,
        }
    }

    fn results(query: &str, ids: &[u64], append: bool, has_more: bool) -> Event {
        Event::Results {
            query: query.into(),
            tracks: ids.iter().map(|id| track(*id)).collect(),
            append,
            has_more,
        }
    }

    fn search_failed(query: &str, append: bool) -> Event {
        Event::SearchFailed {
            query: query.into(),
            append,
            problem: Problem::Offline,
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

    fn ready(ids: &[u64], has_more: bool) -> ResultsState {
        let mut state = ResultsState::new();
        state.apply(&results("house", ids, false, has_more));
        state
    }

    #[test]
    fn a_search_shows_loading_then_replaces_the_results() {
        let mut state = ready(&[1, 2], false);
        assert!(state.apply(&Event::Searching {
            query: "techno".into()
        }));
        assert_eq!(state.phase, Phase::Searching);

        state.apply(&results("techno", &[3], false, true));
        assert_eq!(state.phase, Phase::Ready);
        assert_eq!(state.tracks, vec![track(3)]);
        assert!(state.has_more);
    }

    #[test]
    fn a_next_page_is_appended() {
        let mut state = ready(&[1, 2], true);
        state.loading_more = true;
        state.apply(&results("house", &[3, 4], true, false));
        let ids: Vec<u64> = state.tracks.iter().map(|t| t.id.0).collect();
        assert_eq!(ids, [1, 2, 3, 4]);
        assert!(!state.has_more);
        assert!(!state.loading_more);
    }

    #[test]
    fn an_empty_query_returns_to_the_initial_state() {
        let mut state = ready(&[1], false);
        state.apply(&results("", &[], false, false));
        assert_eq!(state.phase, Phase::Empty);
        assert!(state.tracks.is_empty());
    }

    #[test]
    fn a_failed_search_leaves_loading_and_can_be_retried() {
        let mut state = ResultsState::new();
        state.apply(&Event::Searching {
            query: "house".into(),
        });
        assert!(state.apply(&search_failed("house", false)));
        assert_eq!(state.phase, Phase::Failed);
        assert_eq!(state.query, "house");
    }

    #[test]
    fn a_failure_of_an_older_query_is_ignored() {
        let mut state = ResultsState::new();
        state.apply(&Event::Searching {
            query: "house".into(),
        });
        assert!(!state.apply(&search_failed("hou", false)));
        assert_eq!(state.phase, Phase::Searching);
    }

    #[test]
    fn an_audio_problem_during_a_search_keeps_the_skeleton() {
        let mut state = ResultsState::new();
        state.apply(&Event::Searching {
            query: "house".into(),
        });
        let audio = Event::Problem(Problem::Audio("device lost".into()));
        assert!(!state.apply(&audio));
        assert_eq!(state.phase, Phase::Searching);
    }

    #[test]
    fn a_playback_problem_keeps_the_results() {
        let mut state = ready(&[1], false);
        assert!(!state.apply(&Event::Problem(Problem::CannotPlay)));
        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn a_failed_next_page_stops_asking_for_more() {
        let mut state = ready(&[1, 2], true);
        assert!(state.take_load_more(2));
        assert!(state.apply(&search_failed("house", true)));
        assert!(!state.loading_more);
        assert!(!state.take_load_more(2));
        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn a_playback_problem_during_load_more_keeps_paging() {
        let mut state = ready(&[1, 2], true);
        assert!(state.take_load_more(2));
        assert!(!state.apply(&Event::Problem(Problem::CannotPlay)));
        assert!(state.loading_more);
        assert!(state.has_more);
    }

    #[test]
    fn load_more_fires_once_near_the_end() {
        let ids: Vec<u64> = (1..=30).collect();
        let mut state = ready(&ids, true);
        assert!(!state.take_load_more(10), "far from the end");
        assert!(state.take_load_more(26), "within the margin");
        assert!(!state.take_load_more(30), "already loading");

        state.apply(&results("house", &[31], true, true));
        assert!(state.take_load_more(31), "the next page can be requested");
    }

    #[test]
    fn load_more_needs_more_pages_and_ready_results() {
        let mut last_page = ready(&[1, 2], false);
        assert!(!last_page.take_load_more(2));

        let mut searching = ready(&[1, 2], true);
        searching.apply(&Event::Searching { query: "x".into() });
        assert!(!searching.take_load_more(2));
    }

    #[test]
    fn the_active_row_follows_the_player_without_ticks_re_rendering() {
        let mut state = ready(&[1, 2], false);
        assert!(state.apply(&Event::NowPlaying(track(2))));
        assert_eq!(state.current, Some(TrackId(2)));

        assert!(state.apply(&playback(PlayState::Playing, 0, 200)));
        assert!(state.playing);
        assert!(!state.apply(&playback(PlayState::Playing, 1, 200)), "tick");
        assert!(state.apply(&playback(PlayState::Paused, 1, 200)));
        assert!(!state.playing);
    }

    #[test]
    fn artwork_is_kept_and_only_visible_rows_re_render() {
        let mut state = ready(&[1], false);
        let artwork = |id: u64| Event::Artwork {
            track: TrackId(id),
            path: PathBuf::from(format!("/cache/{id}.jpg")),
        };
        assert!(state.apply(&artwork(1)));
        assert!(!state.apply(&artwork(9)), "not in the list");
        assert!(state.artwork.contains_key(&TrackId(9)));
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
