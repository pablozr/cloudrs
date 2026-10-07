//! The lists the core serves and pages (ADR 0008), and turning API pages into
//! rows.

use std::sync::Arc;
use std::time::Duration;

use sc_api::SoundCloudApi;
use sc_api::models::{Playlist, Track};

use super::{Core, Input};
use crate::lists::{self, Fetched, ListState, SEARCH_KINDS};
use crate::types::{
    ArtKey, ListId, ListItems, PlaylistId, PlaylistSummary, Problem, TrackId, TrackSummary, UserId,
    UserSummary,
};
use crate::{Event, artwork};

impl<A: SoundCloudApi + 'static> Core<A> {
    /// Replaces the state of `list` with a fresh, loading one. Dropping the
    /// old state cancels its fetch; the new generation drops its late answers.
    pub(super) fn reset_list(&mut self, list: ListId) -> u64 {
        self.list_gen += 1;
        self.lists.insert(list, ListState::new(self.list_gen));
        self.list_gen
    }

    /// Restarts the search of the current kind, waiting `delay` first. A new
    /// search invalidates the lists of every tab.
    pub(super) fn start_search(&mut self, delay: Duration) {
        for kind in SEARCH_KINDS {
            self.lists.remove(&ListId::Search { kind });
        }
        let list = ListId::Search { kind: self.kind };
        if self.query.is_empty() {
            self.emit(Event::List {
                list,
                items: ListItems::empty(self.kind),
                append: false,
                has_more: false,
            });
            return;
        }
        let generation = self.reset_list(list);
        self.fetch_list(list, generation, None, delay);
    }

    /// Fetches a page of an API-backed list in the background.
    pub(super) fn fetch_list(
        &mut self,
        list: ListId,
        generation: u64,
        next: Option<String>,
        delay: Duration,
    ) {
        let (api, inputs, events) = (
            Arc::clone(&self.api),
            self.inputs.clone(),
            self.events.clone(),
        );
        let query = self.query.clone();
        let task = tokio::spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let append = next.is_some();
            if let ListId::Search { kind } = list
                && !append
            {
                let _ = events.send(Event::Searching {
                    query: query.clone(),
                    kind,
                });
            }
            let result = lists::fetch(&*api, list, &query, next).await;
            let _ = inputs.send(Input::ListDone {
                list,
                generation,
                append,
                result,
            });
        });
        if let Some(state) = self.lists.get_mut(&list) {
            state.task = Some(task);
        }
    }

    /// The next page of a list. A list never served loads its first page.
    pub(super) fn load_more(&mut self, list: ListId) {
        let Some(state) = self.lists.get_mut(&list) else {
            self.load_first(list);
            return;
        };
        if state.loading {
            return;
        }
        let Some(next) = state.next_href.clone() else {
            return;
        };
        state.loading = true;
        let generation = state.generation;
        self.fetch_list(list, generation, Some(next), Duration::ZERO);
    }

    pub(super) fn load_first(&mut self, list: ListId) {
        match list {
            ListId::Playlist(id) => self.open_playlist(id),
            ListId::History => self.open_history(),
            ListId::Search { .. } if self.query.is_empty() => {}
            _ => {
                let generation = self.reset_list(list);
                self.fetch_list(list, generation, None, Duration::ZERO);
            }
        }
    }

    pub(super) fn list_done(
        &mut self,
        list: ListId,
        generation: u64,
        append: bool,
        result: sc_api::Result<Fetched>,
    ) {
        if self
            .lists
            .get(&list)
            .is_none_or(|s| s.generation != generation)
        {
            return;
        }
        let fetched = match result {
            Ok(fetched) => fetched,
            Err(error) => {
                // A failed first page can be asked for again (no state); a
                // failed next page is not offered again.
                if append {
                    if let Some(state) = self.lists.get_mut(&list) {
                        state.loading = false;
                        state.task = None;
                        state.next_href = None;
                    }
                } else {
                    self.lists.remove(&list);
                }
                self.expired(&error);
                self.emit(Event::ListFailed {
                    list,
                    append,
                    problem: Problem::from_api(&error),
                });
                return;
            }
        };
        let next_href = fetched.next_href();
        let (items, art) = self.absorb(fetched);
        if let Some(state) = self.lists.get_mut(&list) {
            state.loading = false;
            state.task = None;
            state.next_href.clone_from(&next_href);
            if let ListItems::Tracks(tracks) = &items {
                if !append {
                    state.tracks.clear();
                }
                state.tracks.extend(tracks.iter().cloned());
            }
        }
        self.emit(Event::List {
            list,
            items,
            append,
            has_more: next_href.is_some(),
        });
        for key in art {
            self.request_artwork(key);
        }
    }

    /// Remembers what a page carries (tracks to play later, image URLs) and
    /// turns it into rows.
    pub(super) fn absorb(&mut self, fetched: Fetched) -> (ListItems, Vec<ArtKey>) {
        match fetched {
            Fetched::Tracks(page) => self.absorb_tracks(page.collection),
            Fetched::Likes(page) => self.absorb_tracks(
                page.collection
                    .into_iter()
                    .filter_map(|l| l.track)
                    .collect(),
            ),
            Fetched::Users(page) => {
                let mut art = Vec::new();
                let users = page
                    .collection
                    .iter()
                    .map(|user| {
                        let id = UserId(user.id);
                        if let Some(url) = user.avatar(artwork::SIZE) {
                            self.other_art.insert(ArtKey::User(id), url);
                            art.push(ArtKey::User(id));
                        }
                        UserSummary::from_api(user)
                    })
                    .collect();
                (ListItems::Users(users), art)
            }
            Fetched::Playlists(page) => self.absorb_playlists(&page.collection),
            // Playlists in the feed come later; the feed is tracks for now.
            Fetched::Feed(page) => self.absorb_tracks(
                page.collection
                    .into_iter()
                    .filter_map(|item| item.track)
                    .collect(),
            ),
            Fetched::Library(page) => {
                let playlists: Vec<Playlist> = page
                    .collection
                    .into_iter()
                    .filter_map(|item| item.playlist)
                    .collect();
                self.absorb_playlists(&playlists)
            }
        }
    }

    pub(super) fn absorb_playlists(&mut self, playlists: &[Playlist]) -> (ListItems, Vec<ArtKey>) {
        let mut art = Vec::new();
        let rows = playlists
            .iter()
            .map(|playlist| {
                let id = PlaylistId(playlist.id);
                if let Some(url) = playlist.artwork(artwork::SIZE) {
                    self.other_art.insert(ArtKey::Playlist(id), url);
                    art.push(ArtKey::Playlist(id));
                }
                PlaylistSummary::from_api(playlist)
            })
            .collect();
        (ListItems::Playlists(rows), art)
    }

    pub(super) fn absorb_tracks(&mut self, tracks: Vec<Track>) -> (ListItems, Vec<ArtKey>) {
        let summaries = tracks.iter().map(TrackSummary::from_api).collect();
        let art = tracks
            .iter()
            .map(|t| ArtKey::Track(TrackId(t.id)))
            .collect();
        for track in tracks {
            self.tracks.insert(TrackId(track.id), track);
        }
        (ListItems::Tracks(summaries), art)
    }

    /// A track summary from any list or the tracks seen so far.
    pub(super) fn find_summary(&self, id: TrackId) -> Option<TrackSummary> {
        self.tracks
            .get(&id)
            .map(TrackSummary::from_api)
            .or_else(|| {
                self.lists
                    .values()
                    .find_map(|state| state.tracks.iter().find(|t| t.id == id).cloned())
            })
    }
}
