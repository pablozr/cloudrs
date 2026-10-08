//! Client for SoundCloud's internal `api-v2` (see `docs/adr/0002-soundcloud-api-v2.md`).
//!
//! The crate knows nothing about audio or UI: it finds a `client_id`, calls
//! the API, decodes the responses into [`models`] and resolves the URL of a
//! playable stream. Everything goes through the [`SoundCloudApi`] trait so a
//! backend for the official API can be added later.

mod base64;
mod client;
mod client_id;
mod error;
pub mod models;
mod stream;

pub use client::{ClientConfig, ScClient};
pub use client_id::{find_client_id, script_urls};
pub use error::{Error, Result};
pub use stream::{StreamProtocol, StreamSource, pick_transcoding};

use std::future::Future;

use models::{
    Comment, LibraryItem, Like, Page, Playlist, PlaylistEdit, Resource, Selection, StreamItem,
    SystemPlaylist, Track, User, Waveform,
};

/// What the rest of cloudrs may ask of SoundCloud.
pub trait SoundCloudApi: Send + Sync {
    /// Searches tracks. `limit` is capped by SoundCloud at 200.
    fn search_tracks(
        &self,
        query: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Page<Track>>> + Send;

    /// Searches people.
    fn search_users(
        &self,
        query: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Page<User>>> + Send;

    /// Searches playlists.
    fn search_playlists(
        &self,
        query: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Page<Playlist>>> + Send;

    /// Searches albums.
    fn search_albums(
        &self,
        query: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Page<Playlist>>> + Send;

    /// Fetches a user's profile.
    fn user(&self, id: u64) -> impl Future<Output = Result<User>> + Send;

    /// A user's own tracks.
    fn user_tracks(&self, id: u64, limit: u32) -> impl Future<Output = Result<Page<Track>>> + Send;

    /// A user's playlists and albums.
    fn user_playlists(
        &self,
        id: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Page<Playlist>>> + Send;

    /// A user's likes: tracks and playlists mixed (see [`Like`]).
    fn user_likes(&self, id: u64, limit: u32) -> impl Future<Output = Result<Page<Like>>> + Send;

    /// Fetches a playlist. Its tracks come partially: fill the ones that only
    /// carry an `id` with [`SoundCloudApi::tracks`].
    fn playlist(&self, id: u64) -> impl Future<Output = Result<Playlist>> + Send;

    /// Tracks SoundCloud suggests after `id` (autoplay). `limit` is capped at 200.
    fn related(&self, id: u64, limit: u32) -> impl Future<Output = Result<Page<Track>>> + Send;

    /// The first comments of a track (`/tracks/{id}/comments?threaded=0`), newest first.
    /// `limit` is capped at 200.
    fn comments(
        &self,
        track: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Page<Comment>>> + Send;

    /// Fetches the page that follows `page`, if there is one.
    fn next_page<T>(&self, page: &Page<T>) -> impl Future<Output = Result<Option<Page<T>>>> + Send
    where
        T: serde::de::DeserializeOwned + Send + Sync;

    /// Turns a `soundcloud.com` URL into the resource it points to.
    fn resolve(&self, url: &str) -> impl Future<Output = Result<Resource>> + Send;

    /// Fetches one track with its full metadata.
    fn track(&self, id: u64) -> impl Future<Output = Result<Track>> + Send;

    /// Fetches many tracks at once (SoundCloud accepts about 50 ids per call).
    fn tracks(&self, ids: &[u64]) -> impl Future<Output = Result<Vec<Track>>> + Send;

    /// Resolves the URL of the best stream the audio engine can play.
    fn stream_url(&self, track: &Track) -> impl Future<Output = Result<StreamSource>> + Send;

    /// Fetches the waveform of a track (`Track::waveform_url`).
    fn waveform(&self, url: &str) -> impl Future<Output = Result<Waveform>> + Send;

    /// Downloads a file from SoundCloud's CDN, such as artwork.
    fn download(&self, url: &str) -> impl Future<Output = Result<Vec<u8>>> + Send;

    /// Signs in (`Some`) or out (`None`): later requests carry this token.
    fn set_oauth_token(&self, token: Option<String>);

    /// The signed-in user. [`Error::Unauthorized`] when the token is refused.
    fn me(&self) -> impl Future<Output = Result<User>> + Send;

    /// The signed-in user's feed: posts and reposts of the people they follow.
    fn feed(&self, limit: u32) -> impl Future<Output = Result<Page<StreamItem>>> + Send;

    /// The signed-in user's playlists and albums, made or liked.
    fn library(&self, limit: u32) -> impl Future<Output = Result<Page<LibraryItem>>> + Send;

    /// The people a user follows.
    fn followings(&self, user: u64, limit: u32) -> impl Future<Output = Result<Page<User>>> + Send;

    /// Every track the signed-in user liked, by id.
    fn liked_track_ids(&self) -> impl Future<Output = Result<Vec<u64>>> + Send;

    /// Every user the signed-in user follows, by id.
    fn followed_user_ids(&self) -> impl Future<Output = Result<Vec<u64>>> + Send;

    /// Likes or unlikes a track as the signed-in user `me`.
    fn set_track_like(
        &self,
        me: u64,
        track: u64,
        liked: bool,
    ) -> impl Future<Output = Result<()>> + Send;

    /// SoundCloud's own home rows (`/mixed-selections`): curated playlists,
    /// personal ones when signed in.
    fn mixed_selections(&self) -> impl Future<Output = Result<Page<Selection>>> + Send;

    /// The genre charts (`/charts/selections`), as playlists.
    fn chart_selections(&self) -> impl Future<Output = Result<Page<Selection>>> + Send;

    /// A playlist SoundCloud makes, by urn. Its tracks only carry an id.
    fn system_playlist(&self, urn: &str) -> impl Future<Output = Result<SystemPlaylist>> + Send;

    /// Creates a playlist for the signed-in user from `playlist` (title,
    /// description, privacy, genre, tags, tracks). Answers the new playlist.
    fn create_playlist(
        &self,
        playlist: &PlaylistEdit,
    ) -> impl Future<Output = Result<Playlist>> + Send;

    /// Sets a playlist's cover from an image file's bytes (JPEG or PNG).
    fn set_playlist_artwork(
        &self,
        id: u64,
        image: &[u8],
    ) -> impl Future<Output = Result<()>> + Send;

    /// Changes a playlist of the signed-in user: only what `edit` sets.
    /// `tracks` replaces the whole list, so it must start from the current one.
    fn edit_playlist(
        &self,
        id: u64,
        edit: &PlaylistEdit,
    ) -> impl Future<Output = Result<Playlist>> + Send;

    /// Deletes a playlist of the signed-in user.
    fn delete_playlist(&self, id: u64) -> impl Future<Output = Result<()>> + Send;

    /// Follows or unfollows a user as the signed-in user.
    fn set_following(&self, user: u64, following: bool) -> impl Future<Output = Result<()>> + Send;
}
