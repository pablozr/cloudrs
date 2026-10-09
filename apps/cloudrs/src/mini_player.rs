//! The mini player: a small window with the cover, the title, the transport
//! and a progress line (ADR 0023). It holds no core handle. The shell feeds it
//! the same events as the player bar and turns its [`MiniAction`]s into
//! commands, so there is still one event pump.

use std::path::Path;
use std::sync::Arc;

use cloudrs_ui::Theme;
use cloudrs_ui::components::{Icon, icon_button, play_button, tooltip};
use cloudrs_ui::tokens::{self, radius, size, space, typography};
use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, EventEmitter, FocusHandle, Focusable, MouseButton, ObjectFit, Pixels,
    TitlebarOptions, Window, WindowBounds, WindowControlArea, WindowDecorations, WindowHandle,
    WindowKind, WindowOptions, div, img, point, px, relative,
};
use sc_core::{Event, TrackId};

use crate::i18n::{mini as t, player};
use crate::shell::shortcuts::{
    MINI_PLAYER, NextTrack, PreviousTrack, ToggleMiniPlayer, TogglePlay,
};
use crate::state::{ArtworkMap, PlayerState};

/// What the person asked of the mini player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniAction {
    TogglePlay,
    Previous,
    Next,
    /// Bring the main window to the front.
    ShowMain,
    /// The window went away (its close button, the shortcut or the OS).
    Closed,
}

pub struct MiniPlayer {
    state: PlayerState,
    focus: FocusHandle,
}

impl EventEmitter<MiniAction> for MiniPlayer {}

impl Focusable for MiniPlayer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Opens the mini player showing `state`. `None` when the OS refuses the
/// window; the app goes on without it.
pub fn open(state: PlayerState, cx: &mut App) -> Option<WindowHandle<MiniPlayer>> {
    let opened = cx.open_window(options(cx), |window, cx| {
        cx.new(|cx| MiniPlayer::new(state, window, cx))
    });
    match opened {
        Ok(handle) => Some(handle),
        Err(error) => {
            tracing::warn!(%error, "failed to open the mini player");
            None
        }
    }
}

