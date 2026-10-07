//! The lists the core serves (ADR 0008): the paging state of each one and the
//! API call behind each kind.

use sc_api::SoundCloudApi;
use sc_api::models::{Like, Page, Playlist, Track, User};
use tokio::task::JoinHandle;

use crate::types::{ListId, ListItems, SearchKind, TrackSummary};

/// Rows per page.
pub const PAGE_SIZE: u32 = 30;

/// Every search tab, to drop the lists of all of them when a search restarts.
pub const SEARCH_KINDS: [SearchKind; 4] = [
    SearchKind::Tracks,
    SearchKind::People,
    SearchKind::Playlists,
    SearchKind::Albums,
];

/// Where a list is: what the core served and how to get the next page.
/// A list the core never served has no state at all.
pub struct ListState {
    /// Answers carrying another generation are stale and dropped.
    pub generation: u64,
    /// The track rows served so far: the context of `Command::Play`.
    pub tracks: Vec<TrackSummary>,
    /// Where the next page is. `None`: the list is over, or a page failed.
    pub next_href: Option<String>,
    /// A page is being fetched.
    pub loading: bool,
    pub task: Option<JoinHandle<()>>,
}

impl ListState {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            tracks: Vec::new(),
            next_href: None,
            loading: true,
            task: None,
        }
    }
}

/// A list that is dropped or replaced stops its fetch.
impl Drop for ListState {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl ListItems {
    pub fn empty(kind: SearchKind) -> Self {
        match kind {
            SearchKind::Tracks => Self::Tracks(Vec::new()),
            SearchKind::People => Self::Users(Vec::new()),
            SearchKind::Playlists | SearchKind::Albums => Self::Playlists(Vec::new()),
        }
    }
}

/// One page as the API returned it.
pub enum Fetched {
    Tracks(Page<Track>),
    Users(Page<User>),
    Playlists(Page<Playlist>),
    Likes(Page<Like>),
}

impl Fetched {
    pub fn next_href(&self) -> Option<String> {
        match self {
            Self::Tracks(page) => page.next_href.clone(),
            Self::Users(page) => page.next_href.clone(),
            Self::Playlists(page) => page.next_href.clone(),
            Self::Likes(page) => page.next_href.clone(),
        }
    }
}

fn empty_page<T>() -> Page<T> {
    Page {
        collection: Vec::new(),
        next_href: None,
        total_results: None,
    }
}

/// The page after `href`, typed like the list it belongs to. A macro because
/// naming the trait bound of `next_page` would need `serde` as a dependency.
macro_rules! more {
    ($api:expr, $href:expr) => {{
        let from = Page {
            next_href: Some($href),
            ..empty_page()
        };
        Ok::<_, sc_api::Error>($api.next_page(&from).await?.unwrap_or_else(empty_page))
    }};
}

/// Fetches the first page of `list` (`next: None`) or the page at `next`.
/// `Playlist` and `History` are not paged from the API and never get here.
pub async fn fetch<A: SoundCloudApi>(
    api: &A,
    list: ListId,
    query: &str,
    next: Option<String>,
) -> sc_api::Result<Fetched> {
    if let Some(href) = next {
        return match list {
            ListId::Search {
                kind: SearchKind::People,
            } => more!(api, href).map(Fetched::Users),
            ListId::Search {
                kind: SearchKind::Playlists | SearchKind::Albums,
            }
            | ListId::UserPlaylists(_) => more!(api, href).map(Fetched::Playlists),
            ListId::UserLikes(_) => more!(api, href).map(Fetched::Likes),
            ListId::Search {
                kind: SearchKind::Tracks,
            }
            | ListId::UserTracks(_)
            | ListId::Related(_)
            | ListId::Playlist(_)
            | ListId::History => more!(api, href).map(Fetched::Tracks),
        };
    }
    match list {
        ListId::Search { kind } => match kind {
            SearchKind::Tracks => api
                .search_tracks(query, PAGE_SIZE)
                .await
                .map(Fetched::Tracks),
            SearchKind::People => api.search_users(query, PAGE_SIZE).await.map(Fetched::Users),
            SearchKind::Playlists => api
                .search_playlists(query, PAGE_SIZE)
                .await
                .map(Fetched::Playlists),
            SearchKind::Albums => api
                .search_albums(query, PAGE_SIZE)
                .await
                .map(Fetched::Playlists),
        },
        ListId::UserTracks(id) => api.user_tracks(id.0, PAGE_SIZE).await.map(Fetched::Tracks),
        ListId::UserPlaylists(id) => api
            .user_playlists(id.0, PAGE_SIZE)
            .await
            .map(Fetched::Playlists),
        ListId::UserLikes(id) => api.user_likes(id.0, PAGE_SIZE).await.map(Fetched::Likes),
        ListId::Related(id) => api.related(id.0, PAGE_SIZE).await.map(Fetched::Tracks),
        ListId::Playlist(_) | ListId::History => Err(sc_api::Error::NotFound),
    }
}
