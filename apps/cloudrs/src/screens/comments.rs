//! Timed comments on the track page (ADR 0021): the pin lane under the large
//! waveform, the popover over a pin, and the Comments tab.

use std::ops::Range;
use std::time::Duration;

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{CommentRowData, comment_popover, comment_row};
use cloudrs_ui::components::{ButtonKind, Icon, button, comment_lane, skeleton_row};
use cloudrs_ui::tokens::{size, space};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Pixels, Point, Role, Window, anchored, deferred, div, point, px,
    uniform_list,
};
use sc_core::{ArtKey, Command, UserId, WAVEFORM_BARS};

use crate::comments::{POPOVER_COMMENTS, marker_near};
use crate::i18n;
use crate::intent::UiIntent;
use crate::models::{ListId, Page, TrackId};
use crate::nav::Route;
use crate::shell::{Shell, status_view};
use crate::state::format_time;

/// Skeleton rows while the comments load.
const SKELETONS: usize = 8;

impl Shell {
    /// A click on a time: seeks when the track is the one playing, otherwise
    /// plays it from the start (ADR 0021).
    fn pick_time(&mut self, track: TrackId, at: Duration, cx: &mut Context<Self>) {
        if self.models.current == Some(track) {
            self.send(Command::Seek(at));
        } else {
            let list = ListId::Related(track);
            self.dispatch(UiIntent::Play { list, track }, cx);
        }
    }

    /// The pin under the pointer changed: remember it and ask for the avatars
    /// its popover shows. Notifies only on change.
    fn hover_comment(
        &mut self,
        track: TrackId,
        fraction: Option<f32>,
        place: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(Page::Ready(view)) = self.models.comments.get(&track) else {
            return;
        };
        let near = fraction.and_then(|at| marker_near(&view.markers, at, WAVEFORM_BARS));
        let shown = self
            .comment_hover
            .filter(|(hovered, ..)| *hovered == track)
            .map(|(_, ix, _)| ix);
        if near == shown {
            return;
        }
        let people: Vec<UserId> = near
            .and_then(|ix| view.markers.get(ix))
            .map(|marker| {
                marker
                    .comments
                    .iter()
                    .take(POPOVER_COMMENTS)
                    .filter_map(|ix| view.items[*ix as usize].user)
                    .collect()
            })
            .unwrap_or_default();
        self.comment_hover = near.map(|ix| (track, ix, place));
        self.ask_avatars(people);
        cx.notify();
    }

    fn pick_pin(&mut self, track: TrackId, fraction: f32, cx: &mut Context<Self>) {
        let Some(Page::Ready(view)) = self.models.comments.get(&track) else {
            return;
        };
        let at = marker_near(&view.markers, fraction, WAVEFORM_BARS)
            .and_then(|ix| view.markers.get(ix))
            .map(|marker| marker.at);
        if let Some(at) = at {
            self.pick_time(track, at, cx);
        }
    }

    /// Asks the core for the avatars not asked for yet.
    fn ask_avatars(&mut self, people: impl IntoIterator<Item = UserId>) {
        for user in people {
            if self.models.art.ask_avatar(user) {
                self.send(Command::LoadArtwork(ArtKey::User(user)));
            }
        }
    }

    /// The strip of pins under the waveform. Space is kept while the comments
    /// load; nothing is drawn when they are off, failed or have no timed one.
    pub(crate) fn comment_lane_view(
        &self,
        track: TrackId,
        commentable: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !commentable {
            return None;
        }
        let view = match self.models.comments.get(&track) {
            Some(Page::Ready(view)) if !view.pins.is_empty() => view,
            Some(Page::Ready(_) | Page::Failed) => return None,
            Some(Page::Loading) | None => {
                return Some(div().h(size::COMMENT_LANE).into_any_element());
            }
        };
        let active = self
            .comment_hover
            .filter(|(hovered, ..)| *hovered == track)
            .and_then(|(_, ix, _)| view.markers.get(ix))
            .map(|marker| marker.bar);
        Some(
            comment_lane(
                theme,
                "track-comment-lane",
                WAVEFORM_BARS,
                view.pins.clone(),
                active,
                cx.processor(
                    move |this, fraction: Option<f32>, window: &mut Window, cx| {
                        this.hover_comment(track, fraction, window.mouse_position(), cx);
                    },
                ),
                cx.processor(move |this, fraction: f32, _, cx| this.pick_pin(track, fraction, cx)),
            )
            .aria_label(i18n::comments::lane())
            .into_any_element(),
        )
    }