fn options(cx: &App) -> WindowOptions {
    let display = cx.primary_display().map(|display| display.bounds());
    WindowOptions {
        window_bounds: display.map(|display| WindowBounds::Windowed(initial_bounds(display))),
        titlebar: Some(TitlebarOptions {
            title: Some(t::window_title().into()),
            // The mini player draws its own surface.
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_decorations: Some(WindowDecorations::Client),
        is_resizable: false,
        is_minimizable: false,
        app_id: Some("dev.cloudrs.cloudrs".into()),
        kind: window_kind(),
        // Opening it leaves the window the person is using active; a click on it
        // activates it. Only Windows honors this in the pinned GPUI.
        focus: false,
        ..Default::default()
    }
}

/// Always on top and without a taskbar button on Windows (a pop-up); a normal
/// window elsewhere, where the platform may not honor "on top" (ADR 0023).
fn window_kind() -> WindowKind {
    if cfg!(windows) {
        WindowKind::PopUp
    } else {
        WindowKind::Normal
    }
}

/// The bottom right corner of `display`, inside a margin.
pub fn initial_bounds(display: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = size::MINI_PLAYER_WIDTH;
    let height = size::MINI_PLAYER_HEIGHT;
    let margin = space::S6;
    Bounds::new(
        point(
            display.origin.x + display.size.width - width - margin,
            display.origin.y + display.size.height - height - margin,
        ),
        gpui::size(width, height),
    )
}

/// What the mini player draws; it redraws only when this changes.
#[derive(Debug, PartialEq)]
struct Shown {
    track: Option<TrackId>,
    artwork: Option<Arc<Path>>,
    playing: bool,
    progress_px: u32,
}

fn shown(state: &PlayerState) -> Shown {
    Shown {
        track: state.track.as_ref().map(|track| track.id),
        artwork: state.artwork.clone(),
        playing: state.playing(),
        progress_px: (state.progress() * f32::from(size::MINI_PLAYER_WIDTH)).round() as u32,
    }
}

impl MiniPlayer {
    fn new(state: PlayerState, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            this.update(cx, |_, cx| cx.emit(MiniAction::Closed)).ok();
            true
        });
        let focus = cx.focus_handle();
        // Focus inside its own window, so its keys work once it is clicked.
        window.focus(&focus, cx);
        Self { state, focus }
    }

    /// Follows the same events as the player bar, but re-renders only when
    /// what it draws changes.
    pub fn apply(&mut self, event: &Event, artwork: &ArtworkMap, cx: &mut Context<Self>) {
        let before = shown(&self.state);
        self.state.apply(event, artwork);
        if shown(&self.state) != before {
            cx.notify();
        }
    }

    /// Closes the window and tells the shell.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        cx.emit(MiniAction::Closed);
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let state = &self.state;
        let active = state.track.is_some();
        let playing = state.playing();
        let toggle_label = if playing {
            player::pause()
        } else {
            player::play()
        };
        let dim = |on: bool| if on { 1.0 } else { tokens::DISABLED_OPACITY };

        let cover = div()
            .flex_none()
            .size(size::MINI_PLAYER_COVER)
            .rounded(radius::M)
            .overflow_hidden()
            .bg(c.accent_soft)
            .when_some(state.artwork.clone(), |cover, path| {
                cover.child(img(path).size_full().object_fit(ObjectFit::Cover))
            });
        let (title, subtitle, title_color) = match &state.track {
            Some(track) => (track.title.as_str(), track.artist.as_str(), c.text),
            None => (
                player::nothing_playing(),
                player::nothing_playing_hint(),
                c.text_muted,
            ),
        };
        // The part of the window that drags it; the buttons stay outside it.
        let info = div()
            .flex()
            .flex_1()
            .min_w(px(0.0))
            .items_center()
            .gap(space::S3)
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                // Windows drags through the control area above; Linux asks.
                if cfg!(target_os = "linux") {
                    window.start_window_move();
                }
            })
            .child(cover)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(
                        theme
                            .text(div(), typography::BODY)
                            .truncate()
                            .text_color(title_color)
                            .child(title.to_owned()),
                    )
                    .child(
                        theme
                            .text(div(), typography::BODY_MUTED)
                            .truncate()
                            .text_color(c.text_subtle)
                            .child(subtitle.to_owned()),
                    ),
            );

        let previous = icon_button(&theme, "mini-previous", Icon::Previous, false)
            .aria_label(player::previous())
            .tooltip(tooltip(player::previous()))
            .opacity(dim(active))
            .when(active, |button| {
                button.on_click(cx.listener(|_, _, _, cx| cx.emit(MiniAction::Previous)))
            });
        let next = icon_button(&theme, "mini-next", Icon::Next, false)
            .aria_label(player::next())
            .tooltip(tooltip(player::next()))
            .opacity(dim(active))
            .when(active, |button| {
                button.on_click(cx.listener(|_, _, _, cx| cx.emit(MiniAction::Next)))
            });
        let play = play_button(&theme, "mini-play", playing, size::PLAY_BUTTON)
            .aria_label(toggle_label)
            .tooltip(tooltip(toggle_label))
            .opacity(dim(active))
            .when(active, |button| {
                button.on_click(cx.listener(|_, _, _, cx| cx.emit(MiniAction::TogglePlay)))
            });
        let show_main = icon_button(&theme, "mini-open-main", Icon::Maximize, false)
            .aria_label(t::open_main())
            .tooltip(tooltip(t::open_main()))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(MiniAction::ShowMain)));
        let close = icon_button(&theme, "mini-close", Icon::Remove, false)
            .aria_label(t::close())
            .tooltip(tooltip(t::close()))
            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)));

        let transport = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::S1)
            .child(previous)
            .child(play)
            .child(next);
        let window_buttons = div()
            .flex()
            .flex_none()
            .flex_col()
            .gap(space::S1)
            .child(show_main)
            .child(close);
        let progress = div()
            .flex_none()
            .w_full()
            .h(size::MINI_PROGRESS_HEIGHT)
            .bg(c.line)
            .child(div().h_full().w(relative(state.progress())).bg(c.accent));

        div()
            .key_context(MINI_PLAYER)
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &TogglePlay, _, cx| cx.emit(MiniAction::TogglePlay)))
            .on_action(cx.listener(|_, _: &PreviousTrack, _, cx| cx.emit(MiniAction::Previous)))
            .on_action(cx.listener(|_, _: &NextTrack, _, cx| cx.emit(MiniAction::Next)))
            .on_action(cx.listener(|this, _: &ToggleMiniPlayer, window, cx| {
                this.close(window, cx);
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(c.canvas_deep)
            .border_1()
            .border_color(c.line)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .items_center()
                    .gap(space::S2)
                    .px(space::S3)
                    .child(info)
                    .child(transport)
                    .child(window_buttons),
            )
            .child(progress)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sc_core::{PlayState, Playback, QueueSnapshot, Repeat, TrackSummary};

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

    fn playback(state: PlayState, position: f32) -> Event {
        Event::Playback(Playback {
            state,
            position: Duration::from_secs_f32(position),
            duration: Duration::from_secs(200),
            volume: 0.5,
        })
    }

    fn state_playing_at(position: f32) -> PlayerState {
        let mut state = PlayerState::new();
        let artwork = ArtworkMap::new();
        state.apply(&Event::NowPlaying(track(1)), &artwork);
        state.apply(&playback(PlayState::Playing, position), &artwork);
        state
    }

    fn display() -> Bounds<Pixels> {
        Bounds::new(
            point(px(100.0), px(50.0)),
            gpui::size(px(1920.0), px(1080.0)),
        )
    }

    #[test]
    fn the_window_opens_inside_the_display_at_the_corner() {
        let display = display();
        let bounds = initial_bounds(display);
        assert_eq!(bounds.size.width, size::MINI_PLAYER_WIDTH);
        assert_eq!(bounds.size.height, size::MINI_PLAYER_HEIGHT);
        assert!(display.contains(&bounds.origin));
        assert!(display.contains(&bounds.bottom_right()));
        let margin = space::S6;
        assert_eq!(display.bottom_right().x - bounds.bottom_right().x, margin);
        assert_eq!(display.bottom_right().y - bounds.bottom_right().y, margin);
    }

    #[test]
    fn only_windows_makes_a_pop_up() {
        let expected = if cfg!(windows) {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        };
        assert_eq!(window_kind(), expected);
    }

    #[test]
    fn a_tick_inside_the_same_pixel_changes_nothing() {
        let mut state = state_playing_at(100.0);
        let before = shown(&state);
        state.apply(&playback(PlayState::Playing, 100.1), &ArtworkMap::new());
        assert_eq!(shown(&state), before);
    }

    #[test]
    fn what_the_mini_draws_counts_as_a_change() {
        let artwork = ArtworkMap::new();
        let mut state = state_playing_at(100.0);

        let before = shown(&state);
        state.apply(&playback(PlayState::Playing, 106.0), &artwork);
        assert_ne!(shown(&state), before, "the progress line moved");

        let before = shown(&state);
        state.apply(&playback(PlayState::Paused, 106.0), &artwork);
        assert_ne!(shown(&state), before, "play became pause");

        let before = shown(&state);
        state.apply(&Event::NowPlaying(track(2)), &artwork);
        assert_ne!(shown(&state), before, "another track");

        let before = shown(&state);
        state.apply(
            &Event::Queue(QueueSnapshot {
                shuffle: true,
                repeat: Repeat::Off,
                ..QueueSnapshot::default()
            }),
            &artwork,
        );
        assert_eq!(shown(&state), before, "shuffle is not drawn");
    }
}
