//! Home, where cloudrs opens (design preview): a greeting, then shelves of
//! what to play next. Recently played comes from the history, your
//! playlists from the library and the newest posts from the feed once signed
//! in. A shelf without anything to show is left out.

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{CardData, card, skeleton_card};
use cloudrs_ui::components::{ButtonKind, Icon, button, icon};
use cloudrs_ui::tokens::{size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, SharedString, div};
use sc_core::{ListItems, PlaylistSummary, TrackSummary};

use crate::i18n::{self, home as t};
use crate::intent::UiIntent;
use crate::models::ListId;
use crate::nav::Route;
use crate::shell::Shell;

/// Cards on a shelf; the rest is one "See all" away.
const SHELF_CARDS: usize = 12;
/// Feed rows on Home.
const FEED_ROWS: usize = 5;
/// Skeleton cards while a shelf loads.
const SKELETON_CARDS: usize = 6;

impl Shell {
    /// Asks for what Home shows: the history every time (it changes as you
    /// listen), the account's lists the first time.
    pub(crate) fn refresh_home(&mut self, cx: &mut Context<Self>) {
        self.send(sc_core::Command::OpenHistory);
        if self.models.account.is_some() {
            for list in [ListId::Library, ListId::Feed] {
                if !self.models.lists.contains_key(&list) {
                    self.dispatch(UiIntent::OpenList(list), cx);
                }
            }
        }
    }

