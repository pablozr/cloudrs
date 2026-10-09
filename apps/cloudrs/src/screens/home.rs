//! Home, where cloudrs opens (design preview, ADR 0013): a greeting, a
//! highlight, quick tiles, trending tracks by genre, and shelves of cards —
//! recently played, SoundCloud's own rows and, once signed in, the feed,
//! likes, playlists and the people followed. A shelf with nothing to show is
//! left out; one on its way shows skeleton cards.

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{CardData, HeroData, card, hero, quick_tile, skeleton_card};
use cloudrs_ui::components::{ButtonKind, Icon, button, icon, pill};
use cloudrs_ui::tokens::{radius, size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Div, SharedString, Window, div};
use sc_core::{ArtKey, Genre, ListItems, PlaylistSummary, TrackId, TrackSummary, UserSummary};

use crate::i18n::{self, home as t};
use crate::intent::UiIntent;
use crate::models::ListId;
use crate::nav::Route;
use crate::shell::Shell;

/// Cards on a shelf; the rest is one "See all" away.
const SHELF_CARDS: usize = 12;
/// Quick tiles: two rows of three.
const TILES: usize = 6;
const TILES_PER_ROW: usize = 3;
/// Skeleton cards while a shelf loads.
const SKELETON_CARDS: usize = 6;

/// What a shelf holds.
enum Shelf {
    Tracks(ListId, Vec<TrackSummary>),
    Playlists(Vec<PlaylistSummary>),
    People(Vec<UserSummary>),
}

impl Shell {
    /// Asks for what Home shows: the history every time (it changes as you
    /// listen), the rest the first time.
    pub(crate) fn refresh_home(&mut self, cx: &mut Context<Self>) {
        self.send(sc_core::Command::OpenHistory);
        if self.models.home_shelves.is_empty() {
            self.send(sc_core::Command::OpenHome);
        }
        let mut lists = vec![ListId::Trending(self.models.home_genre)];
        if let Some(me) = &self.models.account {
            lists.extend([
                ListId::Library,
                ListId::Feed,
                ListId::UserLikes(me.id),
                ListId::Followings(me.id),
            ]);
        }
        for list in lists {
            if !self.models.lists.contains_key(&list) {
                self.dispatch(UiIntent::OpenList(list), cx);
            }
        }
    }

