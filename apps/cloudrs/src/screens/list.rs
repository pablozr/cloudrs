//! The one list component of every screen: a virtual list with skeleton,
//! empty and error states, paging near the end and rows that open pages and
//! play from the list.

use std::ops::Range;

use cloudrs_ui::Theme;
use cloudrs_ui::browse::{CollectionRowData, UserRowData, collection_row, user_row};
use cloudrs_ui::components::{
    ButtonKind, Icon, RowLink, TrackRowData, button, row_action, row_toggle, skeleton_row,
    track_row,
};
use cloudrs_ui::tokens::space;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Role, SharedString, div, uniform_list};

use crate::i18n;
use crate::intent::UiIntent;
use sc_core::{ListItems, PlaylistSummary, TrackId, TrackSummary, UserSummary};

use crate::models::ListId;
use crate::shell::{Shell, status_view};
use crate::state::{compact_count, format_time};

/// Skeleton rows while a first page loads, and at the end while the next does.
const FIRST_PAGE_SKELETONS: usize = 10;
const NEXT_PAGE_SKELETONS: usize = 3;

/// The title and hint of an empty list.
/// The muted icon above an empty list.
fn empty_icon(key: ListId) -> Icon {
    match key {
        ListId::History => Icon::History,
        ListId::Search { .. } => Icon::Search,
        _ => Icon::Queue,
    }
}

fn empty_text(key: ListId, query: &str) -> (String, &'static str) {
    use i18n::list as t;
    let hint = t::empty_hint();
    match key {
        ListId::Search { .. } => (
            i18n::search::no_results_title(query),
            i18n::search::no_results_hint(),
        ),
        ListId::UserTracks(_) => (t::user_tracks_empty().to_owned(), hint),
        ListId::UserPlaylists(_) => (t::user_playlists_empty().to_owned(), hint),
        ListId::UserLikes(_) => (t::user_likes_empty().to_owned(), hint),
        ListId::Playlist(_) => (t::playlist_empty().to_owned(), hint),
        ListId::Related(_) => (t::related_empty().to_owned(), hint),
        ListId::History => (t::history_empty().to_owned(), t::history_empty_hint()),
        ListId::Followings(_) => (t::followings_empty().to_owned(), hint),
        ListId::Feed => (t::feed_empty().to_owned(), t::feed_empty_hint()),
        ListId::Library => (t::library_empty().to_owned(), hint),
    }
}

impl Shell {
    /// The list `key` with whatever state it is in. `label` names it for
    /// screen readers.
    pub(crate) fn list_view(
        &mut self,
        key: ListId,
        label: &'static str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (loading, error, len, loading_more) = self
            .models
            .lists
            .get(&key)
            .map_or((false, false, 0, false), |list| {
                (list.loading, list.error, list.len(), list.loading_more)
            });
        let query = &self.models.query;

        if matches!(key, ListId::Search { .. }) && query.is_empty() {
            return status_view(
                theme,
                Icon::Search,
                "search-prompt",
                i18n::search::empty_title(),
                i18n::search::empty_hint(),
                None,
            );
        }
        if loading {
            return div()
                .size_full()
                .overflow_hidden()
                .flex()
                .flex_col()
                .pt(space::S2)
                .children((0..FIRST_PAGE_SKELETONS).map(|i| skeleton_row(theme, ("skeleton", i))))
                .into_any_element();
        }
        if error {
            let (title, hint) = match key {
                ListId::Search { .. } => (i18n::search::error_title(), i18n::search::error_hint()),
                _ => (i18n::list::error_title(), i18n::list::error_hint()),
            };
            let intent = UiIntent::OpenList(key);
            let retry = button(
                theme,
                "retry-list",
                i18n::app::try_again(),
                ButtonKind::Primary,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.dispatch(intent.clone(), cx)));
            return status_view(theme, Icon::Alert, "list-failed", title, hint, Some(retry));
        }
        if len == 0 {
            let (title, hint) = empty_text(key, query);
            return status_view(theme, empty_icon(key), "list-empty", &title, hint, None);
        }