    pub(crate) fn home_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        let greeting = match &self.models.account {
            Some(me) => t::welcome_back(&me.username),
            None => t::welcome().to_owned(),
        };
        let mut page = div()
            .w_full()
            .max_w(size::HOME_MAX_WIDTH)
            .flex()
            .flex_col()
            .gap(space::S6)
            .px(space::S5)
            .pb(space::S8)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .pt(space::S2)
                    .child(
                        theme
                            .text(div(), typography::DISPLAY_L)
                            .text_color(c.text)
                            .child(greeting),
                    )
                    .child(
                        theme
                            .text(div(), typography::BODY_MUTED)
                            .text_color(c.text_muted)
                            .child(t::subtitle()),
                    ),
            );

        let recent = self.recent_tracks();
        let history_loading = self
            .models
            .lists
            .get(&ListId::History)
            .is_some_and(|list| list.loading);
        if history_loading && recent.is_empty() {
            page = page.child(shelf_skeleton(
                theme,
                t::recently_played(),
                "recent-skeleton",
            ));
        } else if !recent.is_empty() {
            let cards = recent
                .iter()
                .enumerate()
                .map(|(ix, track)| self.track_card(ix, track, theme, cx))
                .collect();
            page = page.child(self.shelf(
                theme,
                t::recently_played(),
                Some(UiIntent::OpenHistory),
                cards,
                cx,
            ));
        }

        if self.models.account.is_some() {
            let playlists = self.playlists(ListId::Library);
            if playlists.is_empty() && self.list_loading(ListId::Library) {
                page = page.child(shelf_skeleton(
                    theme,
                    t::your_playlists(),
                    "library-skeleton",
                ));
            } else if !playlists.is_empty() {
                let cards = playlists
                    .iter()
                    .enumerate()
                    .map(|(ix, playlist)| self.playlist_card(ix, playlist, theme, cx))
                    .collect();
                page = page.child(self.shelf(
                    theme,
                    t::your_playlists(),
                    Some(UiIntent::OpenList(ListId::Library)),
                    cards,
                    cx,
                ));
            }
            let feed = self.tracks(ListId::Feed);
            if !feed.is_empty() {
                let rows: Vec<AnyElement> = feed
                    .iter()
                    .take(FEED_ROWS)
                    .enumerate()
                    .map(|(ix, track)| self.track_item(ListId::Feed, ix, track, theme, cx))
                    .collect();
                page = page.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .child(self.shelf_header(
                            theme,
                            t::from_your_feed(),
                            Some(UiIntent::OpenList(ListId::Feed)),
                            cx,
                        ))
                        .children(rows),
                );
            }
        } else if recent.is_empty() && !history_loading {
            page = page.child(self.home_welcome(theme, cx));
        }

        div()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .child(page)
            .into_any_element()
    }

    /// The tracks played, newest first, each once.
    fn recent_tracks(&self) -> Vec<TrackSummary> {
        let mut tracks = self.tracks(ListId::History);
        tracks.truncate(SHELF_CARDS);
        tracks
    }

    fn tracks(&self, list: ListId) -> Vec<TrackSummary> {
        match self.models.lists.get(&list).map(|l| &l.items) {
            Some(ListItems::Tracks(tracks)) => tracks.clone(),
            _ => Vec::new(),
        }
    }

    fn playlists(&self, list: ListId) -> Vec<PlaylistSummary> {
        match self.models.lists.get(&list).map(|l| &l.items) {
            Some(ListItems::Playlists(playlists)) => {
                playlists.iter().take(SHELF_CARDS).cloned().collect()
            }
            _ => Vec::new(),
        }
    }

    fn list_loading(&self, list: ListId) -> bool {
        self.models.lists.get(&list).is_some_and(|l| l.loading)
    }

    /// A shelf: a title (with "See all" when there is more) over a row of
    /// cards that scrolls sideways.
    fn shelf(
        &self,
        theme: &Theme,
        title: &'static str,
        see_all: Option<UiIntent>,
        cards: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(space::S2)
            .child(self.shelf_header(theme, title, see_all, cx))
            .child(
                div()
                    .id(title)
                    .flex()
                    .gap(space::S2)
                    .overflow_x_scroll()
                    .children(cards),
            )
    }

    fn shelf_header(
        &self,
        theme: &Theme,
        title: &'static str,
        see_all: Option<UiIntent>,
        cx: &mut Context<Self>,
    ) -> Div {
        let c = theme.colors;
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                theme
                    .text(div(), typography::TITLE)
                    .text_color(c.text)
                    .child(title),
            )
            .when_some(see_all, |header, intent| {
                header.child(
                    button(
                        theme,
                        SharedString::from(format!("see-all-{title}")),
                        t::see_all(),
                        ButtonKind::Ghost,
                    )
                    .aria_label(t::see_all_of(title))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_from_home(intent.clone(), cx);
                    })),
                )
            })
    }

    /// "See all": the History screen, or an account list's screen.
    fn open_from_home(&mut self, intent: UiIntent, cx: &mut Context<Self>) {
        let route = match &intent {
            UiIntent::OpenList(ListId::Library) => Some(Route::Library),
            UiIntent::OpenList(ListId::Feed) => Some(Route::Feed),
            _ => None,
        };
        match route {
            Some(route) => self.navigate(route, cx),
            None => self.dispatch(intent, cx),
        }
    }

    fn track_card(
        &self,
        ix: usize,
        track: &TrackSummary,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = track.id;
        let play = cx.listener(move |this, _, _, cx| {
            this.dispatch(
                UiIntent::Play {
                    list: ListId::History,
                    track: id,
                },
                cx,
            );
        });
        card(
            theme,
            ("recent", ix),
            CardData {
                title: &track.title,
                meta: &track.artist,
                cover: self.models.art.tracks.get(&id).cloned(),
                round: false,
            },
            Some(play),
        )
        .aria_label(i18n::search::play_track(&track.title, &track.artist))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch(UiIntent::OpenTrack(id), cx);
        }))
        .into_any_element()
    }

    fn playlist_card(
        &self,
        ix: usize,
        playlist: &PlaylistSummary,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = playlist.id;
        let meta = i18n::count::tracks(playlist.track_count);
        card(
            theme,
            ("playlist-card", ix),
            CardData {
                title: &playlist.title,
                meta: &meta,
                cover: self.models.art.playlists.get(&id).cloned(),
                round: false,
            },
            None::<fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>,
        )
        .aria_label(i18n::playlist::open_playlist(&playlist.title))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch(UiIntent::OpenPlaylist(id), cx);
        }))
        .into_any_element()
    }

    /// Nothing played and not signed in: where to start.
    fn home_welcome(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = theme.colors;
        div()
            .flex()
            .flex_col()
            .gap(space::S3)
            .p(space::S6)
            .rounded(cloudrs_ui::tokens::radius::XL)
            .bg(c.surface)
            .border_1()
            .border_color(c.line)
            .child(icon(Icon::Search, size::ICON_STATUS, c.accent))
            .child(
                theme
                    .text(div(), typography::TITLE)
                    .text_color(c.text)
                    .child(t::start_title()),
            )
            .child(
                theme
                    .text(div(), typography::BODY_MUTED)
                    .text_color(c.text_muted)
                    .child(t::start_hint()),
            )
            .child(
                div()
                    .flex()
                    .gap(space::S2)
                    .pt(space::S2)
                    .child(
                        button(theme, "home-search", t::start_search(), ButtonKind::Primary)
                            .aria_label(t::start_search())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.focus_search_field(window, cx);
                            })),
                    )
                    .child(
                        button(
                            theme,
                            "home-sign-in",
                            i18n::nav::sign_in(),
                            ButtonKind::Secondary,
                        )
                        .aria_label(i18n::nav::sign_in())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.navigate(Route::Account, cx);
                        })),
                    ),
            )
    }
}

fn shelf_skeleton(theme: &Theme, title: &'static str, id: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(space::S2)
        .child(
            theme
                .text(div(), typography::TITLE)
                .text_color(theme.colors.text)
                .child(title),
        )
        .child(
            div()
                .flex()
                .gap(space::S2)
                .overflow_hidden()
                .children((0..SKELETON_CARDS).map(|i| skeleton_card(theme, (id, i)))),
        )
}
