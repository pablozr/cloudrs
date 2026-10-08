//! View state of the browsing screens, as plain data (ADR 0008): what the
//! core sent (`ListItems`, the page types) plus the UI's own loading flags.
//! Only `seam.rs` fills it from core events.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

pub use sc_core::{ArtKey, ListId, ListItems, PlaylistId, SearchKind, TrackId, UserId};
use sc_core::{Genre, HomeShelf, JamState, PlaylistPage, TrackPage, UserPage, UserSummary};

use crate::state::ArtworkMap;
use crate::tint::Rgb;

/// How close to the end of the list (in rows) the next page is requested.
const LOAD_MORE_MARGIN: usize = 5;

/// The search tabs, in order.
pub const SEARCH_TABS: [SearchKind; 4] = [
    SearchKind::Tracks,
    SearchKind::People,
    SearchKind::Playlists,
    SearchKind::Albums,
];

pub fn tab_index(kind: SearchKind) -> usize {
    SEARCH_TABS.iter().position(|k| *k == kind).unwrap_or(0)
}

/// The rows of a list that has not received a page yet.
pub fn empty_items(list: ListId) -> ListItems {
    match list {
        ListId::Search {
            kind: SearchKind::People,
        }
        | ListId::Followings(_) => ListItems::Users(Vec::new()),
        ListId::Search {
            kind: SearchKind::Playlists | SearchKind::Albums,
        }
        | ListId::UserPlaylists(_)
        | ListId::Library => ListItems::Playlists(Vec::new()),
        _ => ListItems::Tracks(Vec::new()),
    }
}

fn len(items: &ListItems) -> usize {
    match items {
        ListItems::Tracks(items) => items.len(),
        ListItems::Users(items) => items.len(),
        ListItems::Playlists(items) => items.len(),
    }
}

/// One paged list as the screens see it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListView {
    pub items: ListItems,
    pub has_more: bool,
    /// The first page is on its way.
    pub loading: bool,
    /// A next page is on its way.
    pub loading_more: bool,
    /// The first page failed; the person can try again.
    pub error: bool,
}

impl ListView {
    /// Nothing requested yet: an empty list.
    pub fn empty(list: ListId) -> Self {
        Self {
            items: empty_items(list),
            has_more: false,
            loading: false,
            loading_more: false,
            error: false,
        }
    }

    pub fn len(&self) -> usize {
        len(&self.items)
    }

    /// The first page was requested.
    pub fn begin(&mut self) {
        self.loading = true;
        self.loading_more = false;
        self.error = false;
    }

    /// The first page arrived.
    pub fn replace(&mut self, items: ListItems, has_more: bool) {
        self.items = items;
        self.has_more = has_more;
        self.loading = false;
        self.loading_more = false;
        self.error = false;
    }

    /// A next page arrived; a page of another kind is dropped.
    pub fn append(&mut self, page: ListItems, has_more: bool) {
        match (&mut self.items, page) {
            (ListItems::Tracks(items), ListItems::Tracks(page)) => items.extend(page),
            (ListItems::Users(items), ListItems::Users(page)) => items.extend(page),
            (ListItems::Playlists(items), ListItems::Playlists(page)) => items.extend(page),
            _ => tracing::warn!("a list page of another kind was dropped"),
        }
        self.has_more = has_more;
        self.loading_more = false;
    }

    /// A page could not be fetched. The core ends a list whose next page
    /// failed, so the list stops asking for more.
    pub fn fail(&mut self, append: bool) {
        if append {
            self.loading_more = false;
            self.has_more = false;
        } else {
            self.loading = false;
            self.error = true;
        }
    }

    /// True once per page: when the rows on screen end near the end of the
    /// list and the core has more. Marks the page as loading.
    pub fn take_load_more(&mut self, visible_end: usize) -> bool {
        let near_end = visible_end + LOAD_MORE_MARGIN >= self.len();
        if self.loading || self.error || !self.has_more || self.loading_more || !near_end {
            return false;
        }
        self.loading_more = true;
        true
    }
}

/// A page whose header the core answers with.
#[derive(Debug, Clone, PartialEq)]
pub enum Page<T> {
    Loading,
    Ready(T),
    Failed,
}

impl<T> Page<T> {
    /// Waits for a header again after a failure; a shown header stays.
    fn expect(&mut self) {
        if matches!(self, Self::Failed) {
            *self = Self::Loading;
        }
    }

    fn fail_if_loading(&mut self) -> bool {
        let loading = matches!(self, Self::Loading);
        if loading {
            *self = Self::Failed;
        }
        loading
    }
}

