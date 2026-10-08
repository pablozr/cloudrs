//! What plays, on the OS media controls (ADR 0019). Follows the core's events
//! like presence.rs; playback ticks never reach the OS. Media keys come back
//! as core commands.

use std::time::{Duration, Instant};

use gpui::{Context, Task, Window};
use sc_core::{Command, Event, PlayState, Playback, TrackId, TrackSummary};
use sc_platform::media::{MediaControls, MediaKey, Metadata, State};

use super::Shell;
use crate::state::nudge_target;

/// A position this far from where the last one sent would have it is a seek.
const SEEK_THRESHOLD_MS: i64 = 2000;

/// What the OS was last told about playback.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Sent {
    playing: bool,
    stopped: bool,
    /// While playing, when the track would have started (on our clock);
    /// while paused, the position.
    anchor_ms: i64,
}

#[derive(Default)]
pub(crate) struct SystemMedia {
    controls: Option<MediaControls>,
    /// Reads the media keys; dropping it stops them.
    _keys: Option<Task<()>>,
    now: Option<TrackSummary>,
    cover: Option<(TrackId, Option<String>)>,
    playback: Playback,
    sent: Option<Sent>,
    sent_volume: Option<f32>,
    clock: Option<Instant>,
}

impl SystemMedia {
    /// Registers with the OS. Call on the main thread, once the window exists.
    pub(crate) fn start(window: &Window, cx: &mut Context<Shell>) -> Self {
        let mut media = Self {
            clock: Some(Instant::now()),
            ..Self::default()
        };
        if let Some((controls, keys)) = sc_platform::media::start(hwnd(window)) {
            media.controls = Some(controls);
            media._keys = Some(cx.spawn(async move |this, cx| {
                while let Ok(key) = keys.recv_async().await {
                    if this.update(cx, |shell, _| shell.on_media_key(key)).is_err() {
                        break;
                    }
                }
            }));
        }
        media
    }

    fn apply(&mut self, event: &Event) {
        if self.controls.is_none() {
            return;
        }
        match event {
            Event::NowPlaying(track) => {
                self.now = Some(track.clone());
                self.sent = None;
                self.show();
            }
            Event::NowPlayingLinks {
                track, cover_url, ..
            } => {
                self.cover = Some((*track, cover_url.clone()));
                if self.now.as_ref().is_some_and(|now| now.id == *track) {
                    self.show();
                }
            }
            Event::Playback(playback) => {
                self.playback = *playback;
                self.sync();
            }
            _ => {}
        }
    }

    fn show(&mut self) {
        let (Some(controls), Some(track)) = (&mut self.controls, &self.now) else {
            return;
        };
        controls.show(&metadata(track, self.cover.as_ref()));
    }

    /// Tells the OS only when something it shows changed.
    fn sync(&mut self) {
        let Some(controls) = &mut self.controls else {
            return;
        };
        let playback = &self.playback;
        if let Some(state) = os_state(playback, self.now.is_some()) {
            let clock_ms = self
                .clock
                .map_or(0, |start| start.elapsed().as_millis() as i64);
            let next = sent_for(state, clock_ms);
            if needs_send(self.sent.as_ref(), &next) {
                controls.set_state(state);
                self.sent = Some(next);
            }
        }
        if self.sent_volume != Some(playback.volume) {
            controls.set_volume(playback.volume);
            self.sent_volume = Some(playback.volume);
        }
    }
}

impl Shell {
    /// Follows what the OS shows: the track, its cover and play state.
    pub(crate) fn media_event(&mut self, event: &Event) {
        self.media.apply(event);
    }

    /// A media key or an OS button; the core refuses what a Jam guest cannot do.
    fn on_media_key(&mut self, key: MediaKey) {
        if let Some(command) = command_for(key, self.media.now.is_some(), &self.media.playback) {
            self.send(command);
        }
    }
}

/// The main window's handle, which SMTC needs.
#[cfg(windows)]
fn hwnd(window: &Window) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut _),
        _ => None,
    }
}

#[cfg(not(windows))]
fn hwnd(_window: &Window) -> Option<*mut std::ffi::c_void> {
    None
}

/// The cover only counts for the track it was announced for.
fn metadata(track: &TrackSummary, cover: Option<&(TrackId, Option<String>)>) -> Metadata {
    let cover_url = match cover {
        Some((id, url)) if *id == track.id => url.clone(),
        _ => None,
    };
    Metadata {
        title: track.title.clone(),
        artist: track.artist.clone(),
        cover_url,
        duration: track.duration,
    }
}

