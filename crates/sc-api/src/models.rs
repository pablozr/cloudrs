//! Response types for `api-v2`.
//!
//! SoundCloud changes these shapes without notice, so every field is optional
//! or defaulted: a missing field must never make a whole page fail to decode.

use serde::{Deserialize, Serialize};

/// One page of a paginated collection (`linked_partitioning=1`).
#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct Page<T> {
    #[serde(default = "Vec::new")]
    pub collection: Vec<T>,
    /// Absolute URL of the next page, without the `client_id`.
    #[serde(default)]
    pub next_href: Option<String>,
    /// Only present on search results.
    #[serde(default)]
    pub total_results: Option<u64>,
}

/// A track. Tracks embedded in playlists often carry only `id` and `kind`;
/// fetch them with `tracks(ids)` to fill in the rest.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub permalink_url: String,
    /// Playable duration in milliseconds (30 000 for GO+ previews).
    pub duration: u64,
    /// Duration of the full track in milliseconds.
    pub full_duration: Option<u64>,
    pub artwork_url: Option<String>,
    /// JSON with the waveform samples drawn in the player.
    pub waveform_url: Option<String>,
    pub genre: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<String>,
    pub playback_count: Option<u64>,
    pub likes_count: Option<u64>,
    pub comment_count: Option<u64>,
    /// `Some(false)` when the artist turned comments off.
    pub commentable: Option<bool>,
    pub streamable: Option<bool>,
    /// `ALLOW`, `MONETIZE`, `SNIP` (preview only) or `BLOCK`.
    pub policy: Option<String>,
    /// Sent back when resolving a stream URL.
    pub track_authorization: Option<String>,
    pub media: Media,
    pub user: Option<UserSummary>,
}

impl Track {
    /// Artwork URL at the requested size (`t500x500`, `t300x300`, `large`...).
    ///
    /// SoundCloud returns `-large` (100 px); other sizes share the same path.
    pub fn artwork(&self, size: &str) -> Option<String> {
        self.artwork_url
            .as_deref()
            .or(self
                .user
                .as_ref()
                .and_then(|user| user.avatar_url.as_deref()))
            .map(|url| resize(url, size))
    }

    /// Whether SoundCloud only allows a preview of this track.
    pub fn is_preview_only(&self) -> bool {
        self.policy.as_deref() == Some("SNIP")
    }
}

/// Swaps the `-large` size of a SoundCloud image URL for `size`.
fn resize(url: &str, size: &str) -> String {
    url.replace("-large.", &format!("-{size}."))
}

/// The audio renditions of a track.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Media {
    pub transcodings: Vec<Transcoding>,
}

/// One rendition. Its `url` resolves to the actual stream URL.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Transcoding {
    pub url: String,
    pub preset: String,
    pub duration: u64,
    /// `true` for 30-second previews.
    pub snipped: bool,
    pub format: Format,
    pub quality: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Format {
    /// `hls`, `progressive`, or an encrypted variant such as `ctr-encrypted-hls`.
    pub protocol: String,
    /// For example `audio/mp4; codecs="mp4a.40.2"` or `audio/mpeg`.
    pub mime_type: String,
}

/// The user fields embedded in tracks and playlists.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UserSummary {
    pub id: u64,
    pub username: String,
    pub permalink_url: String,
    pub avatar_url: Option<String>,
    pub verified: Option<bool>,
}

impl UserSummary {
    /// Avatar URL at the requested size.
    pub fn avatar(&self, size: &str) -> Option<String> {
        self.avatar_url.as_deref().map(|url| resize(url, size))
    }
}

/// A comment on a track.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Comment {
    pub id: u64,
    pub body: String,
    /// Position in the track, in milliseconds; `None` for a comment that is not timed.
    #[serde(rename = "timestamp")]
    pub timestamp_ms: Option<u64>,
    pub created_at: Option<String>,
    pub user: Option<UserSummary>,
}

/// A full user profile.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub full_name: Option<String>,
    pub permalink_url: String,
    pub avatar_url: Option<String>,
    pub description: Option<String>,
    pub city: Option<String>,
    pub followers_count: Option<u64>,
    pub followings_count: Option<u64>,
    pub track_count: Option<u64>,
    pub verified: Option<bool>,
}

impl User {
    /// Avatar URL at the requested size.
    pub fn avatar(&self, size: &str) -> Option<String> {
        self.avatar_url.as_deref().map(|url| resize(url, size))
    }
}

