//! The fake SoundCloud API and audio engine the core tests share.
//! Each test binary uses part of it.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sc_api::models::{
    LibraryItem, Like, Page, Playlist, Resource, Selection, SelectionItem, SelectionItems,
    StreamItem, SystemPlaylist, Track, User, UserSummary, Waveform,
};
use sc_api::{SoundCloudApi, StreamProtocol, StreamSource};
use sc_core::{Command, CoreConfig, CoreHandle, Event, ListId, ListItems, SearchKind};

pub const TRACKS: ListId = ListId::Search {
    kind: SearchKind::Tracks,
};

/// Records calls; answers from canned data.
#[derive(Clone, Default)]
pub struct FakeApi {
    pub calls: Arc<Mutex<Vec<String>>>,
    /// Makes the next `next_page` call fail.
    pub fail_next_page: Arc<AtomicBool>,
    /// Makes every stream URL lookup fail.
    pub fail_streams: Arc<AtomicBool>,
    /// Makes `next_page` answer after 400 ms.
    pub slow_next_page: Arc<AtomicBool>,
    /// The token `set_oauth_token` was given; only `"good"` signs in.
    pub token: Arc<Mutex<Option<String>>>,
    /// Makes like and follow calls fail (`Status(500)`).
    pub fail_actions: Arc<AtomicBool>,
    /// Makes like and follow calls answer `Unauthorized` (an expired token).
    pub expired: Arc<AtomicBool>,
}

pub fn partial(id: u64) -> Track {
    Track {
        id,
        ..Track::default()
    }
}

pub fn owner() -> UserSummary {
    UserSummary {
        id: 7,
        username: "Ana".into(),
        ..UserSummary::default()
    }
}

pub fn track(id: u64, title: &str, policy: &str) -> Track {
    Track {
        id,
        title: title.into(),
        duration: 200_000,
        policy: Some(policy.into()),
        artwork_url: Some(format!("https://i1.sndcdn.com/artworks-{id}-large.jpg")),
        waveform_url: Some(format!("https://wave.sndcdn.com/{id}_m.json")),
        user: Some(UserSummary {
            id: 7,
            username: "Artist".into(),
            ..UserSummary::default()
        }),
        ..Track::default()
    }
}

