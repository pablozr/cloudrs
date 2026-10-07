//! The signed-in person's playlists: create, add and remove tracks, reorder,
//! rename, change privacy and delete (ADR 0014).
//!
//! SoundCloud takes the whole track list on every change, so a change to the
//! tracks first reads the current list from SoundCloud: an edit made on the
//! website in the meantime is not lost.

use std::sync::Arc;

use sc_api::SoundCloudApi;
use sc_api::models::{Playlist, PlaylistEdit, sharing};

use super::{Core, Input};
use crate::Event;
use crate::types::{ListId, PlaylistChange, PlaylistId, PlaylistSummary, Problem, TrackId};

/// A change to send, before SoundCloud's answer.
#[derive(Debug, Clone)]
pub(super) enum Edit {
    Create {
        title: String,
        track: Option<TrackId>,
    },
    Add(TrackId),
    Remove(usize),
    Move {
        from: usize,
        to: usize,
    },
    Rename(String),
    Privacy(bool),
    Delete,
}

/// Applies a change to a track list. `None` when there is nothing to do.
fn edit_tracks(mut ids: Vec<u64>, edit: &Edit) -> Option<Vec<u64>> {
    match *edit {
        Edit::Add(track) if !ids.contains(&track.0) => ids.push(track.0),
        Edit::Remove(index) if index < ids.len() => {
            ids.remove(index);
        }
        Edit::Move { from, to } if from < ids.len() && to < ids.len() && from != to => {
            let id = ids.remove(from);
            ids.insert(to, id);
        }
        _ => return None,
    }
    Some(ids)
}

/// Sends one change to SoundCloud. Answers the playlist as it is after it,
/// and what changed (`AlreadyThere` when the track was in it already).
async fn send<A: SoundCloudApi>(
    api: &A,
    id: Option<PlaylistId>,
    edit: Edit,
) -> sc_api::Result<(Playlist, PlaylistChange)> {
    let id = id.map(|p| p.0).unwrap_or_default();
    match edit {
        Edit::Create { title, track } => {
            let tracks: Vec<u64> = track.into_iter().map(|t| t.0).collect();
            let playlist = api.create_playlist(&title, false, &tracks).await?;
            Ok((playlist, PlaylistChange::Created))
        }
        Edit::Rename(title) => {
            let edit = PlaylistEdit {
                title: Some(title),
                ..PlaylistEdit::default()
            };
            Ok((api.edit_playlist(id, &edit).await?, PlaylistChange::Renamed))
        }
        Edit::Privacy(public) => {
            let edit = PlaylistEdit {
                sharing: Some(sharing(public).to_owned()),
                ..PlaylistEdit::default()
            };
            let change = PlaylistChange::Privacy { public };
            Ok((api.edit_playlist(id, &edit).await?, change))
        }
        Edit::Delete => {
            let playlist = api.playlist(id).await?;
            api.delete_playlist(id).await?;
            Ok((playlist, PlaylistChange::Deleted))
        }
        Edit::Add(_) | Edit::Remove(_) | Edit::Move { .. } => {
            let current = api.playlist(id).await?;
            let ids = current.tracks.iter().map(|t| t.id).collect();
            let change = match edit {
                Edit::Add(track) => PlaylistChange::Added(track),
                Edit::Remove(_) => PlaylistChange::Removed,
                _ => PlaylistChange::Moved,
            };
            let Some(tracks) = edit_tracks(ids, &edit) else {
                let change = match change {
                    PlaylistChange::Added(track) => PlaylistChange::AlreadyThere(track),
                    change => change,
                };
                return Ok((current, change));
            };
            let edit = PlaylistEdit {
                tracks: Some(tracks),
                ..PlaylistEdit::default()
            };
            Ok((api.edit_playlist(id, &edit).await?, change))
        }
    }
}

impl<A: SoundCloudApi + 'static> Core<A> {
    /// Sends a change to one of the person's playlists (`None` creates one).
    pub(super) fn edit_playlist(&mut self, id: Option<PlaylistId>, edit: Edit) {
        if self.account.is_none() {
            self.emit(Event::Problem(Problem::SignInRequired));
            return;
        }
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = send(&*api, id, edit).await.map(Box::new);
            let _ = inputs.send(Input::PlaylistEdited { result });
        });
    }

    pub(super) fn playlist_edited(
        &mut self,
        result: sc_api::Result<Box<(Playlist, PlaylistChange)>>,
    ) {
        let (playlist, change) = match result {
            Ok(done) => *done,
            Err(error) => {
                if !self.expired(&error) {
                    tracing::warn!(%error, "a playlist change was not saved");
                    self.emit(Event::Problem(Problem::PlaylistNotSaved));
                }
                return;
            }
        };
        self.emit(Event::PlaylistSaved {
            playlist: PlaylistSummary::from_api(&playlist),
            change,
        });
        match change {
            PlaylistChange::Created | PlaylistChange::Deleted => self.reload_library(),
            // Renaming and privacy show in the library rows too.
            PlaylistChange::Renamed | PlaylistChange::Privacy { .. } => {
                self.reload_library();
                self.playlist_opened(playlist);
            }
            PlaylistChange::Added(_) | PlaylistChange::Removed | PlaylistChange::Moved => {
                self.playlist_opened(playlist);
            }
            PlaylistChange::AlreadyThere(_) => {}
        }
    }

    /// The library's first page again, after a playlist came or went.
    fn reload_library(&mut self) {
        let list = ListId::Library;
        let generation = self.reset_list(list);
        self.fetch_list(list, generation, None, std::time::Duration::ZERO);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_edits_change_the_whole_list() {
        let ids = vec![1, 2, 3];
        assert_eq!(
            edit_tracks(ids.clone(), &Edit::Add(TrackId(4))),
            Some(vec![1, 2, 3, 4])
        );
        assert_eq!(
            edit_tracks(ids.clone(), &Edit::Add(TrackId(2))),
            None,
            "already there"
        );
        assert_eq!(edit_tracks(ids.clone(), &Edit::Remove(0)), Some(vec![2, 3]));
        assert_eq!(edit_tracks(ids.clone(), &Edit::Remove(9)), None);
        assert_eq!(
            edit_tracks(ids.clone(), &Edit::Move { from: 0, to: 2 }),
            Some(vec![2, 3, 1])
        );
        assert_eq!(edit_tracks(ids, &Edit::Move { from: 1, to: 1 }), None);
    }
}
