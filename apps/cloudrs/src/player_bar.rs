//! The player bar: its own entity, so the ~10 Hz position ticks re-render
//! only this strip and never the results list (ADR 0005). It holds no core
//! handle; it emits [`PlayerAction`]s that the shell forwards.

use std::sync::Arc;
use std::time::Duration;

use cloudrs_ui::Theme;
use cloudrs_ui::components::{play_button, slider, tooltip, waveform};
use cloudrs_ui::tokens::{self, radius, size, space, typography};
use gpui::prelude::*;
use gpui::{Context, EventEmitter, ObjectFit, Window, div, img, px};
use sc_core::Event;

use crate::i18n::player as t;
use crate::state::{ArtworkMap, PlayerState, format_time, seek_target};

/// What the person asked of the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerAction {
    TogglePlay,
    Seek(Duration),
    /// 0.0 to 1.0.
    SetVolume(f32),
}

pub struct PlayerBar {
    state: PlayerState,
    /// Shown as the waveform until (or unless) the track has one.
    flat_waveform: Arc<[f32]>,
    /// Fraction of the waveform under the pointer. Kept here, not in the
    /// element, so the preview re-renders only this bar.
    hover: Option<f32>,
}

impl EventEmitter<PlayerAction> for PlayerBar {}

impl PlayerBar {
    pub fn new() -> Self {
        Self {
            state: PlayerState::new(),
            flat_waveform: vec![tokens::PLACEHOLDER_LEVEL; tokens::PLACEHOLDER_BARS].into(),
            hover: None,
        }
    }

    pub fn apply(&mut self, event: &Event, artwork: &ArtworkMap, cx: &mut Context<Self>) {
        self.state.apply(event, artwork);
        cx.notify();
    }
}

impl Render for PlayerBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let state = &self.state;
        let active = state.track.is_some();
        let playing = state.playing();
        let toggle_label = if playing { t::pause() } else { t::play() };

        let cover = div()
            .flex_none()
            .size(size::PLAYER_COVER)
            .rounded(radius::M)
            .overflow_hidden()
            .bg(c.accent_soft)
            .when_some(state.artwork.clone(), |cover, path| {
                cover.child(img(path).size_full().object_fit(ObjectFit::Cover))
            });
        let (title, subtitle, title_color) = match &state.track {
            Some(track) => (track.title.as_str(), track.artist.as_str(), c.text),
            None => (
                t::nothing_playing(),
                t::nothing_playing_hint(),
                c.text_muted,
            ),
        };
        let info = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::S3)
            .w(size::PLAYER_INFO_WIDTH)
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

        let play = play_button(&theme, "play", playing, size::PLAY_BUTTON)
            .aria_label(toggle_label)
            .tooltip(tooltip(toggle_label))
            .when(active, |button| {
                button.on_click(cx.listener(|_, _, _, cx| cx.emit(PlayerAction::TogglePlay)))
            })
            .when(!active, |button| button.opacity(tokens::DISABLED_OPACITY));

        let time = |text: String| {
            theme
                .text(div(), typography::MONO)
                .flex_none()
                .text_color(c.text_subtle)
                .child(text)
        };
        let samples = state
            .waveform
            .clone()
            .unwrap_or_else(|| self.flat_waveform.clone());
        let timeline = div()
            .flex()
            .flex_1()
            .items_center()
            .gap(space::S3)
            .when(!active, |timeline| {
                timeline.opacity(tokens::DISABLED_OPACITY)
            })
            .child(time(format_time(state.playback.position)))
            .child(
                div().flex_1().h(size::WAVEFORM_HEIGHT).child(
                    waveform(
                        &theme,
                        "seek",
                        samples,
                        state.progress(),
                        self.hover,
                        cx.processor(|this, hover: Option<f32>, _, cx| {
                            // No target to preview without a duration or real bars.
                            let seekable = !this.state.playback.duration.is_zero()
                                && this.state.waveform.is_some();
                            let hover = hover.filter(|_| seekable);
                            if this.hover != hover {
                                this.hover = hover;
                                cx.notify();
                            }
                        }),
                        cx.processor(|this, fraction: f32, _, cx| {
                            let duration = this.state.playback.duration;
                            if !duration.is_zero() {
                                cx.emit(PlayerAction::Seek(seek_target(fraction, duration)));
                            }
                        }),
                    )
                    .aria_label(t::seek()),
                ),
            )
            .child(time(format_time(state.playback.duration)));

        let volume = div().flex_none().w(size::VOLUME_WIDTH).child(
            slider(
                &theme,
                "volume",
                state.playback.volume,
                cx.processor(|_, volume: f32, _, cx| cx.emit(PlayerAction::SetVolume(volume))),
            )
            .aria_label(t::volume())
            .tooltip(tooltip(t::volume())),
        );

        div()
            .flex()
            .items_center()
            .gap(space::S6)
            .h(size::PLAYER_HEIGHT)
            .px(space::S5)
            .bg(c.canvas_deep)
            .border_t_1()
            .border_color(c.line)
            .child(info)
            .child(play)
            .child(timeline)
            .child(volume)
    }
}