    pub(crate) fn home_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        let name = self.models.account.as_ref().map(|me| me.username.as_str());
        let greeting = greeting(local_hour(), name);
        let mut page = div()
            .w_full()
            .max_w(size::HOME_MAX_WIDTH)
            .flex()
            .flex_col()
            .gap(space::S8)
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
                            .child(greeting),
                    )
                    .child(
                        theme
                            .text(div(), typography::BODY_MUTED)
                            .text_color(c.text_muted)
                            .child(t::subtitle()),
                    ),
            );
        if let Some(highlight) = self.home_hero(theme, cx) {
            page = page.child(highlight);
        }
        if let Some(tiles) = self.home_tiles(theme, cx) {
            page = page.child(tiles);
        }
        if self.models.account.is_some() {
            // Your playlists come first: the thing most often opened.
            page = page.child(self.list_shelf(
                theme,
                "library",
                t::your_playlists(),
                ListId::Library,
                Route::Library,
                cx,
            ));
        }
        page = page.child(self.home_trending(theme, cx));

        let recent = self.tracks(ListId::History);
        let nothing_played = recent.is_empty();
        if !nothing_played {
            let header =
                self.shelf_header(theme, t::recently_played().into(), Some(Route::History), cx);
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(header)
                    .child(self.cards(theme, "recent", Shelf::Tracks(ListId::History, recent), cx)),
            );
        }
        let me = self.models.account.as_ref().map(|me| me.id);
        if let Some(me) = me {
            page = page
                .child(self.list_shelf(
                    theme,
                    "feed",
                    t::from_people_you_follow(),
                    ListId::Feed,
                    Route::Feed,
                    cx,
                ))
                .child(self.list_shelf(
                    theme,
                    "likes",
                    t::liked_tracks(),
                    ListId::UserLikes(me),
                    Route::Likes(me),
                    cx,
                ));
        }
        let soundcloud: Vec<(String, Vec<PlaylistSummary>)> = self
            .models
            .home_shelves
            .iter()
            .map(|s| (s.title.clone(), s.playlists.clone()))
            .collect();
        for (ix, (title, playlists)) in soundcloud.into_iter().enumerate() {
            let header = self.shelf_header(theme, title.into(), None, cx);
            let key = SharedString::from(format!("soundcloud-{ix}"));
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(header)
                    .child(self.cards(theme, key, Shelf::Playlists(playlists), cx)),
            );
        }
        match me {
            Some(me) => {
                page = page.child(self.list_shelf(
                    theme,
                    "following",
                    t::artists_you_follow(),
                    ListId::Followings(me),
                    Route::Following(me),
                    cx,
                ));
            }
            None if nothing_played => page = page.child(self.home_welcome(theme, cx)),
            None => {}
        }

        div()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .child(page)
            .into_any_element()
    }

    /// What plays now, else the last played, else the top trending track.
    fn home_hero(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let genre = self.models.home_genre;
        let playing_now = self.models.current.and_then(|id| self.find_track(id));
        let (eyebrow, track, list) = if let Some(track) = playing_now {
            (t::now_playing(), track, None)
        } else if let Some(track) = self.tracks(ListId::History).into_iter().next() {
            (t::jump_back_in(), track, Some(ListId::History))
        } else {
            let track = self.tracks(ListId::Trending(genre)).into_iter().next()?;
            (t::trending_now(), track, Some(ListId::Trending(genre)))
        };
        let id = track.id;
        let playing = list.is_none() && self.models.playing;
        let play_label = if playing { t::pause() } else { t::play() };
        let play = button(theme, "hero-play", play_label, ButtonKind::Primary)
            .aria_label(play_label)
            .on_click(cx.listener(move |this, _, _, cx| match list {
                None => this.send(sc_core::Command::TogglePlay),
                Some(list) => this.dispatch(UiIntent::Play { list, track: id }, cx),
            }))
            .into_any_element();
        let open = button(theme, "hero-open", t::open_track(), ButtonKind::Secondary)
            .aria_label(t::open_track())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dispatch(UiIntent::OpenTrack(id), cx);
            }))
            .into_any_element();
        let meta = i18n::dot_join(&[
            track.artist.clone(),
            crate::state::format_time(track.duration),
        ]);
        Some(
            hero(
                theme,
                "hero",
                HeroData {
                    eyebrow,
                    title: &track.title,
                    meta: &meta,
                    cover: self.models.art.tracks.get(&id).cloned(),
                    tint: self.models.art.tint(ArtKey::Track(id)).map(Into::into),
                    actions: vec![play, open],
                },
            )
            .aria_label(i18n::search::play_track(&track.title, &track.artist))
            .into_any_element(),
        )
    }

    /// Shortcuts to play right away: what you played last, then your likes.
    /// Your playlists have their own shelf (and the sidebar), so they only
    /// fill in when there is nothing else.
    fn home_tiles(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        enum Tile {
            Playlist(PlaylistSummary),
            Track(ListId, TrackSummary),
        }
        let mut sources = vec![ListId::History];
        if let Some(me) = &self.models.account {
            sources.push(ListId::UserLikes(me.id));
        }
        let mut seen = std::collections::HashSet::new();
        let mut tiles: Vec<Tile> = sources
            .into_iter()
            .flat_map(|list| self.tracks(list).into_iter().map(move |t| (list, t)))
            .filter(|(_, t)| seen.insert(t.id))
            .map(|(list, t)| Tile::Track(list, t))
            .collect();
        if tiles.len() < TILES_PER_ROW {
            tiles.extend(
                self.playlists(ListId::Library)
                    .into_iter()
                    .map(Tile::Playlist),
            );
        }
        // Whole rows only: a lone tile stretched across a row looks broken.
        let whole = tiles.len().min(TILES) / TILES_PER_ROW * TILES_PER_ROW;
        tiles.truncate(whole);
        if tiles.is_empty() {
            return None;
        }
        let tiles: Vec<AnyElement> = tiles
            .into_iter()
            .enumerate()
            .map(|(ix, tile)| match tile {
                Tile::Playlist(playlist) => {
                    let id = playlist.id;
                    quick_tile(
                        theme,
                        ("tile", ix),
                        &playlist.title,
                        self.models.art.playlists.get(&id).cloned(),
                        false,
                        open_playlist(id, cx),
                    )
                    .aria_label(i18n::playlist::open_playlist(&playlist.title))
                    .on_click(open_playlist(id, cx))
                    .into_any_element()
                }
                Tile::Track(list, track) => {
                    let id = track.id;
                    quick_tile(
                        theme,
                        ("tile", ix),
                        &track.title,
                        self.models.art.tracks.get(&id).cloned(),
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.dispatch(UiIntent::Play { list, track: id }, cx);
                        }),
                    )
                    .aria_label(i18n::search::play_track(&track.title, &track.artist))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.dispatch(UiIntent::OpenTrack(id), cx);
                    }))
                    .into_any_element()
                }
            })
            .collect();
        let mut grid = div().flex().flex_col().gap(space::S2);
        let mut tiles = tiles.into_iter().peekable();
        while tiles.peek().is_some() {
            let row: Vec<AnyElement> = tiles.by_ref().take(TILES_PER_ROW).collect();
            grid = grid.child(div().flex().gap(space::S2).children(row));
        }
        Some(grid)
    }

    /// The genre pills over the trending tracks of the picked genre.
    fn home_trending(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let picked = self.models.home_genre;
        let pills: Vec<AnyElement> = Genre::ALL
            .into_iter()
            .enumerate()
            .map(|(ix, genre)| {
                let label = genre_label(genre);
                pill(theme, ("genre", ix), label, genre == picked)
                    .tab_index(0)
                    .aria_label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.models.home_genre = genre;
                        let list = ListId::Trending(genre);
                        if this.models.lists.contains_key(&list) {
                            cx.notify();
                        } else {
                            this.dispatch(UiIntent::OpenList(list), cx);
                        }
                    }))
                    .into_any_element()
            })
            .collect();
        let list = ListId::Trending(picked);
        div()
            .flex()
            .flex_col()
            .gap(space::S3)
            .child(
                theme
                    .text(div(), typography::TITLE)
                    .text_color(theme.colors.text)
                    .child(t::trending()),
            )
            .child(
                div()
                    .id("genres")
                    .flex()
                    .gap(space::S2)
                    .overflow_x_scroll()
                    .children(pills),
            )
            .child(self.shelf_body(theme, "trending", list, cx))
    }

    /// A shelf fed by a core list, with its loading state and "See all".
    fn list_shelf(
        &mut self,
        theme: &Theme,
        key: &'static str,
        title: &'static str,
        list: ListId,
        see_all: Route,
        cx: &mut Context<Self>,
    ) -> Div {
        let view = self.models.lists.get(&list);
        let loading = view.is_some_and(|l| l.loading);
        let empty = view.is_none_or(|l| l.len() == 0);
        if empty && !loading {
            return div();
        }
        let header = self.shelf_header(theme, title.into(), Some(see_all), cx);
        div()
            .flex()
            .flex_col()
            .gap(space::S2)
            .child(header)
            .child(self.shelf_body(theme, key, list, cx))
    }

    /// The cards of a core list (skeletons while its first page loads).
    fn shelf_body(
        &mut self,
        theme: &Theme,
        key: &'static str,
        list: ListId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = self.models.lists.get(&list);
        if view.is_none_or(|l| l.loading) {
            return skeleton_row(theme, key).into_any_element();
        }
        let shelf = match view.map(|l| &l.items) {
            Some(ListItems::Tracks(tracks)) => {
                Shelf::Tracks(list, tracks.iter().take(SHELF_CARDS).cloned().collect())
            }
            Some(ListItems::Playlists(playlists)) => {
                Shelf::Playlists(playlists.iter().take(SHELF_CARDS).cloned().collect())
            }
            Some(ListItems::Users(users)) => {
                Shelf::People(users.iter().take(SHELF_CARDS).cloned().collect())
            }
            None => return div().into_any_element(),
        };
        self.cards(theme, key, shelf, cx).into_any_element()
    }

    /// A row of cards that scrolls sideways.
    fn cards(
        &mut self,
        theme: &Theme,
        key: impl Into<SharedString>,
        shelf: Shelf,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let key: SharedString = key.into();
        let cards: Vec<AnyElement> = match shelf {
            Shelf::Tracks(list, tracks) => tracks
                .iter()
                .enumerate()
                .map(|(ix, track)| self.track_card(&key, ix, list, track, theme, cx))
                .collect(),
            Shelf::Playlists(playlists) => playlists
                .iter()
                .enumerate()
                .map(|(ix, playlist)| self.playlist_card(&key, ix, playlist, theme, cx))
                .collect(),
            Shelf::People(people) => people
                .iter()
                .enumerate()
                .map(|(ix, user)| self.person_card(&key, ix, user, theme, cx))
                .collect(),
        };
        div()
            .id(SharedString::from(format!("{key}-cards")))
            .flex()
            .gap(space::S1)
            .overflow_x_scroll()
            .children(cards)
    }

    fn shelf_header(
        &self,
        theme: &Theme,
        title: SharedString,
        see_all: Option<Route>,
        cx: &mut Context<Self>,
    ) -> Div {
        let c = theme.colors;
        let label = t::see_all_of(&title);
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                theme
                    .text(div(), typography::TITLE)
                    .text_color(c.text)
                    .child(title.clone()),
            )
            .when_some(see_all, |header, route| {
                header.child(
                    button(
                        theme,
                        SharedString::from(format!("see-all-{title}")),
                        t::see_all(),
                        ButtonKind::Ghost,
                    )
                    .aria_label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_see_all(route.clone(), cx);
                    })),
                )
            })
    }

    /// "See all" opens the screen of that list (and loads it the first time).
    fn open_see_all(&mut self, route: Route, cx: &mut Context<Self>) {
        if route == Route::History {
            return self.dispatch(UiIntent::OpenHistory, cx);
        }
        let list = route.account_list();
        self.navigate(route, cx);
        if let Some(list) = list
            && !self.models.lists.contains_key(&list)
        {
            self.dispatch(UiIntent::OpenList(list), cx);
        }
    }

    fn track_card(
        &self,
        key: &SharedString,
        ix: usize,
        list: ListId,
        track: &TrackSummary,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = track.id;
        let play = cx.listener(move |this, _: &ClickEvent, _: &mut Window, cx| {
            this.dispatch(UiIntent::Play { list, track: id }, cx);
        });
        card(
            theme,
            (key.clone(), ix),
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

    pub(crate) fn playlist_card(
        &self,
        key: &SharedString,
        ix: usize,
        playlist: &PlaylistSummary,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = playlist.id;
        let count = i18n::count::tracks(playlist.track_count);
        let meta = if playlist.owner.is_empty() {
            count
        } else {
            i18n::dot_join(&[playlist.owner.clone(), count])
        };
        card(
            theme,
            (key.clone(), ix),
            CardData {
                title: &playlist.title,
                meta: &meta,
                cover: self.models.art.playlists.get(&id).cloned(),
                round: false,
            },
            Some(open_playlist(id, cx)),
        )
        .aria_label(i18n::playlist::open_playlist(&playlist.title))
        .on_click(open_playlist(id, cx))
        .into_any_element()
    }

    fn person_card(
        &self,
        key: &SharedString,
        ix: usize,
        user: &UserSummary,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = user.id;
        let meta = user
            .followers
            .map(|n| i18n::user::followers(i18n::number::compact(n)))
            .unwrap_or_default();
        card(
            theme,
            (key.clone(), ix),
            CardData {
                title: &user.username,
                meta: &meta,
                cover: self.models.art.users.get(&id).cloned(),
                round: true,
            },
            None::<fn(&ClickEvent, &mut Window, &mut gpui::App)>,
        )
        .aria_label(i18n::user::open_profile(&user.username))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch(UiIntent::OpenUser(id), cx);
        }))
        .into_any_element()
    }

    fn tracks(&self, list: ListId) -> Vec<TrackSummary> {
        match self.models.lists.get(&list).map(|l| &l.items) {
            Some(ListItems::Tracks(tracks)) => tracks.iter().take(SHELF_CARDS).cloned().collect(),
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

    /// A track the screens have seen, by id.
    fn find_track(&self, id: TrackId) -> Option<TrackSummary> {
        self.models
            .lists
            .values()
            .find_map(|list| match &list.items {
                ListItems::Tracks(tracks) => tracks.iter().find(|t| t.id == id).cloned(),
                _ => None,
            })
    }

    /// Nothing played and not signed in: where to start.
    fn home_welcome(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = theme.colors;
        div()
            .flex()
            .flex_col()
            .gap(space::S3)
            .p(space::S6)
            .rounded(radius::XL)
            .bg(c.surface)
            .border_1()
            .border_color(c.line)
            .child(icon(Icon::Account, size::ICON_STATUS, c.accent))
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
                div().flex().gap(space::S2).pt(space::S2).child(
                    button(
                        theme,
                        "home-sign-in",
                        i18n::nav::sign_in(),
                        ButtonKind::Primary,
                    )
                    .aria_label(i18n::nav::sign_in())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate(Route::Account, cx);
                    })),
                ),
            )
    }
}