/// A playlist or an album (`is_album`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Playlist {
    pub id: u64,
    pub title: String,
    pub permalink_url: String,
    pub artwork_url: Option<String>,
    pub duration: u64,
    pub track_count: Option<u64>,
    pub is_album: Option<bool>,
    pub set_type: Option<String>,
    /// `public` or `private`.
    pub sharing: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub tag_list: Option<String>,
    pub user: Option<UserSummary>,
    /// The first tracks come complete; the rest only carry an `id`.
    pub tracks: Vec<Track>,
}

impl Playlist {
    /// Cover URL at the requested size: the playlist's own, else the first
    /// complete track's, else the owner's avatar.
    pub fn artwork(&self, size: &str) -> Option<String> {
        self.artwork_url
            .as_deref()
            .map(|url| resize(url, size))
            .or_else(|| self.tracks.iter().find_map(|track| track.artwork(size)))
            .or_else(|| {
                let user = self.user.as_ref()?;
                user.avatar_url.as_deref().map(|url| resize(url, size))
            })
    }
}

/// One entry of a user's likes. SoundCloud mixes tracks and playlists; only
/// `track` is set for a liked track.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Like {
    pub track: Option<Track>,
}

/// One entry of the signed-in user's feed (`/stream`): a post or a repost of
/// a track or a playlist.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StreamItem {
    /// `track`, `track-repost`, `playlist` or `playlist-repost`.
    #[serde(rename = "type")]
    pub kind: String,
    pub track: Option<Track>,
    pub playlist: Option<Playlist>,
}

/// One entry of the signed-in user's library (`/me/library/all`): a
/// playlist or album they made or liked. Other kinds have no `playlist`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LibraryItem {
    /// `playlist`, `playlist-like`, `system-playlist-like`...
    #[serde(rename = "type")]
    pub kind: String,
    pub playlist: Option<Playlist>,
}

/// A playlist SoundCloud makes (trending by genre, stations). Its tracks
/// carry only an `id`: fill them with `tracks(ids)`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemPlaylist {
    /// `soundcloud:system-playlists:trending-by-genre:house`...
    pub urn: String,
    pub title: String,
    pub short_title: Option<String>,
    pub artwork_url: Option<String>,
    pub calculated_artwork_url: Option<String>,
    pub tracks: Vec<Track>,
}

/// A row of SoundCloud's own home page ("Artists to watch out for",
/// "Curated by SoundCloud", the charts): a title over playlists.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Selection {
    pub urn: String,
    pub title: String,
    pub items: SelectionItems,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SelectionItems {
    pub collection: Vec<SelectionItem>,
}

/// What a selection holds. Kinds cloudrs does not show are `Other`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
pub enum SelectionItem {
    #[serde(rename = "playlist")]
    Playlist(Box<Playlist>),
    #[serde(rename = "system-playlist")]
    SystemPlaylist(Box<SystemPlaylist>),
    #[serde(other)]
    Other,
}

/// What to change on a playlist (`PUT /playlists/{id}`). Fields left `None`
/// are not sent, so they stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PlaylistEdit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `public` or `private`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    /// Tags separated by spaces; one with spaces goes in double quotes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_list: Option<String>,
    /// The whole track list, in order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracks: Option<Vec<u64>>,
}

/// SoundCloud's word for a playlist's privacy.
pub fn sharing(public: bool) -> &'static str {
    if public { "public" } else { "private" }
}
/// The waveform drawn in the player: one value per column, from 0 to `height`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Waveform {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<u32>,
}