/// What the OS should show for a playback state; `None` leaves it as it is
/// (a track that is loading would flash "paused" between two tracks).
fn os_state(playback: &Playback, has_track: bool) -> Option<State> {
    let at = playback.position;
    match playback.state {
        PlayState::Playing => Some(State::Playing { at }),
        PlayState::Paused | PlayState::Ended => Some(State::Paused { at }),
        PlayState::Idle if has_track => Some(State::Paused { at }),
        PlayState::Idle => Some(State::Stopped),
        PlayState::Loading => None,
    }
}

fn sent_for(state: State, clock_ms: i64) -> Sent {
    let ms = |at: Duration| at.as_millis() as i64;
    match state {
        State::Playing { at } => Sent {
            playing: true,
            stopped: false,
            anchor_ms: clock_ms - ms(at),
        },
        State::Paused { at } => Sent {
            playing: false,
            stopped: false,
            anchor_ms: ms(at),
        },
        State::Stopped => Sent {
            playing: false,
            stopped: true,
            anchor_ms: 0,
        },
    }
}

fn needs_send(sent: Option<&Sent>, next: &Sent) -> bool {
    match sent {
        None => true,
        Some(sent) => {
            sent.playing != next.playing
                || sent.stopped != next.stopped
                || (sent.anchor_ms - next.anchor_ms).abs() >= SEEK_THRESHOLD_MS
        }
    }
}

