//! The signed-in person's playlists: create (with description, privacy,
//! genre, tags and a cover), add and remove tracks, reorder, rename, describe,
//! change privacy or cover, and delete (ADR 0014, ADR 0015).
//!
//! SoundCloud takes the whole track list on every change, so a change to the
//! tracks first reads the current list from SoundCloud: an edit made on the
//! website in the meantime is not lost.

use std::path::PathBuf;
use std::sync::Arc;

use sc_api::SoundCloudApi;
use sc_api::models::{Playlist, PlaylistEdit, sharing};

use super::{Core, Input};
use crate::Event;
use crate::types::{
    ListId, NewPlaylist, PlaylistChange, PlaylistId, PlaylistSummary, Problem, TrackId,
};

/// A change to send, before SoundCloud's answer.
#[derive(Debug, Clone)]
pub(super) enum Edit {
    Create(Box<NewPlaylist>),
    /// At the end, or at this place.
    Add(TrackId, Option<usize>),
    Remove(usize),
    Move {
        from: usize,
        to: usize,
    },
    Rename(String),
    Describe(String),
    Privacy(bool),
    /// A cover image from this file.
    Cover(PathBuf),
    Delete,
}

/// SoundCloud's answer to a change.
pub(super) struct Saved {
    pub playlist: Playlist,
    pub change: PlaylistChange,
    /// A cover sent with a new playlist was taken (true when there was none).
    pub cover_saved: bool,
}

fn saved(playlist: Playlist, change: PlaylistChange) -> Saved {
    Saved {
        playlist,
        change,
        cover_saved: true,
    }
}

/// Applies a change to a track list. `None` when there is nothing to do.
fn edit_tracks(mut ids: Vec<u64>, edit: &Edit) -> Option<Vec<u64>> {
    match *edit {
        Edit::Add(track, at) if !ids.contains(&track.0) => {
            let at = at.unwrap_or(ids.len()).min(ids.len());
            ids.insert(at, track.0);
        }
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

/// Reads an image file off the core's thread and sends it as the cover.
/// Answers whether SoundCloud took it.
async fn upload_cover<A: SoundCloudApi>(api: &A, id: u64, file: PathBuf) -> bool {
    let bytes = match tokio::task::spawn_blocking(move || std::fs::read(file)).await {
        Ok(Ok(bytes)) => bytes,
        _ => return false,
    };
    match api.set_playlist_artwork(id, &bytes).await {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "the playlist cover was not saved");
            false
        }
    }
}

