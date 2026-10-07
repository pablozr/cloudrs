//! A track page: header, a large waveform, the description and related tracks.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{PageHeaderData, page_header, skeleton_header};
use cloudrs_ui::components::{ButtonKind, Icon, button, icon_button, tooltip, waveform};
use cloudrs_ui::tokens::{self, size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, EventEmitter, Window, div, px};
use sc_core::{Event, Playback, TrackId, TrackPage};

use crate::i18n;
use crate::intent::UiIntent;
use crate::models::{ListId, Page};
use crate::shell::{Shell, status_view};
use crate::state::{compact_count, format_time, seek_target};

/// Waveforms kept at most; the oldest are dropped all at once.
const MAX_WAVEFORMS: usize = 16;

/// What the person did with the large waveform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaveAction {
    Seek(Duration),
    /// Another track than the one playing: play it.
    Play(TrackId),
}

/// The large waveform of a track page. Its own entity, like the player bar,
/// so the ~10 Hz position ticks re-render only this strip.
pub struct TrackWave {
    /// The track the page shows.
    page: Option<TrackId>,
    /// The track the player is on.
    current: Option<TrackId>,
    /// Waveforms received, so a page keeps its bars while another track plays.
    samples: HashMap<TrackId, Arc<[f32]>>,
    playback: Playback,
    flat: Arc<[f32]>,
    hover: Option<f32>,
}

impl EventEmitter<WaveAction> for TrackWave {}

impl TrackWave {
    pub fn new() -> Self {
        Self {
            page: None,
            current: None,
            samples: HashMap::new(),
            playback: Playback::default(),
            flat: vec![tokens::PLACEHOLDER_LEVEL; tokens::PLACEHOLDER_BARS].into(),
            hover: None,
        }
    }

    pub fn show(&mut self, track: Option<TrackId>, cx: &mut Context<Self>) {
        if self.page != track {
            self.page = track;
            self.hover = None;
            cx.notify();
        }
    }

    pub fn apply(&mut self, event: &Event, cx: &mut Context<Self>) {
        match event {
            Event::NowPlaying(track) => self.current = Some(track.id),
            Event::Waveform { track, bars } => {
                if self.samples.len() >= MAX_WAVEFORMS {
                    self.samples.clear();
                }
                self.samples.insert(*track, Arc::from(bars.as_slice()));
            }
            Event::Playback(playback) => self.playback = *playback,
            _ => return,
        }
        // Off the track page nothing shows it: keep the state, skip the redraw.
        if self.page.is_some() {
            cx.notify();
        }
    }

    fn is_current(&self) -> bool {
        self.page.is_some() && self.page == self.current
    }
}

impl Render for TrackWave {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let current = self.is_current();
        let samples = self
            .page
            .and_then(|page| self.samples.get(&page))
            .map_or_else(|| self.flat.clone(), Arc::clone);
        let duration = self.playback.duration;
        let progress = if current && !duration.is_zero() {
            (self.playback.position.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let page = self.page;

        div().w_full().h(size::WAVEFORM_LARGE_HEIGHT).child(
            waveform(
                &theme,
                "track-waveform",
                samples,
                progress,
                self.hover.filter(|_| current),
                cx.processor(|this, hover: Option<f32>, _, cx| {
                    let hover = hover.filter(|_| this.is_current());
                    if this.hover != hover {
                        this.hover = hover;
                        cx.notify();
                    }
                }),
                cx.processor(move |this, fraction: f32, _, cx| {
                    if this.is_current() {
                        let duration = this.playback.duration;
                        if !duration.is_zero() {
                            cx.emit(WaveAction::Seek(seek_target(fraction, duration)));
                        }
                    } else if let Some(track) = page {
                        cx.emit(WaveAction::Play(track));
                    }
                }),
            )
            .aria_label(i18n::player::seek()),
        )
    }
}

/// `Ana · 3:45 · 1.2K plays`
fn meta(header: &TrackPage) -> String {
    let mut parts = vec![
        header.track.artist.clone(),
        format_time(header.track.duration),
    ];
    parts.extend(
        header
            .plays
            .map(|plays| i18n::track::plays(compact_count(plays))),
    );
    i18n::dot_join(&parts)
}

impl Shell {
    pub(crate) fn track_screen(
        &mut self,
        id: TrackId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = match self.models.tracks.get(&id) {
            Some(Page::Ready(header)) => header,
            Some(Page::Failed) => {
                let retry = button(
                    theme,
                    "retry-page",
                    i18n::app::try_again(),
                    ButtonKind::Primary,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.dispatch(UiIntent::OpenTrack(id), cx);
                }));
                return status_view(
                    theme,
                    Icon::Alert,
                    "track-failed",
                    i18n::page::error_title(),
                    i18n::page::error_hint(),
                    Some(retry),
                );
            }
            Some(Page::Loading) | None => {
                return div()
                    .size_full()
                    .pt(space::S2)
                    .child(skeleton_header(theme, "track-skeleton", false))
                    .into_any_element();
            }
        };

        let artist = header.track.artist_id.map(|user| {
            button(
                theme,
                "track-artist",
                header.track.artist.clone(),
                ButtonKind::Secondary,
            )
            .aria_label(i18n::track::open_profile(&header.track.artist))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dispatch(UiIntent::OpenUser(user), cx);
            }))
            .into_any_element()
        });
        let like = self.models.account.is_some().then(|| {
            let liked = self.models.liked.contains(&id);
            let (glyph, label) = if liked {
                (Icon::HeartFilled, i18n::social::unlike())
            } else {
                (Icon::Heart, i18n::social::like())
            };
            icon_button(theme, "track-like", glyph, liked)
                .aria_label(label)
                .tooltip(tooltip(label))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.dispatch(
                        UiIntent::Like {
                            track: id,
                            liked: !liked,
                        },
                        cx,
                    );
                }))
                .into_any_element()
        });
        let add = self.models.account.is_some().then(|| {
            let label = i18n::playlists::add_to_playlist();
            icon_button(theme, "track-add-to-playlist", Icon::Plus, false)
                .aria_label(label)
                .tooltip(tooltip(label))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_playlist_menu(id, window.mouse_position(), cx);
                }))
                .into_any_element()
        });
        let meta = meta(header);
        let page = page_header(
            theme,
            PageHeaderData {
                artwork: self.models.art.tracks.get(&id).cloned(),
                round: false,
                title: &header.track.title,
                meta: &meta,
                actions: artist.into_iter().chain(like).chain(add).collect(),
            },
        );
        let description = header.description.as_ref().map(|text| {
            theme
                .text(div(), typography::BODY_MUTED)
                .max_w(size::DESCRIPTION_MAX_WIDTH)
                .pb(space::S4)
                .line_clamp(3)
                .text_color(theme.colors.text_muted)
                .child(text.clone())
        });
        let related = self.list_view(ListId::Related(id), i18n::track::related(), theme, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .pt(space::S2)
            .child(page)
            .child(div().px(space::S5).pb(space::S4).child(self.wave.clone()))
            .child(div().px(space::S5).children(description))
            .child(
                theme
                    .text(div(), typography::TITLE)
                    .px(space::S5)
                    .pb(space::S2)
                    .text_color(theme.colors.text)
                    .child(i18n::track::related()),
            )
            .child(div().flex_1().min_h(px(0.0)).px(space::S5).child(related))
            .into_any_element()
    }
}