/// Image files by what they show.
#[derive(Debug, Default)]
pub struct Art {
    /// Never cleared: the core sends each file once.
    pub tracks: ArtworkMap,
    pub users: HashMap<UserId, Arc<Path>>,
    pub playlists: HashMap<PlaylistId, Arc<Path>>,
    /// Dominant colour by image: present once requested, `Some` once computed
    /// (`None` when the file was unreadable).
    tints: HashMap<ArtKey, Option<Rgb>>,
}

impl Art {
    /// Marks the image's colour as requested. True only the first time, so
    /// each image is decoded once.
    pub fn begin_tint(&mut self, key: ArtKey) -> bool {
        if self.tints.contains_key(&key) {
            return false;
        }
        self.tints.insert(key, None);
        true
    }

    pub fn set_tint(&mut self, key: ArtKey, color: Option<Rgb>) {
        self.tints.insert(key, color);
    }

    pub fn tint(&self, key: ArtKey) -> Option<Rgb> {
        self.tints.get(&key).copied().flatten()
    }
}

/// Everything the browsing screens render.
#[derive(Debug)]
pub struct Models {
    pub lists: HashMap<ListId, ListView>,
    pub tracks: HashMap<TrackId, Page<TrackPage>>,
    pub users: HashMap<UserId, Page<UserPage>>,
    pub playlists: HashMap<PlaylistId, Page<PlaylistPage>>,
    /// The text of the search field; empty shows the search prompt.
    pub query: String,
    pub search_kind: SearchKind,
    pub art: Art,
    /// The track the player is on, for the active row.
    pub current: Option<TrackId>,
    /// The player is playing (the equalizer moves), not paused or loading.
    pub playing: bool,
    /// The signed-in person, once the core says so.
    pub account: Option<UserSummary>,
    /// The sign-in window is open or a token is being checked.
    pub signing_in: bool,
    /// The person's liked tracks and the people they follow.
    pub liked: HashSet<TrackId>,
    pub followed: HashSet<UserId>,
    /// The Jam this person hosts or joined.
    pub jam: Option<JamState>,
    /// SoundCloud's own home rows, and the genre picked for Trending.
    pub home_shelves: Vec<HomeShelf>,
    pub home_genre: Genre,
    /// The settings in effect, as the core last said.
    pub settings: sc_core::Settings,
}

impl Models {
    pub fn new() -> Self {
        Self {
            lists: HashMap::new(),
            tracks: HashMap::new(),
            users: HashMap::new(),
            playlists: HashMap::new(),
            query: String::new(),
            search_kind: SearchKind::Tracks,
            art: Art::default(),
            current: None,
            playing: false,
            account: None,
            signing_in: false,
            liked: HashSet::new(),
            followed: HashSet::new(),
            jam: None,
            home_shelves: Vec::new(),
            home_genre: Genre::All,
            settings: sc_core::Settings::default(),
        }
    }

    /// The list, created empty on first use.
    pub fn list_mut(&mut self, list: ListId) -> &mut ListView {
        self.lists
            .entry(list)
            .or_insert_with(|| ListView::empty(list))
    }

    /// Marks a list as waiting for its first page.
    pub fn begin(&mut self, list: ListId) {
        self.list_mut(list).begin();
    }

    /// A page that is not showing a header yet waits for it.
    pub fn expect_track(&mut self, id: TrackId) {
        self.tracks.entry(id).or_insert(Page::Loading).expect();
    }

    pub fn expect_user(&mut self, id: UserId) {
        self.users.entry(id).or_insert(Page::Loading).expect();
    }

    pub fn expect_playlist(&mut self, id: PlaylistId) {
        self.playlists.entry(id).or_insert(Page::Loading).expect();
    }

