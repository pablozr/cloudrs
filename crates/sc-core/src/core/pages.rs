//! The screens' headers: track, profile, playlist and history (ADR 0008).

use std::collections::HashMap;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use sc_api::SoundCloudApi;
use sc_api::models::Resource;
use sc_api::models::{Playlist, Track, User};

use super::{Core, Input};
use crate::store::{self, SessionTrack};
use crate::types::{
    ArtKey, ListId, ListItems, PlaylistId, PlaylistPage, Problem, TrackId, TrackPage, TrackSummary,
    UserId, UserPage,
};
use crate::{Event, artwork, waveform};

/// Tracks the history screen lists.
const HISTORY_LIMIT: u32 = 200;
/// Ids per `/tracks?ids=` call when filling a playlist.
const FILL_BATCH: usize = 50;

/// Completes the tracks of a playlist that only carry an id, fetching them
/// `FILL_BATCH` at a time. Tracks SoundCloud no longer returns are dropped.
async fn fill_tracks<A: SoundCloudApi>(api: &A, tracks: Vec<Track>) -> sc_api::Result<Vec<Track>> {
    let missing: Vec<u64> = tracks
        .iter()
        .filter(|t| t.title.is_empty())
        .map(|t| t.id)
        .collect();
    let mut filled = HashMap::new();
    for batch in missing.chunks(FILL_BATCH) {
        for track in api.tracks(batch).await? {
            filled.insert(track.id, track);
        }
    }
    Ok(tracks
        .into_iter()
        .filter_map(|t| {
            if t.title.is_empty() {
                filled.remove(&t.id)
            } else {
                Some(t)
            }
        })
        .collect())
}

