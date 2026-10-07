//! The queue side panel (ADR 0007). The shell draws it from `QueueState`, so
//! playback ticks, which never notify the shell, never re-render it.

use std::ops::Range;

use cloudrs_ui::Theme;
use cloudrs_ui::components::{Icon, TrackRowData, drag_preview, row_action, track_row};
use cloudrs_ui::tokens::{size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Render, Role, SharedString, Window, div, px, uniform_list};
use sc_core::Command;

use super::{Shell, status_view};
use crate::i18n;
use crate::state::format_time;

/// What a dragged queue row carries.
struct QueueDrag {
    from: usize,
    title: SharedString,
    artist: SharedString,
}

/// The view that follows the pointer while a row is dragged.
pub(crate) struct DragView {
    pub(crate) title: SharedString,
    pub(crate) artist: SharedString,
}

impl Render for DragView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        drag_preview(&Theme::of(cx), self.title.clone(), self.artist.clone())
    }
}

impl Shell {
    pub(super) fn queue_panel(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme.colors;
        let count = self.queue.snapshot.tracks.len();
        let body = if count == 0 {
            status_view(
                theme,
                Icon::Queue,
                "queue-empty",
                i18n::queue::empty_title(),
                i18n::queue::empty_hint(),
                None,
            )
        } else {
            uniform_list(
                "queue-list",
                count,
                cx.processor(|this, range, _, cx| this.queue_rows(range, cx)),
            )
            .track_scroll(&self.queue_scroll)
            .size_full()
            .into_any_element()
        };
        div()
            .id("queue-panel")
            .role(Role::Complementary)
            .aria_label(i18n::queue::title())
            .w(size::QUEUE_PANEL_WIDTH)
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(c.canvas_deep)
            .border_l_1()
            .border_color(c.line)
            .child(
                theme
                    .text(div(), typography::TITLE)
                    .px(space::S5)
                    .py(space::S4)
                    .text_color(c.text)
                    .child(i18n::queue::title()),
            )
            .child(div().flex_1().min_h(px(0.0)).px(space::S2).child(body))
    }

    fn queue_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = Theme::of(cx);
        let c = theme.colors;
        range
            .filter_map(|ix| {
                let snapshot = &self.queue.snapshot;
                let track = snapshot.tracks.get(ix)?;
                let active = snapshot.current == Some(ix);
                let duration = format_time(track.duration);
                let actions = if self.queue.can_remove(ix) {
                    vec![
                        row_action(
                            &theme,
                            ("queue-remove", ix),
                            Icon::Remove,
                            i18n::queue::remove(),
                            cx.listener(move |this, _, _, _| {
                                this.send(Command::RemoveFromQueue(ix));
                            }),
                        )
                        .into_any_element(),
                    ]
                } else {
                    Vec::new()
                };
                let drag = QueueDrag {
                    from: ix,
                    title: track.title.clone().into(),
                    artist: track.artist.clone().into(),
                };
                let row = TrackRowData {
                    index: ix + 1,
                    title: &track.title,
                    artist: &track.artist,
                    duration: &duration,
                    artwork: self.models.art.tracks.get(&track.id).cloned(),
                    active,
                    playing: active && self.models.playing,
                    preview_badge: None,
                    actions,
                    title_link: None,
                    artist_link: None,
                };
                Some(
                    track_row(&theme, ("queue-row", ix), row)
                        .aria_label(i18n::search::play_track(&track.title, &track.artist))
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.send(Command::PlayQueueIndex(ix));
                        }))
                        .on_drag(drag, |drag, _, _, cx| {
                            cx.new(|_| DragView {
                                title: drag.title.clone(),
                                artist: drag.artist.clone(),
                            })
                        })
                        .drag_over::<QueueDrag>(move |style, _, _, _| {
                            style.bg(c.accent_soft).border_color(c.accent)
                        })
                        .on_drop(cx.listener(move |this, drag: &QueueDrag, _, _| {
                            this.send(Command::MoveInQueue {
                                from: drag.from,
                                to: ix,
                            });
                        }))
                        .into_any_element(),
                )
            })
            .collect()
    }
}