    /// The core could not answer: pages still waiting for a header show the
    /// error. Returns whether any did.
    pub fn fail_loading_pages(&mut self) -> bool {
        let tracks = self
            .tracks
            .values_mut()
            .fold(false, |any, page| page.fail_if_loading() | any);
        let users = self
            .users
            .values_mut()
            .fold(false, |any, page| page.fail_if_loading() | any);
        let playlists = self
            .playlists
            .values_mut()
            .fold(false, |any, page| page.fail_if_loading() | any);
        tracks | users | playlists
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sc_core::TrackSummary;

    use super::*;

    const LIST: ListId = ListId::History;

    fn track(id: u64) -> TrackSummary {
        TrackSummary {
            id: TrackId(id),
            title: format!("Track {id}"),
            artist: "Artist".into(),
            artist_id: None,
            duration: Duration::from_secs(200),
            preview_only: false,
        }
    }

    fn tracks(ids: &[u64]) -> ListItems {
        ListItems::Tracks(ids.iter().map(|id| track(*id)).collect())
    }

    fn ready(ids: &[u64], has_more: bool) -> ListView {
        let mut list = ListView::empty(LIST);
        list.replace(tracks(ids), has_more);
        list
    }

    #[test]
    fn a_list_starts_with_the_items_of_its_kind() {
        let kind = |list| ListView::empty(list).items;
        assert!(matches!(kind(LIST), ListItems::Tracks(_)));
        let people = ListId::Search {
            kind: SearchKind::People,
        };
        assert!(matches!(kind(people), ListItems::Users(_)));
        let albums = ListId::Search {
            kind: SearchKind::Albums,
        };
        assert!(matches!(kind(albums), ListItems::Playlists(_)));
        assert!(matches!(
            kind(ListId::UserPlaylists(UserId(1))),
            ListItems::Playlists(_)
        ));
    }

    #[test]
    fn the_first_page_replaces_and_a_next_page_is_appended() {
        let mut list = ready(&[1, 2], true);
        list.begin();
        assert!(list.loading);

        list.replace(tracks(&[3]), true);
        assert!(!list.loading);
        assert_eq!(list.items, tracks(&[3]));

        list.loading_more = true;
        list.append(tracks(&[4, 5]), false);
        assert_eq!(list.items, tracks(&[3, 4, 5]));
        assert!(!list.has_more);
        assert!(!list.loading_more);
    }

    #[test]
    fn a_page_of_another_kind_is_dropped() {
        let mut list = ready(&[1], true);
        list.append(ListItems::Users(vec![]), true);
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn a_failed_first_page_shows_the_error_until_retried() {
        let mut list = ListView::empty(LIST);
        list.begin();
        list.fail(false);
        assert!(list.error && !list.loading);

        list.begin();
        assert!(!list.error && list.loading);
    }

    #[test]
    fn a_failed_next_page_stops_asking_for_more() {
        let mut list = ready(&[1, 2], true);
        assert!(list.take_load_more(2));
        list.fail(true);
        assert!(!list.loading_more && !list.error);
        assert!(!list.take_load_more(2));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn load_more_fires_once_near_the_end() {
        let ids: Vec<u64> = (1..=30).collect();
        let mut list = ready(&ids, true);
        assert!(!list.take_load_more(10), "far from the end");
        assert!(list.take_load_more(26), "within the margin");
        assert!(!list.take_load_more(30), "already loading");

        list.append(tracks(&[31]), true);
        assert!(list.take_load_more(31), "the next page can be requested");
    }

    #[test]
    fn load_more_needs_more_pages_and_a_settled_list() {
        assert!(!ready(&[1, 2], false).take_load_more(2), "last page");

        let mut loading = ready(&[1, 2], true);
        loading.begin();
        assert!(!loading.take_load_more(2));

        let mut failed = ready(&[1, 2], true);
        failed.fail(false);
        assert!(!failed.take_load_more(2));
    }

    #[test]
    fn a_failed_page_header_is_requested_again() {
        let mut models = Models::new();
        models.expect_user(UserId(7));
        assert_eq!(models.users[&UserId(7)], Page::Loading);

        models.users.insert(UserId(7), Page::Failed);
        models.expect_user(UserId(7));
        assert_eq!(models.users[&UserId(7)], Page::Loading);
    }

    #[test]
    fn only_pages_still_waiting_fail() {
        let mut models = Models::new();
        models.expect_playlist(PlaylistId(1));
        assert!(models.fail_loading_pages());
        assert_eq!(models.playlists[&PlaylistId(1)], Page::Failed);
        assert!(!models.fail_loading_pages(), "nothing left waiting");
    }

    #[test]
    fn an_image_colour_is_requested_once() {
        let mut art = Art::default();
        let key = ArtKey::Track(TrackId(1));
        assert!(art.begin_tint(key));
        assert!(!art.begin_tint(key), "already requested");
        assert_eq!(art.tint(key), None, "not computed yet");

        art.set_tint(key, Some(Rgb(255, 85, 0)));
        assert!(!art.begin_tint(key), "a computed colour is kept");
        assert_eq!(art.tint(key), Some(Rgb(255, 85, 0)));
    }

    #[test]
    fn search_tabs_keep_their_order() {
        for (i, kind) in SEARCH_TABS.into_iter().enumerate() {
            assert_eq!(tab_index(kind), i);
        }
    }
}
