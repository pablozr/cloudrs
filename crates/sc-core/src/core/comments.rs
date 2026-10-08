//! A track's comments (ADR 0021).

use std::sync::Arc;

use sc_api::SoundCloudApi;
use sc_api::models::{Comment, Page};

use super::{Core, Input};
use crate::types::{ArtKey, CommentSummary, Problem, TrackId, UserId};
use crate::{Event, artwork};

/// The most comments one request brings; there is no "load more".
const COMMENTS_LIMIT: u32 = 200;

impl<A: SoundCloudApi + 'static> Core<A> {
    pub(super) fn load_comments(&mut self, id: TrackId) {
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.comments(id.0, COMMENTS_LIMIT).await;
            let _ = inputs.send(Input::CommentsDone { track: id, result });
        });
    }

    /// Keeps each commenter's avatar URL, without downloading it: the UI asks
    /// for the ones it shows with `Command::LoadArtwork`.
    pub(super) fn comments_done(&mut self, track: TrackId, result: sc_api::Result<Page<Comment>>) {
        match result {
            Ok(page) => {
                for user in page.collection.iter().filter_map(|c| c.user.as_ref()) {
                    if let Some(url) = user.avatar(artwork::SIZE) {
                        self.other_art
                            .entry(ArtKey::User(UserId(user.id)))
                            .or_insert(url);
                    }
                }
                let comments = page.collection.iter().map(CommentSummary::from_api);
                self.emit(Event::Comments {
                    track,
                    comments: comments.collect(),
                });
            }
            Err(error) => self.emit(Event::CommentsFailed {
                track,
                problem: Problem::from_api(&error),
            }),
        }
    }
}