fn open_playlist(
    id: sc_core::PlaylistId,
    cx: &mut Context<Shell>,
) -> impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static {
    cx.listener(move |this, _, _, cx| {
        this.dispatch(UiIntent::OpenPlaylist(id), cx);
    })
}

fn skeleton_row(theme: &Theme, key: &'static str) -> Div {
    div()
        .flex()
        .gap(space::S1)
        .overflow_hidden()
        .children((0..SKELETON_CARDS).map(|i| skeleton_card(theme, (key, i))))
}

fn genre_label(genre: Genre) -> &'static str {
    use crate::i18n::genre as g;
    match genre {
        Genre::All => g::all(),
        Genre::Electronic => g::electronic(),
        Genre::House => g::house(),
        Genre::HipHop => g::hip_hop(),
        Genre::Dubstep => g::dubstep(),
        Genre::Ambient => g::ambient(),
        Genre::Pop => g::pop(),
        Genre::Rock => g::rock(),
        Genre::Indie => g::indie(),
        Genre::Latin => g::latin(),
        Genre::RnB => g::r_n_b(),
        Genre::Trap => g::trap(),
    }
}

/// Part of the day, from the local hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DayPart {
    Morning,
    Afternoon,
    Evening,
}

fn day_part(hour: u8) -> DayPart {
    match hour {
        5..=11 => DayPart::Morning,
        12..=17 => DayPart::Afternoon,
        _ => DayPart::Evening,
    }
}