/// The core only has `TogglePlay` and does nothing while loading, so play,
/// pause and stop act only when they would change something. Without a
/// track every key does nothing, like the keyboard shortcuts.
fn command_for(key: MediaKey, has_track: bool, playback: &Playback) -> Option<Command> {
    if !has_track {
        return None;
    }
    let (position, duration) = (playback.position, playback.duration);
    let playing = playback.state == PlayState::Playing;
    let seek_by = |forward: bool, by: Duration| {
        (!duration.is_zero()).then(|| {
            Command::Seek(if forward {
                (position + by).min(duration)
            } else {
                position.saturating_sub(by)
            })
        })
    };
    match key {
        MediaKey::Toggle => Some(Command::TogglePlay),
        MediaKey::Play => {
            (!playing && playback.state != PlayState::Loading).then_some(Command::TogglePlay)
        }
        MediaKey::Pause | MediaKey::Stop => playing.then_some(Command::TogglePlay),
        MediaKey::Next => Some(Command::Next),
        MediaKey::Previous => Some(Command::Previous),
        MediaKey::Seek { forward } => nudge_target(position, duration, forward).map(Command::Seek),
        MediaKey::SeekBy { forward, by } => seek_by(forward, by),
        MediaKey::SetPosition(at) => (!duration.is_zero()).then(|| Command::Seek(at.min(duration))),
        MediaKey::SetVolume(volume) => Some(Command::SetVolume(volume.clamp(0.0, 1.0) as f32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn track(id: u64) -> TrackSummary {
        TrackSummary {
            id: TrackId(id),
            title: "Title".into(),
            artist: "Artist".into(),
            artist_id: None,
            duration: secs(200),
            preview_only: false,
        }
    }

    fn playback(state: PlayState, position: u64, duration: u64) -> Playback {
        Playback {
            state,
            position: secs(position),
            duration: secs(duration),
            volume: 1.0,
        }
    }

    #[test]
    fn the_cover_must_be_the_tracks() {
        let cover = (TrackId(1), Some("https://img/1.jpg".to_owned()));
        let same = metadata(&track(1), Some(&cover));
        assert_eq!(same.cover_url.as_deref(), Some("https://img/1.jpg"));
        assert_eq!(same.duration, secs(200));
        assert_eq!(metadata(&track(2), Some(&cover)).cover_url, None);
        assert_eq!(metadata(&track(1), None).cover_url, None);
    }

    #[test]
    fn the_os_state_follows_playback() {
        let at = secs(30);
        let of = |state, has_track| os_state(&playback(state, 30, 200), has_track);
        assert_eq!(of(PlayState::Playing, true), Some(State::Playing { at }));
        assert_eq!(of(PlayState::Paused, true), Some(State::Paused { at }));
        assert_eq!(of(PlayState::Ended, true), Some(State::Paused { at }));
        assert_eq!(of(PlayState::Idle, true), Some(State::Paused { at }));
        assert_eq!(of(PlayState::Idle, false), Some(State::Stopped));
        assert_eq!(of(PlayState::Loading, true), None);
        assert_eq!(of(PlayState::Loading, false), None);
    }

    #[test]
    fn only_flips_and_seeks_reach_the_os() {
        let playing = |at: u64, clock_ms: i64| {
            sent_for(
                State::Playing {
                    at: Duration::from_millis(at),
                },
                clock_ms,
            )
        };
        let first = playing(10_000, 1_000);
        assert!(needs_send(None, &first));
        // A tick: the clock and the position both advance by 100 ms.
        assert!(!needs_send(Some(&first), &playing(10_100, 1_100)));
        // A seek forward by 10 s.
        assert!(needs_send(Some(&first), &playing(20_100, 1_100)));
        // Pause.
        let paused = sent_for(State::Paused { at: secs(11) }, 1_100);
        assert!(needs_send(Some(&first), &paused));
        // Paused and still.
        assert!(!needs_send(Some(&paused), &paused));
        // Stopped.
        assert!(needs_send(Some(&paused), &sent_for(State::Stopped, 0)));
    }

    #[test]
    fn keys_respect_the_core() {
        let play = |state| command_for(MediaKey::Play, true, &playback(state, 10, 200));
        assert_eq!(play(PlayState::Playing), None);
        assert_eq!(play(PlayState::Loading), None);
        assert_eq!(play(PlayState::Paused), Some(Command::TogglePlay));
        assert_eq!(play(PlayState::Ended), Some(Command::TogglePlay));

        let pause = |key, state| command_for(key, true, &playback(state, 10, 200));
        assert_eq!(pause(MediaKey::Pause, PlayState::Paused), None);
        assert_eq!(
            pause(MediaKey::Pause, PlayState::Playing),
            Some(Command::TogglePlay)
        );
        assert_eq!(
            pause(MediaKey::Stop, PlayState::Playing),
            Some(Command::TogglePlay)
        );
        assert_eq!(pause(MediaKey::Stop, PlayState::Paused), None);
        assert_eq!(
            pause(MediaKey::Toggle, PlayState::Loading),
            Some(Command::TogglePlay)
        );
        assert_eq!(
            pause(MediaKey::Next, PlayState::Playing),
            Some(Command::Next)
        );
        assert_eq!(
            pause(MediaKey::Previous, PlayState::Playing),
            Some(Command::Previous)
        );
    }

    #[test]
    fn without_a_track_every_key_does_nothing() {
        let playback = playback(PlayState::Playing, 10, 200);
        for key in [
            MediaKey::Play,
            MediaKey::Pause,
            MediaKey::Toggle,
            MediaKey::Stop,
            MediaKey::Next,
            MediaKey::Previous,
            MediaKey::Seek { forward: true },
            MediaKey::SetPosition(secs(5)),
            MediaKey::SetVolume(0.5),
        ] {
            assert_eq!(command_for(key, false, &playback), None, "{key:?}");
        }
    }

    #[test]
    fn seeks_stay_inside_the_track() {
        let playback = playback(PlayState::Playing, 50, 200);
        let key = |key| command_for(key, true, &playback);
        assert_eq!(
            key(MediaKey::Seek { forward: true }),
            Some(Command::Seek(secs(55)))
        );
        assert_eq!(
            key(MediaKey::Seek { forward: false }),
            Some(Command::Seek(secs(45)))
        );
        let by = |forward, s| MediaKey::SeekBy {
            forward,
            by: secs(s),
        };
        assert_eq!(key(by(true, 30)), Some(Command::Seek(secs(80))));
        assert_eq!(key(by(true, 500)), Some(Command::Seek(secs(200))));
        assert_eq!(key(by(false, 500)), Some(Command::Seek(secs(0))));
        assert_eq!(
            key(MediaKey::SetPosition(secs(120))),
            Some(Command::Seek(secs(120)))
        );
        assert_eq!(
            key(MediaKey::SetPosition(secs(900))),
            Some(Command::Seek(secs(200)))
        );
    }

    #[test]
    fn nothing_seeks_while_the_duration_is_unknown() {
        let playback = playback(PlayState::Playing, 0, 0);
        let key = |key| command_for(key, true, &playback);
        assert_eq!(key(MediaKey::Seek { forward: true }), None);
        assert_eq!(
            key(MediaKey::SeekBy {
                forward: true,
                by: secs(5)
            }),
            None
        );
        assert_eq!(key(MediaKey::SetPosition(secs(5))), None);
    }

    #[test]
    fn the_volume_is_clamped() {
        let playback = playback(PlayState::Playing, 0, 200);
        let key = |volume| command_for(MediaKey::SetVolume(volume), true, &playback);
        assert_eq!(key(1.7), Some(Command::SetVolume(1.0)));
        assert_eq!(key(-0.2), Some(Command::SetVolume(0.0)));
        assert_eq!(key(0.5), Some(Command::SetVolume(0.5)));
    }
}