/// What a `soundcloud.com` URL points to.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Resource {
    Track(Box<Track>),
    Playlist(Box<Playlist>),
    User(Box<User>),
    /// A kind cloudrs does not handle yet.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_search_page() {
        let page: Page<Track> =
            serde_json::from_str(include_str!("../tests/fixtures/search_tracks.json")).unwrap();
        assert_eq!(page.collection.len(), 2);
        assert_eq!(page.total_results, Some(812));
        assert!(page.next_href.is_some());
        let track = &page.collection[0];
        assert_eq!(track.title, "Lights Out (Extended Mix)");
        assert_eq!(track.media.transcodings.len(), 4);
        assert_eq!(track.user.as_ref().unwrap().username, "Charlotte de Witte");
    }

    #[test]
    fn decodes_comments() {
        let page: Page<Comment> =
            serde_json::from_str(include_str!("../tests/fixtures/track_comments.json")).unwrap();
        assert_eq!(page.collection.len(), 3);
        let timed = &page.collection[0];
        assert_eq!(timed.timestamp_ms, Some(10_000));
        assert_eq!(timed.body, "Drop incoming");
        let user = timed.user.as_ref().unwrap();
        assert_eq!(user.username, "ravefan");
        assert_eq!(
            user.avatar("t300x300").as_deref(),
            Some("https://i1.sndcdn.com/avatars-000111-t300x300.jpg")
        );
        assert_eq!(page.collection[2].timestamp_ms, None);
    }

    #[test]
    fn decodes_commentable() {
        let off: Track = serde_json::from_str(r#"{"id": 1, "commentable": false}"#).unwrap();
        assert_eq!(off.commentable, Some(false));
        let unknown: Track = serde_json::from_str(r#"{"id": 1}"#).unwrap();
        assert_eq!(unknown.commentable, None);
    }

    #[test]
    fn decodes_tracks_with_missing_fields() {
        let track: Track = serde_json::from_str(r#"{"id": 7, "kind": "track"}"#).unwrap();
        assert_eq!(track.id, 7);
        assert!(track.media.transcodings.is_empty());
    }

    #[test]
    fn resolves_by_kind() {
        let playlist: Resource = serde_json::from_str(
            r#"{"kind": "playlist", "id": 3, "title": "Sets", "tracks": [{"id": 1, "kind": "track"}]}"#,
        )
        .unwrap();
        assert!(matches!(playlist, Resource::Playlist(p) if p.tracks.len() == 1));
        let other: Resource = serde_json::from_str(r#"{"kind": "system-playlist"}"#).unwrap();
        assert!(matches!(other, Resource::Unknown));
    }

    #[test]
    fn likes_keep_only_tracks() {
        let page: Page<Like> = serde_json::from_str(
            r#"{"collection":[{"track":{"id":1}},{"playlist":{"id":2}}],"next_href":null}"#,
        )
        .unwrap();
        let tracks: Vec<u64> = page
            .collection
            .into_iter()
            .filter_map(|like| like.track)
            .map(|track| track.id)
            .collect();
        assert_eq!(tracks, [1]);
    }

    #[test]
    fn decodes_feed_and_library_entries() {
        let feed: Page<StreamItem> = serde_json::from_str(
            r#"{"collection":[{"type":"track-repost","track":{"id":1}},{"type":"playlist","playlist":{"id":2}}]}"#,
        )
        .unwrap();
        assert_eq!(feed.collection[0].kind, "track-repost");
        assert_eq!(feed.collection[0].track.as_ref().unwrap().id, 1);
        assert_eq!(feed.collection[1].playlist.as_ref().unwrap().id, 2);

        let library: Page<LibraryItem> = serde_json::from_str(
            r#"{"collection":[{"type":"playlist-like","playlist":{"id":3}},{"type":"system-playlist-like","system_playlist":{"id":"x"}}]}"#,
        )
        .unwrap();
        assert_eq!(library.collection[0].playlist.as_ref().unwrap().id, 3);
        assert!(library.collection[1].playlist.is_none());
    }

    #[test]
    fn playlist_artwork_falls_back_to_a_track() {
        let playlist: Playlist = serde_json::from_str(
            r#"{"id":1,"tracks":[{"id":5},{"id":6,"artwork_url":"https://i1/a-large.jpg"}]}"#,
        )
        .unwrap();
        assert_eq!(
            playlist.artwork("t300x300").as_deref(),
            Some("https://i1/a-t300x300.jpg")
        );
    }

    #[test]
    fn decodes_a_waveform() {
        let waveform: Waveform =
            serde_json::from_str(r#"{"width":4,"height":140,"samples":[0,70,140,35]}"#).unwrap();
        assert_eq!(waveform.samples, [0, 70, 140, 35]);
        assert_eq!(waveform.height, 140);
    }

    #[test]
    fn artwork_switches_size() {
        let track = Track {
            artwork_url: Some("https://i1.sndcdn.com/artworks-abc-large.jpg".into()),
            ..Track::default()
        };
        assert_eq!(
            track.artwork("t500x500").as_deref(),
            Some("https://i1.sndcdn.com/artworks-abc-t500x500.jpg")
        );
    }
}