        let more = if loading_more { NEXT_PAGE_SKELETONS } else { 0 };
        let scroll = self.scrolls.entry(key).or_default().clone();
        div()
            .id("list")
            .role(Role::List)
            .aria_label(label)
            .size_full()
            .child(
                uniform_list(
                    SharedString::from(format!("list-{key:?}")),
                    len + more,
                    cx.processor(move |this, range, _, cx| this.list_rows(key, range, cx)),
                )
                .track_scroll(&scroll)
                .size_full(),
            )
            .into_any_element()
    }

    /// The visible rows (and skeletons for the next page). Asks for the next
    /// page once, when the rows end near the end of the list.
    fn list_rows(
        &mut self,
        key: ListId,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self
            .models
            .lists
            .get_mut(&key)
            .is_some_and(|list| list.take_load_more(range.end))
        {
            self.dispatch(UiIntent::LoadMore(key), cx);
        }
        let Some(list) = self.models.lists.get(&key) else {
            return Vec::new();
        };
        let theme = Theme::of(cx);
        let len = list.len();
        range
            .map(|ix| {
                if ix >= len {
                    return skeleton_row(&theme, ("skeleton-more", ix)).into_any_element();
                }
                match &list.items {
                    ListItems::Tracks(items) => self.track_item(key, ix, &items[ix], &theme, cx),
                    ListItems::Users(items) => self.user_item(ix, &items[ix], &theme, cx),
                    ListItems::Playlists(items) => self.playlist_item(ix, &items[ix], &theme, cx),
                }
            })
            .collect()
    }

    fn track_item(
        &self,
        key: ListId,
        ix: usize,
        track: &TrackSummary,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = track.id;
        let active = self.models.current == Some(id);
        let duration = format_time(track.duration);
        let artist_link = track.artist_id.map(|user| -> RowLink {
            Box::new(cx.listener(move |this, _, _, cx| this.dispatch(UiIntent::OpenUser(user), cx)))
        });
        let row = TrackRowData {
            index: ix + 1,
            title: &track.title,
            artist: &track.artist,
            duration: &duration,
            artwork: self.models.art.tracks.get(&id).cloned(),
            active,
            playing: active && self.models.playing,
            preview_badge: track.preview_only.then(i18n::search::preview_badge),
            actions: self
                .like_toggle(id, ix, theme, cx)
                .into_iter()
                .chain([
                    row_action(
                        theme,
                        ("play-next", ix),
                        Icon::PlayNext,
                        i18n::queue::play_next(),
                        cx.listener(move |this, _, _, cx| {
                            this.dispatch(UiIntent::PlayNext(id), cx)
                        }),
                    )
                    .into_any_element(),
                    row_action(
                        theme,
                        ("add-to-queue", ix),
                        Icon::AddToQueue,
                        i18n::queue::add_to_queue(),
                        cx.listener(move |this, _, _, cx| {
                            this.dispatch(UiIntent::AddToQueue(id), cx)
                        }),
                    )
                    .into_any_element(),
                ])
                .collect(),
            title_link: Some(Box::new(cx.listener(move |this, _, _, cx| {
                this.dispatch(UiIntent::OpenTrack(id), cx)
            }))),
            artist_link,
        };
        track_row(theme, ("track", ix), row)
            .aria_label(i18n::search::play_track(&track.title, &track.artist))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dispatch(
                    UiIntent::Play {
                        list: key,
                        track: id,
                    },
                    cx,
                );
            }))
            .into_any_element()
    }

    /// The heart of a track row, once signed in.
    fn like_toggle(
        &self,
        id: TrackId,
        ix: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.models.account.as_ref()?;
        let liked = self.models.liked.contains(&id);
        let (glyph, label) = if liked {
            (Icon::HeartFilled, i18n::social::unlike())
        } else {
            (Icon::Heart, i18n::social::like())
        };
        Some(
            row_toggle(
                theme,
                ("like", ix),
                glyph,
                label,
                liked,
                cx.listener(move |this, _, _, cx| {
                    this.dispatch(
                        UiIntent::Like {
                            track: id,
                            liked: !liked,
                        },
                        cx,
                    );
                }),
            )
            .into_any_element(),
        )
    }

    fn user_item(
        &self,
        ix: usize,
        user: &UserSummary,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = user.id;
        let meta = user
            .followers
            .map(|followers| i18n::user::followers(compact_count(followers)))
            .unwrap_or_default();
        let row = UserRowData {
            name: &user.username,
            meta: &meta,
            avatar: self.models.art.users.get(&id).cloned(),
        };
        user_row(theme, ("user", ix), row)
            .aria_label(i18n::user::open_profile(&user.username))
            .on_click(cx.listener(move |this, _, _, cx| this.dispatch(UiIntent::OpenUser(id), cx)))
            .into_any_element()
    }

    fn playlist_item(
        &self,
        ix: usize,
        playlist: &PlaylistSummary,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = playlist.id;
        let meta = i18n::dot_join(&[
            playlist.owner.clone(),
            i18n::count::tracks(playlist.track_count),
        ]);
        let row = CollectionRowData {
            title: &playlist.title,
            meta: &meta,
            cover: self.models.art.playlists.get(&id).cloned(),
            album_badge: playlist.is_album.then(i18n::playlist::album_badge),
        };
        collection_row(theme, ("playlist", ix), row)
            .aria_label(i18n::playlist::open_playlist(&playlist.title))
            .on_click(
                cx.listener(move |this, _, _, cx| this.dispatch(UiIntent::OpenPlaylist(id), cx)),
            )
            .into_any_element()
    }
}