/// The local hour; `None` where the system will not say (some Unix setups
/// refuse it in a multi-threaded process).
fn local_hour() -> Option<u8> {
    time::OffsetDateTime::now_local().ok().map(|now| now.hour())
}

/// "Good evening, Ana", or a plain welcome without the hour.
fn greeting(hour: Option<u8>, name: Option<&str>) -> String {
    match (hour.map(day_part), name) {
        (Some(DayPart::Morning), Some(name)) => t::good_morning_name(name),
        (Some(DayPart::Afternoon), Some(name)) => t::good_afternoon_name(name),
        (Some(DayPart::Evening), Some(name)) => t::good_evening_name(name),
        (Some(DayPart::Morning), None) => t::good_morning().to_owned(),
        (Some(DayPart::Afternoon), None) => t::good_afternoon().to_owned(),
        (Some(DayPart::Evening), None) => t::good_evening().to_owned(),
        (None, Some(name)) => t::welcome_back(name),
        (None, None) => t::welcome().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_greeting_follows_the_hour() {
        assert_eq!(day_part(5), DayPart::Morning);
        assert_eq!(day_part(11), DayPart::Morning);
        assert_eq!(day_part(12), DayPart::Afternoon);
        assert_eq!(day_part(18), DayPart::Evening);
        assert_eq!(day_part(2), DayPart::Evening);
        assert_eq!(greeting(Some(20), Some("Ana")), "Good evening, Ana");
        assert_eq!(greeting(Some(9), None), "Good morning");
        assert_eq!(greeting(None, Some("Ana")), "Welcome back, Ana");
    }
}
