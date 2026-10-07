//! Your library: every playlist and album you made or liked, as a grid of
//! large covers, filtered by kind.

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{skeleton_card, tabs};
use cloudrs_ui::components::{ButtonKind, Icon, button};
use cloudrs_ui::tokens::{space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, div};
use sc_core::{ListItems, PlaylistSummary};

use crate::i18n::{self, library as t};
use crate::intent::UiIntent;
use crate::models::ListId;
use crate::shell::{Shell, status_view};

/// Skeleton cards while the library loads.
const SKELETON_CARDS: usize = 12;

/// Whether a playlist shows under a filter tab (All, Playlists, Albums).
fn shows(tab: usize, playlist: &PlaylistSummary) -> bool {
    match tab {
        1 => !playlist.is_album,
        2 => playlist.is_album,
        _ => true,
    }
}

impl Shell {
    pub(crate) fn library_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        let list = self.models.lists.get(&ListId::Library);
        let (loading, error) = list.map_or((true, false), |l| (l.loading, l.error));
        let all: Vec<PlaylistSummary> = match list.map(|l| &l.items) {
            Some(ListItems::Playlists(playlists)) => playlists.clone(),
            _ => Vec::new(),
        };
        let albums = all.iter().filter(|p| p.is_album).count();
        let meta = i18n::dot_join(&[t::playlists(all.len() - albums), t::albums(albums)]);
        let tab = self.library_tab;
        let filter = tabs(
            theme,
            "library-tabs",
            &[t::tab_all(), t::tab_playlists(), t::tab_albums()],
            tab,
            cx.processor(move |this, ix: usize, _, cx| {
                this.library_tab = ix;
                cx.notify();
            }),
        );

        let body = if error {
            let retry = button(
                theme,
                "retry-library",
                i18n::app::try_again(),
                ButtonKind::Primary,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.dispatch(UiIntent::OpenList(ListId::Library), cx);
            }));
            status_view(
                theme,
                Icon::Alert,
                "library-failed",
                i18n::list::error_title(),
                i18n::list::error_hint(),
                Some(retry),
            )
        } else if loading && all.is_empty() {
            div()
                .flex()
                .flex_wrap()
                .gap(space::S1)
                .children(
                    (0..SKELETON_CARDS).map(|i| skeleton_card(theme, ("library-skeleton", i))),
                )
                .into_any_element()
        } else {
            let shown: Vec<PlaylistSummary> = all.into_iter().filter(|p| shows(tab, p)).collect();
            if shown.is_empty() {
                status_view(
                    theme,
                    Icon::Library,
                    "library-empty",
                    i18n::list::library_empty(),
                    i18n::list::empty_hint(),
                    None,
                )
            } else {
                let key = SharedString::from("library");
                let cards: Vec<AnyElement> = shown
                    .iter()
                    .enumerate()
                    .map(|(ix, playlist)| self.playlist_card(&key, ix, playlist, theme, cx))
                    .collect();
                div()
                    .flex()
                    .flex_wrap()
                    .gap(space::S1)
                    .children(cards)
                    .into_any_element()
            }
        };

        div()
            .id("library")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(space::S4)
            .px(space::S5)
            .pt(space::S2)
            .pb(space::S8)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(
                        theme
                            .text(div(), typography::DISPLAY_XL)
                            .text_color(c.text)
                            .child(t::title()),
                    )
                    .child(
                        theme
                            .text(div(), typography::BODY_MUTED)
                            .text_color(c.text_muted)
                            .child(meta),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(space::S3)
                    .child(filter)
                    .child(
                        button(
                            theme,
                            "library-new",
                            i18n::playlists::new_playlist(),
                            ButtonKind::Primary,
                        )
                        .aria_label(i18n::playlists::new_playlist_title())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_dialog(
                                crate::shell::playlist_ui::Dialog::NewPlaylist { track: None },
                                cx,
                            );
                        })),
                    ),
            )
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::PlaylistId;

    fn playlist(is_album: bool) -> PlaylistSummary {
        PlaylistSummary {
            id: PlaylistId(1),
            title: "Mix".into(),
            owner: "Ana".into(),
            owner_id: None,
            track_count: 3,
            is_album,
        }
    }

    #[test]
    fn each_tab_shows_its_kind() {
        let (set, album) = (playlist(false), playlist(true));
        assert!(shows(0, &set) && shows(0, &album));
        assert!(shows(1, &set) && !shows(1, &album));
        assert!(!shows(2, &set) && shows(2, &album));
    }
}
