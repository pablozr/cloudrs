//! A playlist or album page: header, a Play button, then the tracks.

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{PageHeaderData, page_header, skeleton_header};
use cloudrs_ui::components::{ButtonKind, button};
use cloudrs_ui::tokens::{self, space};
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};
use sc_core::{ListItems, PlaylistId, PlaylistPage};

use crate::i18n::{self, playlist as t};
use crate::intent::UiIntent;
use crate::models::{ListId, Page};
use crate::shell::{Shell, status_view};
use crate::state::format_time;

/// `Album · Ana · 12 tracks · 48:10`
fn meta(header: &PlaylistPage) -> String {
    let kind = if header.is_album {
        t::album_badge()
    } else {
        t::kind_playlist()
    };
    i18n::dot_join(&[
        kind.to_owned(),
        header.owner.clone(),
        i18n::count::tracks(header.track_count),
        format_time(header.duration),
    ])
}

impl Shell {
    pub(crate) fn playlist_screen(
        &mut self,
        id: PlaylistId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = match self.models.playlists.get(&id) {
            Some(Page::Ready(header)) => header,
            Some(Page::Failed) => {
                let retry = button(
                    theme,
                    "retry-page",
                    i18n::app::try_again(),
                    ButtonKind::Primary,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.dispatch(UiIntent::OpenPlaylist(id), cx);
                }));
                return status_view(
                    theme,
                    "playlist-failed",
                    i18n::page::error_title(),
                    i18n::page::error_hint(),
                    Some(retry),
                );
            }
            Some(Page::Loading) | None => {
                return div()
                    .size_full()
                    .pt(space::S2)
                    .child(skeleton_header(theme, "playlist-skeleton", false))
                    .into_any_element();
            }
        };

        let key = ListId::Playlist(id);
        let first = match self.models.lists.get(&key).map(|list| &list.items) {
            Some(ListItems::Tracks(items)) => items.first().map(|track| track.id),
            _ => None,
        };
        let play = button(theme, "play-playlist", t::play(), ButtonKind::Primary)
            .aria_label(i18n::playlist::play())
            .when_some(first, |play, track| {
                play.on_click(cx.listener(move |this, _, _, cx| {
                    this.dispatch(UiIntent::Play { list: key, track }, cx);
                }))
            })
            .when(first.is_none(), |play| {
                play.opacity(tokens::DISABLED_OPACITY)
            });

        let owner = header.owner_id.map(|user| {
            button(
                theme,
                "playlist-owner",
                header.owner.clone(),
                ButtonKind::Secondary,
            )
            .aria_label(i18n::user::open_profile(&header.owner))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dispatch(UiIntent::OpenUser(user), cx);
            }))
            .into_any_element()
        });
        let meta = meta(header);
        let page = page_header(
            theme,
            PageHeaderData {
                artwork: self.models.art.playlists.get(&id).cloned(),
                round: false,
                title: &header.title,
                meta: &meta,
                actions: [Some(play.into_any_element()), owner]
                    .into_iter()
                    .flatten()
                    .collect(),
            },
        );
        let list = self.list_view(key, i18n::search::results(), theme, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .pt(space::S2)
            .child(page)
            .child(div().flex_1().min_h(px(0.0)).px(space::S5).child(list))
            .into_any_element()
    }
}