impl FakeApi {
    pub fn log(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl SoundCloudApi for FakeApi {
    async fn search_tracks(&self, query: &str, _limit: u32) -> sc_api::Result<Page<Track>> {
        self.log(format!("search {query}"));
        if query == "fail" {
            return Err(sc_api::Error::NotFound);
        }
        Ok(Page {
            collection: vec![track(1, "One", "ALLOW"), track(2, "Preview", "SNIP")],
            next_href: Some("https://api-v2.soundcloud.com/next".into()),
            total_results: Some(3),
        })
    }

    async fn search_users(&self, query: &str, _limit: u32) -> sc_api::Result<Page<User>> {
        self.log(format!("search_users {query}"));
        let user = self.user(50).await?;
        Ok(Page {
            collection: vec![user],
            next_href: None,
            total_results: None,
        })
    }

    async fn search_playlists(&self, query: &str, _limit: u32) -> sc_api::Result<Page<Playlist>> {
        self.log(format!("search_playlists {query}"));
        let mut playlist = self.playlist(5).await?;
        playlist.is_album = Some(false);
        Ok(Page {
            collection: vec![playlist],
            next_href: None,
            total_results: None,
        })
    }

    async fn search_albums(&self, query: &str, _limit: u32) -> sc_api::Result<Page<Playlist>> {
        self.log(format!("search_albums {query}"));
        let mut album = self.playlist(5).await?;
        album.is_album = Some(true);
        Ok(Page {
            collection: vec![album],
            next_href: None,
            total_results: None,
        })
    }

    async fn user(&self, id: u64) -> sc_api::Result<User> {
        self.log(format!("user {id}"));
        Ok(User {
            id,
            username: "Ana".into(),
            full_name: Some("Ana Maria".into()),
            city: Some("Lisbon".into()),
            followers_count: Some(12),
            followings_count: Some(3),
            track_count: Some(2),
            verified: Some(true),
            avatar_url: Some(format!("https://i1.sndcdn.com/avatars-{id}-large.jpg")),
            ..User::default()
        })
    }

    async fn user_tracks(&self, id: u64, _limit: u32) -> sc_api::Result<Page<Track>> {
        self.log(format!("user_tracks {id}"));
        Ok(Page {
            collection: vec![track(70, "U One", "ALLOW"), track(71, "U Two", "ALLOW")],
            next_href: Some(format!(
                "https://api-v2.soundcloud.com/users/{id}/tracks?o=1"
            )),
            total_results: None,
        })
    }

    async fn user_playlists(&self, id: u64, _limit: u32) -> sc_api::Result<Page<Playlist>> {
        self.log(format!("user_playlists {id}"));
        Ok(Page {
            collection: vec![self.playlist(5).await?],
            next_href: Some(format!(
                "https://api-v2.soundcloud.com/users/{id}/playlists?o=1"
            )),
            total_results: None,
        })
    }

    async fn user_likes(&self, id: u64, _limit: u32) -> sc_api::Result<Page<Like>> {
        self.log(format!("user_likes {id}"));
        Ok(Page {
            collection: vec![
                Like {
                    track: Some(track(72, "Liked", "ALLOW")),
                },
                Like { track: None },
            ],
            next_href: Some(format!(
                "https://api-v2.soundcloud.com/users/{id}/likes?o=1"
            )),
            total_results: None,
        })
    }

    async fn playlist(&self, id: u64) -> sc_api::Result<Playlist> {
        self.log(format!("playlist {id}"));
        let tracks = if id == 6 {
            (1000..1120).map(partial).collect()
        } else {
            vec![track(100, "Full", "ALLOW"), partial(101), partial(102)]
        };
        Ok(Playlist {
            id,
            title: "Mix".into(),
            duration: 600_000,
            track_count: Some(tracks.len() as u64),
            is_album: Some(id == 5),
            user: Some(owner()),
            tracks,
            ..Playlist::default()
        })
    }

    async fn related(&self, id: u64, _limit: u32) -> sc_api::Result<Page<Track>> {
        self.log(format!("related {id}"));
        Ok(Page {
            collection: vec![
                track(90, "Related A", "ALLOW"),
                track(91, "Related B", "ALLOW"),
            ],
            next_href: None,
            total_results: None,
        })
    }

    async fn next_page<T>(&self, page: &Page<T>) -> sc_api::Result<Option<Page<T>>>
    where
        T: serde::de::DeserializeOwned + Send + Sync,
    {
        self.log(format!("next {}", page.next_href.as_deref().unwrap_or("")));
        if self.slow_next_page.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        if self.fail_next_page.load(Ordering::SeqCst) {
            return Err(sc_api::Error::NotFound);
        }
        // Reads as a track, a user, a playlist and a like at once.
        let page = serde_json::json!({
            "collection": [{ "id": 3, "title": "Three", "track": { "id": 3, "title": "Three" } }]
        });
        Ok(Some(serde_json::from_value(page).unwrap()))
    }

    async fn resolve(&self, url: &str) -> sc_api::Result<Resource> {
        self.log(format!("resolve {url}"));
        if url.ends_with("/a-station") {
            return Ok(Resource::Unknown);
        }
        if url.ends_with("/a-user") {
            return Ok(Resource::User(Box::new(self.user(7).await?)));
        }
        if url.ends_with("/a-set") {
            return Ok(Resource::Playlist(Box::new(self.playlist(5).await?)));
        }
        Ok(Resource::Track(Box::new(track(9, "Pasted", "ALLOW"))))
    }

    async fn track(&self, id: u64) -> sc_api::Result<Track> {
        Ok(Track {
            description: Some("About this track".into()),
            playback_count: Some(1_000),
            likes_count: Some(50),
            comment_count: Some(4),
            created_at: Some("2026-01-02T03:04:05Z".into()),
            permalink_url: format!("https://soundcloud.com/artist/{id}"),
            ..track(id, "Any", "ALLOW")
        })
    }

    async fn tracks(&self, ids: &[u64]) -> sc_api::Result<Vec<Track>> {
        self.log(format!("tracks {}", ids.len()));
        Ok(ids.iter().map(|id| track(*id, "Any", "ALLOW")).collect())
    }

    async fn stream_url(&self, track: &Track) -> sc_api::Result<StreamSource> {
        self.log(format!("stream {}", track.id));
        if self.fail_streams.load(Ordering::SeqCst) {
            return Err(sc_api::Error::NoPlayableStream("no supported format"));
        }
        if track.is_preview_only() {
            return Err(sc_api::Error::NoPlayableStream(
                "only a preview is available",
            ));
        }
        Ok(StreamSource {
            url: format!("https://cf-hls-media.sndcdn.com/playlist/{}.m3u8", track.id),
            protocol: StreamProtocol::Hls,
            mime_type: "audio/mp4".into(),
        })
    }

    async fn waveform(&self, _url: &str) -> sc_api::Result<Waveform> {
        Ok(Waveform {
            width: 4,
            height: 10,
            samples: vec![0, 5, 10, 5],
        })
    }

    async fn download(&self, url: &str) -> sc_api::Result<Vec<u8>> {
        self.log(format!("download {url}"));
        Ok(b"jpeg".to_vec())
    }

    fn set_oauth_token(&self, token: Option<String>) {
        self.log(format!("token {}", token.as_deref().unwrap_or("-")));
        *self.token.lock().unwrap() = token;
    }

    async fn me(&self) -> sc_api::Result<User> {
        self.log("me".into());
        if self.token.lock().unwrap().as_deref() != Some("good") {
            return Err(sc_api::Error::Unauthorized);
        }
        Ok(User {
            id: 9,
            username: "Me".into(),
            avatar_url: Some("https://i1.sndcdn.com/avatars-9-large.jpg".into()),
            ..User::default()
        })
    }

    async fn feed(&self, _limit: u32) -> sc_api::Result<Page<StreamItem>> {
        self.log("feed".into());
        Ok(Page {
            collection: vec![
                StreamItem {
                    kind: "track-repost".into(),
                    track: Some(track(80, "Fed", "ALLOW")),
                    playlist: None,
                },
                StreamItem {
                    kind: "playlist".into(),
                    track: None,
                    playlist: Some(self.playlist(5).await?),
                },
            ],
            next_href: Some("https://api-v2.soundcloud.com/stream?o=1".into()),
            total_results: None,
        })
    }

    async fn library(&self, _limit: u32) -> sc_api::Result<Page<LibraryItem>> {
        self.log("library".into());
        Ok(Page {
            collection: vec![
                LibraryItem {
                    kind: "playlist-like".into(),
                    playlist: Some(self.playlist(5).await?),
                },
                LibraryItem {
                    kind: "system-playlist-like".into(),
                    playlist: None,
                },
            ],
            next_href: None,
            total_results: None,
        })
    }

    async fn followings(&self, user: u64, _limit: u32) -> sc_api::Result<Page<User>> {
        self.log(format!("followings {user}"));
        Ok(Page {
            collection: vec![self.user(50).await?],
            next_href: None,
            total_results: None,
        })
    }

    async fn liked_track_ids(&self) -> sc_api::Result<Vec<u64>> {
        self.log("liked_ids".into());
        Ok(vec![1, 72])
    }

    async fn followed_user_ids(&self) -> sc_api::Result<Vec<u64>> {
        self.log("followed_ids".into());
        Ok(vec![50])
    }

    async fn set_track_like(&self, me: u64, track: u64, liked: bool) -> sc_api::Result<()> {
        self.log(format!("like {me} {track} {liked}"));
        if self.expired.load(Ordering::SeqCst) {
            return Err(sc_api::Error::Unauthorized);
        }
        if self.fail_actions.load(Ordering::SeqCst) {
            return Err(sc_api::Error::Status(500));
        }
        Ok(())
    }

    async fn mixed_selections(&self) -> sc_api::Result<Page<Selection>> {
        self.log("mixed_selections".into());
        let playlists = |ids: &[u64]| SelectionItems {
            collection: ids
                .iter()
                .map(|id| {
                    SelectionItem::Playlist(Box::new(Playlist {
                        id: *id,
                        title: format!("Curated {id}"),
                        ..Playlist::default()
                    }))
                })
                .collect(),
        };
        Ok(Page {
            collection: vec![
                Selection {
                    title: "Artists to watch out for".into(),
                    items: playlists(&[11, 12, 13]),
                    ..Selection::default()
                },
                // Too short a row: left out.
                Selection {
                    title: "Tiny".into(),
                    items: playlists(&[14]),
                    ..Selection::default()
                },
                // System playlists only: left out.
                Selection {
                    title: "Trending by genre".into(),
                    items: SelectionItems {
                        collection: vec![SelectionItem::SystemPlaylist(Box::default())],
                    },
                    ..Selection::default()
                },
            ],
            next_href: None,
            total_results: None,
        })
    }

    async fn chart_selections(&self) -> sc_api::Result<Page<Selection>> {
        self.log("chart_selections".into());
        Err(sc_api::Error::NotFound)
    }

    async fn system_playlist(&self, urn: &str) -> sc_api::Result<SystemPlaylist> {
        self.log(format!("system_playlist {urn}"));
        Ok(SystemPlaylist {
            urn: urn.into(),
            tracks: [30, 10, 20].into_iter().map(partial).collect(),
            ..SystemPlaylist::default()
        })
    }
    async fn set_following(&self, user: u64, following: bool) -> sc_api::Result<()> {
        self.log(format!("follow {user} {following}"));
        if self.expired.load(Ordering::SeqCst) {
            return Err(sc_api::Error::Unauthorized);
        }
        if self.fail_actions.load(Ordering::SeqCst) {
            return Err(sc_api::Error::Status(500));
        }
        Ok(())
    }
}

pub fn config(root: &std::path::Path, token: Option<&str>) -> CoreConfig {
    CoreConfig {
        cache_dir: root.to_path_buf(),
        data_dir: root.join("data"),
        oauth_token: token.map(str::to_owned),
        jam_network: sc_core::JamNetwork::Local,
    }
}

pub struct Harness {
    pub core: CoreHandle,
    pub api: FakeApi,
    pub audio_commands: flume::Receiver<sc_audio::Command>,
    pub audio_events: flume::Sender<sc_audio::Event>,
    pub cache: std::path::PathBuf,
}

impl Harness {
    pub fn new(name: &str) -> Self {
        Self::start(name, None)
    }

    /// Starts with a saved token the fake accepts and waits for the sign-in.
    pub fn signed_in(name: &str) -> Self {
        let h = Self::start(name, Some("good"));
        h.wait(|e| matches!(e, Event::SignedIn(_)).then_some(()));
        h
    }

    pub fn start(name: &str, token: Option<&str>) -> Self {
        let api = FakeApi::default();
        let (audio_tx, audio_commands) = flume::unbounded();
        let (audio_events, audio_rx) = flume::unbounded();
        let cache =
            std::env::temp_dir().join(format!("cloudrs-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let core = sc_core::spawn(api.clone(), (audio_tx, audio_rx), config(&cache, token));
        Self {
            core,
            api,
            audio_commands,
            audio_events,
            cache,
        }
    }

    /// Waits for the first event matching `pick`.
    pub fn wait<T>(&self, mut pick: impl FnMut(Event) -> Option<T>) -> T {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let event = self
                .core
                .events()
                .recv_timeout(left)
                .expect("expected event did not arrive");
            if let Some(found) = pick(event) {
                return found;
            }
        }
    }

    pub fn search(&self) -> Vec<sc_core::TrackSummary> {
        self.core.send(Command::Search("lights out".into()));
        self.list(TRACKS).0
    }

    /// Waits for the next page of `list`: its tracks, `append` and `has_more`.
    pub fn list(&self, list: ListId) -> (Vec<sc_core::TrackSummary>, bool, bool) {
        self.page(list, |items| match items {
            ListItems::Tracks(tracks) => Some(tracks),
            _ => None,
        })
    }

    pub fn page<T>(
        &self,
        wanted: ListId,
        rows: impl Fn(ListItems) -> Option<T>,
    ) -> (T, bool, bool) {
        self.wait(|e| match e {
            Event::List {
                list,
                items,
                append,
                has_more,
            } if list == wanted => rows(items).map(|rows| (rows, append, has_more)),
            _ => None,
        })
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.cache);
    }
}