impl<A: SoundCloudApi + 'static> Core<A> {
    pub(super) fn track_opened(&mut self, track: Track) {
        let id = TrackId(track.id);
        self.emit(Event::TrackPage(TrackPage::from_api(&track)));
        if let Some(url) = track.waveform_url.clone() {
            let (api, events) = (Arc::clone(&self.api), self.events.clone());
            tokio::spawn(async move {
                if let Ok(wave) = api.waveform(&url).await {
                    let bars = waveform::to_bars(&wave.samples, wave.height, waveform::BARS);
                    let _ = events.send(Event::Waveform { track: id, bars });
                }
            });
        }
        self.tracks.insert(id, track);
        self.request_artwork(ArtKey::Track(id));

        let list = ListId::Related(id);
        let generation = self.reset_list(list);
        self.fetch_list(list, generation, None, Duration::ZERO);
    }

    /// The profile header, then the first page of its tracks. Playlists and
    /// likes start over: they load when the UI asks.
    pub(super) fn user_opened(&mut self, user: &User) {
        let id = UserId(user.id);
        self.emit(Event::UserPage(UserPage::from_api(user)));
        if let Some(url) = user.avatar(artwork::SIZE) {
            self.other_art.insert(ArtKey::User(id), url);
            self.request_artwork(ArtKey::User(id));
        }
        self.lists.remove(&ListId::UserPlaylists(id));
        self.lists.remove(&ListId::UserLikes(id));

        let list = ListId::UserTracks(id);
        let generation = self.reset_list(list);
        self.fetch_list(list, generation, None, Duration::ZERO);
    }

    pub(super) fn open_playlist(&mut self, id: PlaylistId) {
        let nav = self.next_nav();
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.playlist(id.0).await.map(Box::new);
            let _ = inputs.send(Input::PlaylistOpened { nav, result });
        });
    }

    /// The playlist header, then every track: the ones that only carry an id
    /// are filled in batches before the list is sent.
    pub(super) fn playlist_opened(&mut self, playlist: Playlist) {
        let id = PlaylistId(playlist.id);
        self.emit(Event::PlaylistPage(PlaylistPage::from_api(&playlist)));
        if let Some(url) = playlist.artwork(artwork::SIZE) {
            self.other_art.insert(ArtKey::Playlist(id), url);
            self.request_artwork(ArtKey::Playlist(id));
        }

        let list = ListId::Playlist(id);
        let generation = self.reset_list(list);
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        let task = tokio::spawn(async move {
            let result = fill_tracks(&*api, playlist.tracks).await;
            let _ = inputs.send(Input::PlaylistTracks {
                id,
                generation,
                result,
            });
        });
        if let Some(state) = self.lists.get_mut(&list) {
            state.task = Some(task);
        }
    }

    pub(super) fn playlist_tracks(
        &mut self,
        id: PlaylistId,
        generation: u64,
        result: sc_api::Result<Vec<Track>>,
    ) {
        let list = ListId::Playlist(id);
        if self
            .lists
            .get(&list)
            .is_none_or(|s| s.generation != generation)
        {
            return;
        }
        match result {
            Ok(tracks) => {
                let (items, art) = self.absorb_tracks(tracks);
                if let Some(state) = self.lists.get_mut(&list) {
                    state.loading = false;
                    state.task = None;
                    if let ListItems::Tracks(tracks) = &items {
                        state.tracks.clone_from(tracks);
                    }
                }
                self.emit(Event::List {
                    list,
                    items,
                    append: false,
                    has_more: false,
                });
                for key in art {
                    self.request_artwork(key);
                }
            }
            Err(error) => {
                self.lists.remove(&list);
                self.emit(Event::ListFailed {
                    list,
                    append: false,
                    problem: Problem::from_api(&error),
                });
            }
        }
    }

    pub(super) fn open_history(&mut self) {
        let generation = self.reset_list(ListId::History);
        let Some(store) = self.store.clone() else {
            // The database is not open (yet): nothing was played that we know of.
            self.history_loaded(generation, Ok(Vec::new()));
            return;
        };
        let inputs = self.inputs.clone();
        tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(PoisonError::into_inner);
            let result = store::recent_history(&store.conn, HISTORY_LIMIT);
            let _ = inputs.send(Input::HistoryLoaded { generation, result });
        });
    }

    pub(super) fn history_loaded(
        &mut self,
        generation: u64,
        result: rusqlite::Result<Vec<SessionTrack>>,
    ) {
        let list = ListId::History;
        if self
            .lists
            .get(&list)
            .is_none_or(|s| s.generation != generation)
        {
            return;
        }
        let rows = match result {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "could not read the history");
                self.lists.remove(&list);
                self.emit(Event::ListFailed {
                    list,
                    append: false,
                    problem: Problem::StorageReset,
                });
                return;
            }
        };
        let mut tracks = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(url) = row.artwork_url {
                self.restored_artwork.entry(row.track.id).or_insert(url);
            }
            tracks.push(row.track);
        }
        if let Some(state) = self.lists.get_mut(&list) {
            state.loading = false;
            state.tracks.clone_from(&tracks);
        }
        let ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        self.emit(Event::List {
            list,
            items: ListItems::Tracks(tracks),
            append: false,
            has_more: false,
        });
        for id in ids {
            self.request_artwork(ArtKey::Track(id));
        }
    }

    /// A pasted link: a track plays, a profile or playlist opens its screen.
    pub(super) fn resolved(&mut self, result: sc_api::Result<Resource>) {
        match result {
            Ok(Resource::Track(track)) => {
                let id = TrackId(track.id);
                let summary = TrackSummary::from_api(&track);
                self.tracks.insert(id, *track);
                self.queue.set_context(vec![summary], 0);
                self.queue_changed();
                self.play_current(None, false);
            }
            Ok(Resource::User(user)) => self.user_opened(&user),
            Ok(Resource::Playlist(playlist)) => self.playlist_opened(*playlist),
            Ok(Resource::Unknown) => self.emit(Event::Problem(Problem::UnsupportedLink)),
            Err(error) => self.emit(Event::Problem(Problem::from_api(&error))),
        }
    }

    pub(super) fn open_track(&mut self, id: TrackId) {
        let nav = self.next_nav();
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.track(id.0).await.map(Box::new);
            let _ = inputs.send(Input::TrackOpened { nav, result });
        });
    }

    pub(super) fn open_user(&mut self, id: UserId) {
        let nav = self.next_nav();
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.user(id.0).await.map(Box::new);
            let _ = inputs.send(Input::UserOpened { nav, result });
        });
    }

    pub(super) fn open_url(&mut self, url: String) {
        let nav = self.next_nav();
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.resolve(url.trim()).await;
            let _ = inputs.send(Input::Resolved { nav, result });
        });
    }
}
