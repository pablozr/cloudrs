//! The M0 window: the design system on screen (tokens, embedded fonts,
//! motion, theme switch and the player bar). Real screens replace it in M1.

use std::time::Duration;

use cloudrs_ui::components::{ButtonKind, badge, button, pill, play_button, waveform};
use cloudrs_ui::tokens::{radius, space, status, typography};
use cloudrs_ui::{Theme, ThemeMode, motion};
use gpui::prelude::*;
use gpui::{Context, Task, Window, div, px};

use crate::i18n::preview as t;

pub struct Preview {
    playing: bool,
    /// 0..=1 through the sample track.
    progress: f32,
    filter: usize,
    samples: Vec<f32>,
    ticker: Option<Task<()>>,
}

/// Length of the pretend track, for the time readout.
const SAMPLE_SECONDS: f32 = 372.0;
const TICK: Duration = Duration::from_millis(100);

impl Preview {
    pub fn new() -> Self {
        Self {
            playing: false,
            progress: 0.29,
            filter: 0,
            samples: sample_waveform(120),
            ticker: None,
        }
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        self.playing = !self.playing;
        self.ticker = self.playing.then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    let alive = this.update(cx, |this, cx| {
                        this.progress =
                            (this.progress + TICK.as_secs_f32() / SAMPLE_SECONDS).min(1.0);
                        cx.notify();
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            })
        });
        cx.notify();
    }
}

/// A deterministic, music-like envelope (no randomness, so screenshots match).
fn sample_waveform(bars: usize) -> Vec<f32> {
    (0..bars)
        .map(|i| {
            let t = i as f32 / bars as f32;
            let v = 0.55
                + 0.25 * (t * 9.1 + 1.0).sin()
                + 0.15 * (t * 23.7).sin()
                + 0.08 * (t * 71.0).sin();
            v.clamp(0.14, 1.0)
        })
        .collect()
}

fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}

impl Render for Preview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let mode = theme.mode;

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px(space::S7)
            .py(space::S5)
            .child(
                theme
                    .text(div(), typography::DISPLAY_L)
                    .flex()
                    .text_color(c.text)
                    .child("cloud")
                    .child(div().text_color(c.accent).child("rs")),
            )
            .child(
                button(
                    &theme,
                    "theme-toggle",
                    match mode {
                        ThemeMode::Dark => t::switch_to_light(),
                        ThemeMode::Light => t::switch_to_dark(),
                    },
                    ButtonKind::Ghost,
                )
                .on_click(|_, window, cx| {
                    let next = cx
                        .try_global::<ThemeMode>()
                        .copied()
                        .unwrap_or_default()
                        .toggled();
                    cx.set_global(next);
                    window.refresh();
                }),
            );

        let filters = [
            t::filter_all(),
            t::filter_tracks(),
            t::filter_people(),
            t::filter_playlists(),
        ];
        let pills = div()
            .flex()
            .gap(space::S2)
            .children(filters.into_iter().enumerate().map(|(i, label)| {
                pill(&theme, ("filter", i), label, i == self.filter).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.filter = i;
                        cx.notify();
                    },
                ))
            }));

        let body = motion::content_in(
            "preview-body",
            div()
                .flex()
                .flex_col()
                .gap(space::S6)
                .px(space::S7)
                .pt(space::S6)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .child(
                            theme
                                .text(div(), typography::DISPLAY_XL)
                                .text_color(c.text)
                                .child(t::headline()),
                        )
                        .child(
                            theme
                                .text(div(), typography::BODY_MUTED)
                                .text_color(c.text_muted)
                                .child(t::subtitle()),
                        ),
                )
                .child(pills)
                .child(
                    div()
                        .flex()
                        .gap(space::S2)
                        .child(button(
                            &theme,
                            "play-all",
                            t::play_all(),
                            ButtonKind::Primary,
                        ))
                        .child(button(&theme, "follow", t::follow(), ButtonKind::Secondary))
                        .child(button(&theme, "shuffle", t::shuffle(), ButtonKind::Outline))
                        .child(button(&theme, "cancel", t::cancel(), ButtonKind::Ghost)),
                )
                .child(
                    div()
                        .flex()
                        .gap(space::S2)
                        .child(badge(&theme, t::badge_aac(), c.accent))
                        .child(badge(&theme, t::badge_preview(), status::warning()))
                        .child(badge(&theme, t::badge_cached(), status::success())),
                ),
        );

        let elapsed = self.progress * SAMPLE_SECONDS;
        let player = div()
            .flex()
            .items_center()
            .gap(space::S6)
            .h(px(84.0))
            .px(space::S5)
            .bg(c.canvas_deep)
            .border_t_1()
            .border_color(c.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S3)
                    .w(px(280.0))
                    .child(div().size(px(52.0)).rounded(radius::M).bg(c.accent_soft))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                theme
                                    .text(div(), typography::BODY)
                                    .text_color(c.text)
                                    .child(t::sample_title()),
                            )
                            .child(
                                theme
                                    .text(div(), typography::BODY_MUTED)
                                    .text_color(c.text_subtle)
                                    .child(t::sample_artist()),
                            ),
                    ),
            )
            .child(
                play_button(&theme, "play", self.playing, px(44.0))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap(space::S3)
                    .child(
                        theme
                            .text(div(), typography::MONO)
                            .text_color(c.text_subtle)
                            .child(clock(elapsed)),
                    )
                    .child(div().flex_1().h(px(32.0)).child(waveform(
                        &theme,
                        self.samples.clone(),
                        self.progress,
                    )))
                    .child(
                        theme
                            .text(div(), typography::MONO)
                            .text_color(c.text_subtle)
                            .child(clock(SAMPLE_SECONDS)),
                    ),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c.canvas)
            .text_color(c.text)
            .child(header)
            .child(div().flex_1().child(body))
            .child(player)
    }
}