/// A text field: `None` when only spaces were typed.
fn text(value: &str) -> Option<String> {
    Some(value.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Sends one change to SoundCloud. Answers the playlist as it is after it,
/// and what changed (`AlreadyThere` when the track was in it already).
async fn send<A: SoundCloudApi>(
    api: &A,
    id: Option<PlaylistId>,
    edit: Edit,
) -> sc_api::Result<Saved> {
    let id = id.map(|p| p.0).unwrap_or_default();
    match edit {
        Edit::Create(new) => {
            let edit = PlaylistEdit {
                title: Some(new.title.trim().to_owned()),
                description: text(&new.description),
                sharing: Some(sharing(new.public).to_owned()),
                genre: text(&new.genre),
                tag_list: text(&new.tag_list()),
                tracks: Some(new.track.into_iter().map(|t| t.0).collect()),
            };
            let playlist = api.create_playlist(&edit).await?;
            let Some(cover) = new.cover else {
                return Ok(saved(playlist, PlaylistChange::Created));
            };
            let cover_saved = upload_cover(api, playlist.id, cover).await;
            // The answer to the creation has no cover yet.
            let playlist = api.playlist(playlist.id).await.unwrap_or(playlist);
            Ok(Saved {
                playlist,
                change: PlaylistChange::Created,
                cover_saved,
            })
        }
        Edit::Rename(title) => {
            let edit = PlaylistEdit {
                title: Some(title),
                ..PlaylistEdit::default()
            };
            Ok(saved(
                api.edit_playlist(id, &edit).await?,
                PlaylistChange::Renamed,
            ))
        }
        Edit::Describe(description) => {
            let edit = PlaylistEdit {
                description: Some(description.trim().to_owned()),
                ..PlaylistEdit::default()
            };
            Ok(saved(
                api.edit_playlist(id, &edit).await?,
                PlaylistChange::Described,
            ))
        }
        Edit::Privacy(public) => {
            let edit = PlaylistEdit {
                sharing: Some(sharing(public).to_owned()),
                ..PlaylistEdit::default()
            };
            let change = PlaylistChange::Privacy { public };
            Ok(saved(api.edit_playlist(id, &edit).await?, change))
        }
        Edit::Cover(file) => {
            if !upload_cover(api, id, file).await {
                return Err(sc_api::Error::Status(400));
            }
            Ok(saved(api.playlist(id).await?, PlaylistChange::Cover))
        }
        Edit::Delete => {
            let playlist = api.playlist(id).await?;
            api.delete_playlist(id).await?;
            Ok(saved(playlist, PlaylistChange::Deleted))
        }
        Edit::Add(..) | Edit::Remove(_) | Edit::Move { .. } => {
            let current = api.playlist(id).await?;
            let ids = current.tracks.iter().map(|t| t.id).collect();
            let change = match edit {
                Edit::Add(track, _) => PlaylistChange::Added(track),
                Edit::Remove(_) => PlaylistChange::Removed,
                _ => PlaylistChange::Moved,
            };
            let Some(tracks) = edit_tracks(ids, &edit) else {
                let change = match change {
                    PlaylistChange::Added(track) => PlaylistChange::AlreadyThere(track),
                    change => change,
                };
                return Ok(saved(current, change));
            };
            let edit = PlaylistEdit {
                tracks: Some(tracks),
                ..PlaylistEdit::default()
            };
            Ok(saved(api.edit_playlist(id, &edit).await?, change))
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

    pub(super) fn playlist_edited(&mut self, result: sc_api::Result<Box<Saved>>) {
        let Saved {
            playlist,
            change,
            cover_saved,
        } = match result {
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
        if !cover_saved {
            self.emit(Event::Problem(Problem::PlaylistCoverNotSaved));
        }
        match change {
            PlaylistChange::Created | PlaylistChange::Deleted => self.reload_library(),
            // These show in the library's cards too.
            PlaylistChange::Renamed | PlaylistChange::Privacy { .. } | PlaylistChange::Cover => {
                self.reload_library();
                self.playlist_opened(playlist);
            }
            PlaylistChange::Added(_)
            | PlaylistChange::Removed
            | PlaylistChange::Moved
            | PlaylistChange::Described => {
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
            edit_tracks(ids.clone(), &Edit::Add(TrackId(4), None)),
            Some(vec![1, 2, 3, 4])
        );
        assert_eq!(
            edit_tracks(ids.clone(), &Edit::Add(TrackId(2), None)),
            None,
            "already there"
        );
        assert_eq!(
            edit_tracks(ids.clone(), &Edit::Add(TrackId(9), Some(1))),
            Some(vec![1, 9, 2, 3]),
            "back where it was"
        );
        assert_eq!(edit_tracks(ids.clone(), &Edit::Remove(0)), Some(vec![2, 3]));
        assert_eq!(edit_tracks(ids.clone(), &Edit::Remove(9)), None);
        assert_eq!(
            edit_tracks(ids.clone(), &Edit::Move { from: 0, to: 2 }),
            Some(vec![2, 3, 1])
        );
        assert_eq!(edit_tracks(ids, &Edit::Move { from: 1, to: 1 }), None);
    }

    #[test]
    fn tags_with_spaces_are_quoted() {
        let new = NewPlaylist {
            tags: vec!["deep".into(), " after hours ".into(), String::new()],
            ..NewPlaylist::default()
        };
        assert_eq!(new.tag_list(), "deep \"after hours\"");
    }
}
