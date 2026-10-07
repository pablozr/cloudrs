//! A profile page: header, then the Tracks, Playlists and Likes tabs.

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{PageHeaderData, page_header, skeleton_header, tabs};
use cloudrs_ui::components::{ButtonKind, Icon, button};
use cloudrs_ui::tokens::space;
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};

use crate::i18n::{self, user as t};
use crate::intent::UiIntent;
use crate::models::{ListId, Page};
use crate::shell::{Shell, status_view};
use crate::state::compact_count;
use sc_core::{UserId, UserPage};

/// The list behind each tab, in tab order.
fn tab_list(tab: usize, id: UserId) -> ListId {
    match tab {
        1 => ListId::UserPlaylists(id),
        2 => ListId::UserLikes(id),
        _ => ListId::UserTracks(id),
    }
}

/// `Lisbon · 12K followers · 340 following · 56 tracks`
fn meta(header: &UserPage) -> String {
    let mut parts: Vec<String> = header.city.iter().cloned().collect();
    parts.extend(
        header
            .followers
            .map(|count| t::followers(compact_count(count))),
    );
    parts.extend(
        header
            .followings
            .map(|count| t::following(compact_count(count))),
    );
    parts.extend(header.track_count.map(i18n::count::tracks));
    i18n::dot_join(&parts)
}

impl Shell {
    pub(crate) fn user_screen(
        &mut self,
        id: UserId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = match self.models.users.get(&id) {
            Some(Page::Ready(header)) => header,
            Some(Page::Failed) => {
                let retry = button(
                    theme,
                    "retry-page",
                    i18n::app::try_again(),
                    ButtonKind::Primary,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.dispatch(UiIntent::OpenUser(id), cx);
                }));
                return status_view(
                    theme,
                    Icon::Alert,
                    "user-failed",
                    i18n::page::error_title(),
                    i18n::page::error_hint(),
                    Some(retry),
                );
            }
            Some(Page::Loading) | None => {
                return div()
                    .size_full()
                    .pt(space::S2)
                    .child(skeleton_header(theme, "user-skeleton", true))
                    .into_any_element();
            }
        };

        let meta = meta(header);
        let page = page_header(
            theme,
            PageHeaderData {
                artwork: self.models.art.users.get(&id).cloned(),
                round: true,
                title: &header.username,
                meta: &meta,
                actions: Vec::new(),
            },
        );
        let tab = self.user_tab;
        let tabs = tabs(
            theme,
            "user-tabs",
            &[t::tab_tracks(), t::tab_playlists(), t::tab_likes()],
            tab,
            cx.processor(move |this, ix: usize, _, cx| {
                this.user_tab = ix;
                let list = tab_list(ix, id);
                // A tab is requested the first time it is opened.
                if this.models.lists.contains_key(&list) {
                    cx.notify();
                } else {
                    this.dispatch(UiIntent::OpenList(list), cx);
                }
            }),
        );
        let list = self.list_view(tab_list(tab, id), i18n::search::results(), theme, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .pt(space::S2)
            .child(page)
            .child(div().px(space::S5).pb(space::S3).child(tabs))
            .child(div().flex_1().min_h(px(0.0)).px(space::S5).child(list))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tab_shows_its_own_list() {
        assert_eq!(tab_list(0, UserId(9)), ListId::UserTracks(UserId(9)));
        assert_eq!(tab_list(1, UserId(9)), ListId::UserPlaylists(UserId(9)));
        assert_eq!(tab_list(2, UserId(9)), ListId::UserLikes(UserId(9)));
    }
}