    /// The popover over the hovered pin, above everything else.
    pub(crate) fn comment_popover_view(&self, theme: &Theme) -> Option<AnyElement> {
        let (track, ix, at) = self.comment_hover?;
        if *self.router.current() != Route::Track(track) {
            return None;
        }
        let Some(Page::Ready(view)) = self.models.comments.get(&track) else {
            return None;
        };
        let marker = view.markers.get(ix)?;
        let shown = &marker.comments[..marker.comments.len().min(POPOVER_COMMENTS)];
        let times: Vec<String> = shown
            .iter()
            .map(|ix| format_time(view.items[*ix as usize].at.unwrap_or_default()))
            .collect();
        let entries = shown
            .iter()
            .zip(&times)
            .map(|(ix, time)| {
                let comment = &view.items[*ix as usize];
                CommentRowData {
                    name: &comment.username,
                    time: Some(time),
                    body: &comment.body,
                    avatar: comment
                        .user
                        .and_then(|user| self.models.art.users.get(&user).cloned()),
                }
            })
            .collect();
        let left = marker.comments.len() - shown.len();
        let more = (left > 0).then(|| i18n::comments::more(left));
        let popover = comment_popover(theme, "comment-popover", entries, more);
        Some(
            deferred(
                anchored()
                    .position(at)
                    .offset(point(px(0.0), space::S4))
                    .snap_to_window()
                    .child(popover),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The Comments tab with whatever state the comments are in.
    pub(crate) fn comments_view(
        &mut self,
        track: TrackId,
        commentable: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use i18n::comments as t;
        if !commentable {
            return status_view(
                theme,
                Icon::Private,
                "comments-off",
                t::off_title(),
                t::off_hint(),
                None,
            );
        }
        let len = match self.models.comments.get(&track) {
            None | Some(Page::Loading) => {
                return div()
                    .size_full()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .pt(space::S2)
                    .children((0..SKELETONS).map(|i| skeleton_row(theme, ("comment-skeleton", i))))
                    .into_any_element();
            }
            Some(Page::Failed) => {
                let retry = button(
                    theme,
                    "retry-comments",
                    i18n::app::try_again(),
                    ButtonKind::Primary,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.dispatch(UiIntent::LoadComments(track), cx);
                }));
                return status_view(
                    theme,
                    Icon::Alert,
                    "comments-failed",
                    t::error_title(),
                    i18n::list::error_hint(),
                    Some(retry),
                );
            }
            Some(Page::Ready(view)) => view.items.len(),
        };
        if len == 0 {
            return status_view(
                theme,
                Icon::People,
                "comments-empty",
                t::empty_title(),
                t::empty_hint(),
                None,
            );
        }
        div()
            .id("comments")
            .role(Role::List)
            .aria_label(t::tab())
            .size_full()
            .child(
                uniform_list(
                    "track-comments",
                    len,
                    cx.processor(move |this, range, _, cx| this.comment_rows(track, range, cx)),
                )
                .track_scroll(&self.comments_scroll)
                .size_full(),
            )
            .into_any_element()
    }

    /// The visible rows. Their avatars are asked for here, once each.
    fn comment_rows(
        &mut self,
        track: TrackId,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(Page::Ready(view)) = self.models.comments.get(&track) else {
            return Vec::new();
        };
        let people: Vec<UserId> = range
            .clone()
            .filter_map(|ix| view.items.get(ix)?.user)
            .collect();
        self.ask_avatars(people);

        let Some(Page::Ready(view)) = self.models.comments.get(&track) else {
            return Vec::new();
        };
        let theme = Theme::of(cx);
        range
            .filter_map(|ix| {
                let comment = view.items.get(ix)?;
                let time = comment.at.map(format_time);
                let label = match &time {
                    Some(time) => i18n::comments::row_label(&comment.username, time),
                    None => i18n::comments::row_label_untimed(&comment.username),
                };
                let row = comment_row(
                    &theme,
                    ("comment", ix),
                    CommentRowData {
                        name: &comment.username,
                        time: time.as_deref(),
                        body: &comment.body,
                        avatar: comment
                            .user
                            .and_then(|user| self.models.art.users.get(&user).cloned()),
                    },
                )
                .aria_label(label);
                Some(match comment.at {
                    Some(at) => row
                        .on_click(cx.listener(move |this, _, _, cx| this.pick_time(track, at, cx)))
                        .into_any_element(),
                    None => row.into_any_element(),
                })
            })
            .collect()
    }
}
