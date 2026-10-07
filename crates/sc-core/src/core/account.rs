//! Signing in and out, and the actions that need an account (ADR 0010).

use std::sync::Arc;

use sc_api::SoundCloudApi;
use sc_api::models::User;

use super::{Core, Input};
use crate::types::{Account, ArtKey, ListId, Problem, TrackId, UserId, UserSummary};
use crate::{Event, artwork};

impl<A: SoundCloudApi + 'static> Core<A> {
    /// Checks `token` with `/me`. Until the answer, requests carry the new
    /// token; a refused one puts the previous account's token back.
    pub(super) fn sign_in(&mut self, token: String) {
        let token = token.trim().to_owned();
        self.sign_in_gen += 1;
        let generation = self.sign_in_gen;
        self.api.set_oauth_token(Some(token.clone()));
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.me().await.map(Box::new);
            let _ = inputs.send(Input::SignInChecked {
                generation,
                token,
                result,
            });
        });
    }

    pub(super) fn sign_in_checked(
        &mut self,
        generation: u64,
        token: String,
        result: sc_api::Result<Box<User>>,
    ) {
        if generation != self.sign_in_gen {
            return;
        }
        let user = match result {
            Ok(user) => user,
            Err(error) => {
                let previous = self.account.as_ref().map(|a| a.token.clone());
                self.api.set_oauth_token(previous);
                self.emit(Event::Problem(match error {
                    sc_api::Error::Unauthorized => Problem::SignInFailed,
                    error => Problem::from_api(&error),
                }));
                return;
            }
        };
        let id = UserId(user.id);
        if let Some(url) = user.avatar(artwork::SIZE) {
            self.other_art.insert(ArtKey::User(id), url);
            self.request_artwork(ArtKey::User(id));
        }
        // Another person's lists must not stay behind a new sign-in.
        self.drop_account_lists();
        let account = Account {
            user: UserSummary::from_api(&user),
            token,
        };
        self.account = Some(account.clone());
        self.emit(Event::SignedIn(account));
        self.load_account_ids(generation);
    }

    /// Fetches the liked track ids and followed user ids, for hearts and
    /// follow buttons.
    fn load_account_ids(&self, generation: u64) {
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let liked = api.liked_track_ids().await;
            let followed = api.followed_user_ids().await;
            let _ = inputs.send(Input::AccountIds {
                generation,
                liked,
                followed,
            });
        });
    }

    pub(super) fn account_ids(
        &mut self,
        generation: u64,
        liked: sc_api::Result<Vec<u64>>,
        followed: sc_api::Result<Vec<u64>>,
    ) {
        if generation != self.sign_in_gen || self.account.is_none() {
            return;
        }
        match liked {
            Ok(ids) => self.emit(Event::LikedIds(ids.into_iter().map(TrackId).collect())),
            Err(error) => tracing::warn!(%error, "could not load the liked tracks"),
        }
        match followed {
            Ok(ids) => self.emit(Event::FollowedIds(ids.into_iter().map(UserId).collect())),
            Err(error) => tracing::warn!(%error, "could not load the followed people"),
        }
    }

    pub(super) fn sign_out(&mut self) {
        self.sign_in_gen += 1;
        self.api.set_oauth_token(None);
        self.drop_account_lists();
        if self.account.take().is_some() {
            self.emit(Event::SignedOut);
        }
    }

    /// A refused token while signed in means it expired or was revoked:
    /// sign out and say so. Returns whether that happened.
    pub(super) fn expired(&mut self, error: &sc_api::Error) -> bool {
        if matches!(error, sc_api::Error::Unauthorized) && self.account.is_some() {
            self.sign_out();
            self.emit(Event::Problem(Problem::SessionExpired));
            return true;
        }
        false
    }

    fn drop_account_lists(&mut self) {
        self.lists.remove(&ListId::Feed);
        self.lists.remove(&ListId::Library);
        if let Some(account) = &self.account {
            let me = account.user.id;
            self.lists.remove(&ListId::UserLikes(me));
            self.lists.remove(&ListId::Followings(me));
        }
    }

    pub(super) fn like(&mut self, track: TrackId, liked: bool) {
        let Some(account) = &self.account else {
            self.emit(Event::Problem(Problem::SignInRequired));
            return;
        };
        let me = account.user.id;
        self.emit(Event::Liked { track, liked });
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.set_track_like(me.0, track.0, liked).await;
            let _ = inputs.send(Input::LikeDone {
                track,
                liked,
                result,
            });
        });
    }

    pub(super) fn like_done(&mut self, track: TrackId, liked: bool, result: sc_api::Result<()>) {
        if let Err(error) = result {
            self.emit(Event::Liked {
                track,
                liked: !liked,
            });
            if !self.expired(&error) {
                self.emit(Event::Problem(Problem::from_api(&error)));
            }
        }
    }

    pub(super) fn follow(&mut self, user: UserId, following: bool) {
        if self.account.is_none() {
            self.emit(Event::Problem(Problem::SignInRequired));
            return;
        }
        self.emit(Event::Followed { user, following });
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.set_following(user.0, following).await;
            let _ = inputs.send(Input::FollowDone {
                user,
                following,
                result,
            });
        });
    }

    pub(super) fn follow_done(
        &mut self,
        user: UserId,
        following: bool,
        result: sc_api::Result<()>,
    ) {
        if let Err(error) = result {
            self.emit(Event::Followed {
                user,
                following: !following,
            });
            if !self.expired(&error) {
                self.emit(Event::Problem(Problem::from_api(&error)));
            }
        }
    }
}
