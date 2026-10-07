//! The core against a fake SoundCloud API and a fake audio engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sc_api::models::{
    LibraryItem, Like, Page, Playlist, Resource, StreamItem, Track, User, UserSummary, Waveform,
};
use sc_api::{SoundCloudApi, StreamProtocol, StreamSource};
use sc_core::{
    ArtKey, Command, CoreConfig, CoreHandle, Event, ListId, ListItems, PlayState, PlaylistId,
    Problem, SearchKind, TrackId, UserId,
};

const TRACKS: ListId = ListId::Search {
    kind: SearchKind::Tracks,
};

/// Records calls; answers from canned data.
#[derive(Clone, Default)]
struct FakeApi {
    calls: Arc<Mutex<Vec<String>>>,
    /// Makes the next `next_page` call fail.
    fail_next_page: Arc<AtomicBool>,
    /// Makes every stream URL lookup fail.
    fail_streams: Arc<AtomicBool>,
    /// Makes `next_page` answer after 400 ms.
    slow_next_page: Arc<AtomicBool>,
    /// The token `set_oauth_token` was given; only `"good"` signs in.
    token: Arc<Mutex<Option<String>>>,
    /// Makes like and follow calls fail.
    fail_actions: Arc<AtomicBool>,
}

fn partial(id: u64) -> Track {
    Track {
        id,
        ..Track::default()
    }
}

fn owner() -> UserSummary {
    UserSummary {
        id: 7,
        username: "Ana".into(),
        ..UserSummary::default()
    }
}

fn track(id: u64, title: &str, policy: &str) -> Track {
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
    fn log(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
    fn calls(&self) -> Vec<String> {
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
        if self.fail_actions.load(Ordering::SeqCst) {
            return Err(sc_api::Error::Status(500));
        }
        Ok(())
    }

    async fn set_following(&self, user: u64, following: bool) -> sc_api::Result<()> {
        self.log(format!("follow {user} {following}"));
        if self.fail_actions.load(Ordering::SeqCst) {
            return Err(sc_api::Error::Status(500));
        }
        Ok(())
    }
}

fn config(root: &std::path::Path) -> CoreConfig {
    CoreConfig {
        cache_dir: root.to_path_buf(),
        data_dir: root.join("data"),
    }
}

struct Harness {
    core: CoreHandle,
    api: FakeApi,
    audio_commands: flume::Receiver<sc_audio::Command>,
    audio_events: flume::Sender<sc_audio::Event>,
    cache: std::path::PathBuf,
}

impl Harness {
    fn new(name: &str) -> Self {
        let api = FakeApi::default();
        let (audio_tx, audio_commands) = flume::unbounded();
        let (audio_events, audio_rx) = flume::unbounded();
        let cache =
            std::env::temp_dir().join(format!("cloudrs-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let core = sc_core::spawn(api.clone(), (audio_tx, audio_rx), config(&cache));
        Self {
            core,
            api,
            audio_commands,
            audio_events,
            cache,
        }
    }

    /// Waits for the first event matching `pick`.
    fn wait<T>(&self, mut pick: impl FnMut(Event) -> Option<T>) -> T {
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

    fn search(&self) -> Vec<sc_core::TrackSummary> {
        self.core.send(Command::Search("lights out".into()));
        self.list(TRACKS).0
    }

    /// Waits for the next page of `list`: its tracks, `append` and `has_more`.
    fn list(&self, list: ListId) -> (Vec<sc_core::TrackSummary>, bool, bool) {
        self.page(list, |items| match items {
            ListItems::Tracks(tracks) => Some(tracks),
            _ => None,
        })
    }

    fn page<T>(&self, wanted: ListId, rows: impl Fn(ListItems) -> Option<T>) -> (T, bool, bool) {
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

#[test]
fn debounces_typing_into_one_search() {
    let h = Harness::new("debounce");
    for query in ["l", "li", "lig", "lights out"] {
        h.core.send(Command::Search(query.into()));
        std::thread::sleep(Duration::from_millis(30));
    }
    let (tracks, _, has_more) = h.list(TRACKS);
    assert_eq!(tracks.len(), 2);
    assert!(tracks[1].preview_only);
    assert!(has_more);
    let searches: Vec<_> = h
        .api
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("search"))
        .collect();
    assert_eq!(searches, ["search lights out"]);
}

#[test]
fn an_empty_query_clears_the_results() {
    let h = Harness::new("clear");
    h.core.send(Command::Search("   ".into()));
    let (tracks, _, has_more) = h.list(TRACKS);
    assert!(tracks.is_empty());
    assert!(!has_more);
    assert!(h.api.calls().is_empty());
}

#[test]
fn loads_the_next_page() {
    let h = Harness::new("more");
    h.search();
    h.core.send(Command::LoadMore(TRACKS));
    let (tracks, append, _) = h.list(TRACKS);
    assert!(append);
    assert_eq!(tracks[0].title, "Three");
}

#[test]
fn plays_a_track_and_reports_playback() {
    let h = Harness::new("play");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) => Some(track),
        _ => None,
    });
    assert_eq!(now.title, "One");
    assert_eq!(now.artist, "Artist");

    let load = h
        .audio_commands
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(
        matches!(&load, sc_audio::Command::Load(source) if source.url.ends_with("/1.m3u8")
            && source.kind == sc_audio::SourceKind::Hls),
        "{load:?}"
    );
    let bars = h.wait(|e| match e {
        Event::Waveform { bars, .. } => Some(bars),
        _ => None,
    });
    assert_eq!(bars.len(), 160);

    h.audio_events
        .send(sc_audio::Event::State(sc_audio::PlaybackState::Playing))
        .unwrap();
    h.audio_events
        .send(sc_audio::Event::Position(Duration::from_secs(42)))
        .unwrap();
    let playback = h.wait(|e| match e {
        Event::Playback(p) if p.position == Duration::from_secs(42) => Some(p),
        _ => None,
    });
    assert_eq!(playback.state, PlayState::Playing);
    assert_eq!(playback.duration, Duration::from_secs(200));

    h.core.send(Command::TogglePlay);
    let pause = h
        .audio_commands
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(matches!(pause, sc_audio::Command::Pause));
}

#[test]
fn explains_preview_only_tracks() {
    let h = Harness::new("preview");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(2),
    });
    let problem = h.wait(|e| match e {
        Event::Problem(p) => Some(p),
        _ => None,
    });
    assert_eq!(problem, Problem::PreviewOnly);
    assert!(h.audio_commands.try_recv().is_err());
}

#[test]
fn plays_a_pasted_link_and_rejects_non_tracks() {
    let h = Harness::new("paste");
    h.core.send(Command::OpenUrl(
        "https://soundcloud.com/someone/pasted".into(),
    ));
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) => Some(track),
        _ => None,
    });
    assert_eq!(now.title, "Pasted");

    h.core
        .send(Command::OpenUrl("https://soundcloud.com/a-station".into()));
    let problem = h.wait(|e| match e {
        Event::Problem(p) => Some(p),
        _ => None,
    });
    assert_eq!(problem, Problem::UnsupportedLink);
}

#[test]
fn caches_artwork_on_disk() {
    let h = Harness::new("artwork");
    h.search();
    let path = h.wait(|e| match e {
        Event::Artwork {
            key: ArtKey::Track(TrackId(1)),
            path,
        } => Some(path),
        _ => None,
    });
    assert_eq!(std::fs::read(&path).unwrap(), b"jpeg");
    assert!(path.starts_with(&h.cache));
    let downloads = h
        .api
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("download") && c.contains("t300x300"))
        .count();
    assert_eq!(downloads, 2);
}

#[test]
fn reports_a_failed_search_and_a_failed_next_page() {
    let h = Harness::new("search-failed");
    h.core.send(Command::Search("fail".into()));
    let failed = h.wait(|e| match e {
        Event::ListFailed {
            list,
            append,
            problem,
        } => Some((list, append, problem)),
        _ => None,
    });
    assert_eq!(failed, (TRACKS, false, Problem::NotFound));

    h.search();
    h.api.fail_next_page.store(true, Ordering::SeqCst);
    h.core.send(Command::LoadMore(TRACKS));
    let failed = h.wait(|e| match e {
        Event::ListFailed {
            list,
            append,
            problem,
        } => Some((list, append, problem)),
        _ => None,
    });
    assert_eq!(failed, (TRACKS, true, Problem::NotFound));
}

fn queue_ids(snapshot: &sc_core::QueueSnapshot) -> Vec<u64> {
    snapshot.tracks.iter().map(|t| t.id.0).collect()
}

impl Harness {
    /// Waits for the first queue snapshot that satisfies `pick`.
    fn queue_where(
        &self,
        pick: impl Fn(&sc_core::QueueSnapshot) -> bool,
    ) -> sc_core::QueueSnapshot {
        self.wait(|e| match e {
            Event::Queue(q) if pick(&q) => Some(q),
            _ => None,
        })
    }
}

#[test]
fn playing_from_results_makes_them_the_queue() {
    let h = Harness::new("queue-context");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(2),
    });
    let queue = h.queue_where(|_| true);
    assert_eq!(queue_ids(&queue), [1, 2]);
    assert_eq!(queue.current, Some(1));
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) => Some(track),
        _ => None,
    });
    assert_eq!(now.id, TrackId(2));
}

#[test]
fn queue_commands_edit_the_queue() {
    let h = Harness::new("queue-edit");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.core.send(Command::AddToQueue(TrackId(1)));
    let queue = h.queue_where(|q| q.tracks.len() == 3);
    assert_eq!(queue_ids(&queue), [1, 1, 2]);

    h.core.send(Command::MoveInQueue { from: 2, to: 1 });
    let queue = h.queue_where(|q| queue_ids(q) == [1, 2, 1]);
    assert_eq!(queue.current, Some(0));

    h.core.send(Command::RemoveFromQueue(1));
    h.queue_where(|q| queue_ids(q) == [1, 1]);

    h.core.send(Command::SetShuffle(true));
    h.queue_where(|q| q.shuffle);
    h.core.send(Command::SetRepeat(sc_core::Repeat::All));
    h.queue_where(|q| q.repeat == sc_core::Repeat::All);

    h.core.send(Command::PlayQueueIndex(1));
    let queue = h.queue_where(|q| q.current == Some(1));
    assert_eq!(queue_ids(&queue), [1, 1]);
}

#[test]
fn previous_restarts_a_track_past_three_seconds() {
    let h = Harness::new("previous");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.audio_events
        .send(sc_audio::Event::Position(Duration::from_secs(10)))
        .unwrap();
    h.wait(|e| match e {
        Event::Playback(p) if p.position == Duration::from_secs(10) => Some(()),
        _ => None,
    });
    h.core.send(Command::Previous);
    let seek = h
        .audio_commands
        .iter()
        .find_map(|c| match c {
            sc_audio::Command::Seek(at) => Some(at),
            _ => None,
        })
        .unwrap();
    assert_eq!(seek, Duration::ZERO);
}

#[test]
fn the_next_track_plays_when_one_ends() {
    let h = Harness::new("track-end");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.wait(|e| matches!(e, Event::NowPlaying(_)).then_some(()));
    h.audio_events
        .send(sc_audio::Event::State(sc_audio::PlaybackState::Ended))
        .unwrap();
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) if track.id == TrackId(2) => Some(track),
        _ => None,
    });
    assert_eq!(now.title, "Preview");
}

#[test]
fn autoplay_appends_related_tracks_when_the_queue_ends() {
    let h = Harness::new("autoplay");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(2),
    });
    h.wait(|e| matches!(e, Event::NowPlaying(_)).then_some(()));
    h.audio_events
        .send(sc_audio::Event::State(sc_audio::PlaybackState::Ended))
        .unwrap();
    let queue = h.queue_where(|q| q.tracks.len() == 4);
    assert_eq!(queue_ids(&queue), [1, 2, 90, 91]);
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) if track.id == TrackId(90) => Some(track),
        _ => None,
    });
    assert_eq!(now.title, "Related A");
    assert!(h.api.calls().contains(&"related 2".to_string()));
}

#[test]
fn repeat_one_replays_the_same_track() {
    let h = Harness::new("repeat-one");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.wait(|e| matches!(e, Event::NowPlaying(_)).then_some(()));
    h.core.send(Command::SetRepeat(sc_core::Repeat::One));
    h.queue_where(|q| q.repeat == sc_core::Repeat::One);
    h.audio_events
        .send(sc_audio::Event::State(sc_audio::PlaybackState::Ended))
        .unwrap();
    let again = h.wait(|e| match e {
        Event::NowPlaying(track) => Some(track),
        _ => None,
    });
    assert_eq!(again.id, TrackId(1));
}

/// The database opens off the actor loop; saves before that are skipped, so
/// tests that read the disk wait for the schema first.
fn wait_for_store(h: &Harness) {
    let db = h.cache.join("data/cloudrs.db");
    for _ in 0..50 {
        if let Ok(conn) = rusqlite::Connection::open(&db)
            && let Ok(version) =
                conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
            && version == 1
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(100));
}

/// Starts a second core on the same folders, as a restart would.
fn restart(h: &Harness) -> (CoreHandle, flume::Receiver<sc_audio::Command>) {
    let (audio_tx, audio_commands) = flume::unbounded();
    let (_audio_events, audio_rx) = flume::unbounded();
    let core = sc_core::spawn(h.api.clone(), (audio_tx, audio_rx), config(&h.cache));
    (core, audio_commands)
}

#[test]
fn restores_the_session_paused_and_resumes_from_the_saved_position() {
    let h = Harness::new("restore");
    wait_for_store(&h);
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.wait(|e| matches!(e, Event::NowPlaying(_)).then_some(()));
    h.audio_events
        .send(sc_audio::Event::Position(Duration::from_secs(42)))
        .unwrap();
    h.audio_events
        .send(sc_audio::Event::State(sc_audio::PlaybackState::Paused))
        .unwrap();

    // The write happens off the actor loop: restart until the latest has landed.
    let mut restored = None;
    for _ in 0..30 {
        let (core, audio) = restart(&h);
        let (mut queue, mut now, mut paused_at) = (None, None, None);
        while let Ok(event) = core.events().recv_timeout(Duration::from_millis(300)) {
            match event {
                Event::Queue(q) => queue = Some(q),
                Event::NowPlaying(track) => now = Some(track),
                Event::Playback(p) if p.state == PlayState::Paused => {
                    paused_at = Some(p.position);
                    break;
                }
                _ => {}
            }
        }
        if paused_at == Some(Duration::from_secs(42)) {
            restored = Some((core, audio, queue, now));
            break;
        }
    }
    let (core, audio, queue, now) = restored.expect("the session was saved");
    let queue = queue.unwrap();
    assert_eq!(queue_ids(&queue), [1, 2]);
    assert_eq!(queue.current, Some(0));
    assert_eq!(now.unwrap().id, TrackId(1));

    // The first play resolves the stream and starts at the saved position.
    core.send(Command::TogglePlay);
    let commands: Vec<_> = audio
        .iter()
        .filter(|c| !matches!(c, sc_audio::Command::SetVolume(_)))
        .take(2)
        .collect();
    assert!(matches!(commands[0], sc_audio::Command::Load(_)));
    assert!(matches!(
        commands[1],
        sc_audio::Command::Seek(at) if at == Duration::from_secs(42)
    ));
}

#[test]
fn records_a_track_in_the_history_after_thirty_seconds() {
    let h = Harness::new("history");
    wait_for_store(&h);
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.wait(|e| matches!(e, Event::NowPlaying(_)).then_some(()));
    for second in 1..=31 {
        h.audio_events
            .send(sc_audio::Event::Position(Duration::from_secs(second)))
            .unwrap();
    }
    h.wait(|e| match e {
        Event::Playback(p) if p.position == Duration::from_secs(31) => Some(()),
        _ => None,
    });

    let db = h.cache.join("data/cloudrs.db");
    let mut titles = Vec::new();
    for _ in 0..30 {
        if let Ok(conn) = rusqlite::Connection::open(&db)
            && let Ok(mut query) = conn.prepare("SELECT title FROM history")
            && let Ok(rows) = query.query_map([], |row| row.get::<_, String>(0))
        {
            titles = rows.filter_map(Result::ok).collect();
            if !titles.is_empty() {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(titles, ["One"]);
}

#[test]
fn next_on_the_last_track_plays_the_related_tracks() {
    let h = Harness::new("next-at-end");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(2),
    });
    h.wait(|e| matches!(e, Event::NowPlaying(_)).then_some(()));
    h.core.send(Command::Next);
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) if track.id == TrackId(90) => Some(track),
        _ => None,
    });
    assert_eq!(now.title, "Related A");
}

#[test]
fn a_track_that_cannot_play_is_skipped_when_moving_on() {
    let h = Harness::new("skip-failed");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.wait(|e| matches!(e, Event::NowPlaying(_)).then_some(()));
    // Track 2 is preview-only, so its stream fails.
    h.audio_events
        .send(sc_audio::Event::State(sc_audio::PlaybackState::Ended))
        .unwrap();
    let problem = h.wait(|e| match e {
        Event::Problem(p) => Some(p),
        _ => None,
    });
    assert_eq!(problem, Problem::PreviewOnly);
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) if track.id == TrackId(90) => Some(track),
        _ => None,
    });
    assert_eq!(now.title, "Related A");
}

#[test]
fn skipping_stops_after_a_full_pass_of_failures() {
    let h = Harness::new("skip-loop");
    h.api.fail_streams.store(true, Ordering::SeqCst);
    h.search();
    // Without the guard, repeat all would skip around the queue forever.
    h.core.send(Command::SetRepeat(sc_core::Repeat::All));
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.wait(|e| matches!(e, Event::Problem(_)).then_some(()));
    h.core.send(Command::Next);
    h.wait(|e| matches!(e, Event::Problem(_)).then_some(()));
    std::thread::sleep(Duration::from_millis(300));
    let streams = h
        .api
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("stream"))
        .count();
    assert_eq!(streams, 2);
}

#[test]
fn a_double_click_loads_the_track_once() {
    let h = Harness::new("double-click");
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    std::thread::sleep(Duration::from_millis(500));
    let loads = h
        .audio_commands
        .try_iter()
        .filter(|c| matches!(c, sc_audio::Command::Load(_)))
        .count();
    assert_eq!(loads, 1);
}

#[test]
fn the_volume_is_saved_once_the_slider_rests() {
    let h = Harness::new("volume");
    wait_for_store(&h);
    h.core.send(Command::SetVolume(0.9));
    h.core.send(Command::SetVolume(0.3));
    let db = h.cache.join("data/cloudrs.db");
    let mut saved = None;
    for _ in 0..60 {
        if let Ok(conn) = rusqlite::Connection::open(&db)
            && let Ok(volume) =
                conn.query_row("SELECT volume FROM session", [], |row| row.get::<_, f64>(0))
        {
            saved = Some(volume);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!((saved.expect("the session was saved") - 0.3).abs() < 1e-6);
}

#[test]
fn a_damaged_database_is_reset_and_reported() {
    let dir = std::env::temp_dir().join(format!("cloudrs-core-corrupt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(
        dir.join("data/cloudrs.db"),
        b"not a database at all, only text".repeat(50),
    )
    .unwrap();

    let (audio_tx, _audio_commands) = flume::unbounded();
    let (_audio_events, audio_rx) = flume::unbounded();
    let core = sc_core::spawn(FakeApi::default(), (audio_tx, audio_rx), config(&dir));
    let problem = loop {
        let event = core.events().recv_timeout(Duration::from_secs(10)).unwrap();
        if let Event::Problem(p) = event {
            break p;
        }
    };
    assert_eq!(problem, Problem::StorageReset);

    let aside = std::fs::read_dir(dir.join("data"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("cloudrs.db.corrupt-")
        });
    assert!(aside, "the damaged file was kept");
    let conn = rusqlite::Connection::open(dir.join("data/cloudrs.db")).unwrap();
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shutdown_saves_the_session_before_replying() {
    let h = Harness::new("shutdown");
    wait_for_store(&h);
    // The debounced volume save has not fired yet: only the final save can
    // have written it by the time Stopped arrives.
    h.core.send(Command::SetVolume(0.4));
    h.core.send(Command::Shutdown);
    h.wait(|e| matches!(e, Event::Stopped).then_some(()));

    let conn = rusqlite::Connection::open(h.cache.join("data/cloudrs.db")).unwrap();
    let volume: f64 = conn
        .query_row("SELECT volume FROM session", [], |row| row.get(0))
        .unwrap();
    assert!((volume - 0.4).abs() < 1e-6);
}

fn users(items: ListItems) -> Option<Vec<sc_core::UserSummary>> {
    match items {
        ListItems::Users(users) => Some(users),
        _ => None,
    }
}

fn playlists(items: ListItems) -> Option<Vec<sc_core::PlaylistSummary>> {
    match items {
        ListItems::Playlists(playlists) => Some(playlists),
        _ => None,
    }
}

#[test]
fn search_tabs_search_the_same_query_for_each_kind() {
    let h = Harness::new("tabs");
    h.search();

    h.core.send(Command::SetSearchKind(SearchKind::People));
    let (query, kind) = h.wait(|e| match e {
        Event::Searching { query, kind } => Some((query, kind)),
        _ => None,
    });
    assert_eq!((query.as_str(), kind), ("lights out", SearchKind::People));
    let people = ListId::Search {
        kind: SearchKind::People,
    };
    let (found, append, has_more) = h.page(people, users);
    assert_eq!(found[0].username, "Ana");
    assert_eq!(found[0].followers, Some(12));
    assert!(found[0].verified);
    assert!(!append && !has_more);
    h.wait(|e| match e {
        Event::Artwork {
            key: ArtKey::User(UserId(50)),
            path,
        } => Some(path),
        _ => None,
    });

    h.core.send(Command::SetSearchKind(SearchKind::Albums));
    let albums = ListId::Search {
        kind: SearchKind::Albums,
    };
    let (found, _, _) = h.page(albums, playlists);
    assert!(found[0].is_album);
    assert_eq!(found[0].owner, "Ana");

    h.core.send(Command::SetSearchKind(SearchKind::Playlists));
    let playlist_tab = ListId::Search {
        kind: SearchKind::Playlists,
    };
    let (found, _, _) = h.page(playlist_tab, playlists);
    assert!(!found[0].is_album);

    let calls = h.api.calls();
    for expected in [
        "search_users lights out",
        "search_albums lights out",
        "search_playlists lights out",
    ] {
        assert!(
            calls.contains(&expected.to_owned()),
            "{expected}: {calls:?}"
        );
    }
}

#[test]
fn a_profile_pages_each_of_its_lists_on_its_own() {
    let h = Harness::new("profile");
    h.core.send(Command::OpenUser(UserId(7)));
    let page = h.wait(|e| match e {
        Event::UserPage(page) => Some(page),
        _ => None,
    });
    assert_eq!(page.username, "Ana");
    assert_eq!(page.full_name.as_deref(), Some("Ana Maria"));
    assert_eq!(page.city.as_deref(), Some("Lisbon"));
    assert_eq!((page.followers, page.followings), (Some(12), Some(3)));
    assert_eq!(page.track_count, Some(2));
    assert!(page.verified);

    let user_tracks = ListId::UserTracks(UserId(7));
    let (tracks, append, has_more) = h.list(user_tracks);
    assert_eq!(tracks.len(), 2);
    assert!(!append && has_more);
    // Playlists and likes wait for the UI.
    assert!(
        !h.api
            .calls()
            .iter()
            .any(|c| c.starts_with("user_playlists") || c.starts_with("user_likes"))
    );

    let user_playlists = ListId::UserPlaylists(UserId(7));
    h.core.send(Command::LoadMore(user_playlists));
    let (found, append, _) = h.page(user_playlists, playlists);
    assert!(!append);
    assert_eq!(found.len(), 1);

    let user_likes = ListId::UserLikes(UserId(7));
    h.core.send(Command::LoadMore(user_likes));
    let (liked, append, has_more) = h.list(user_likes);
    assert_eq!(liked.iter().map(|t| t.id.0).collect::<Vec<_>>(), [72]);
    assert!(!append && has_more);

    // Each list continues from its own next page.
    h.core.send(Command::LoadMore(user_likes));
    let (_, append, _) = h.list(user_likes);
    assert!(append);
    h.core.send(Command::LoadMore(user_tracks));
    let (_, append, _) = h.list(user_tracks);
    assert!(append);
    let nexts: Vec<_> = h
        .api
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("next"))
        .collect();
    assert_eq!(
        nexts,
        [
            "next https://api-v2.soundcloud.com/users/7/likes?o=1",
            "next https://api-v2.soundcloud.com/users/7/tracks?o=1",
        ]
    );

    h.core.send(Command::Play {
        list: user_tracks,
        track: TrackId(71),
    });
    let queue = h.queue_where(|_| true);
    assert_eq!(queue_ids(&queue), [70, 71, 3]);
    assert_eq!(queue.current, Some(1));
}

#[test]
fn a_new_search_drops_the_late_page_of_the_old_one() {
    let h = Harness::new("stale");
    h.search();
    h.api.slow_next_page.store(true, Ordering::SeqCst);
    h.core.send(Command::LoadMore(TRACKS));
    h.core.send(Command::Search("other".into()));
    let (_, append, _) = h.list(TRACKS);
    assert!(!append);

    let deadline = std::time::Instant::now() + Duration::from_millis(900);
    while let Ok(event) = h.core.events().recv_deadline(deadline) {
        assert!(
            !matches!(event, Event::List { append: true, .. }),
            "stale page delivered: {event:?}"
        );
    }
}

#[test]
fn opening_a_track_sends_its_page_waveform_and_related_tracks() {
    let h = Harness::new("track-page");
    h.core.send(Command::OpenTrack(TrackId(5)));
    let page = h.wait(|e| match e {
        Event::TrackPage(page) => Some(page),
        _ => None,
    });
    assert_eq!(page.track.id, TrackId(5));
    assert_eq!(page.track.artist_id, Some(UserId(7)));
    assert_eq!(page.description.as_deref(), Some("About this track"));
    assert_eq!(
        (page.plays, page.likes, page.comments),
        (Some(1_000), Some(50), Some(4))
    );
    assert_eq!(page.created_at.as_deref(), Some("2026-01-02T03:04:05Z"));
    assert_eq!(page.permalink, "https://soundcloud.com/artist/5");

    let bars = h.wait(|e| match e {
        Event::Waveform {
            track: TrackId(5),
            bars,
        } => Some(bars),
        _ => None,
    });
    assert_eq!(bars.len(), 160);
    let (related, append, has_more) = h.list(ListId::Related(TrackId(5)));
    assert_eq!(related.len(), 2);
    assert!(!append && !has_more);
}

#[test]
fn a_playlist_arrives_whole_with_its_partial_tracks_filled_in_batches() {
    let h = Harness::new("playlist");
    h.core.send(Command::OpenPlaylist(PlaylistId(5)));
    let page = h.wait(|e| match e {
        Event::PlaylistPage(page) => Some(page),
        _ => None,
    });
    assert_eq!(page.title, "Mix");
    assert_eq!(page.owner, "Ana");
    assert_eq!(page.owner_id, Some(UserId(7)));
    assert_eq!(page.track_count, 3);
    assert_eq!(page.duration, Duration::from_secs(600));
    assert!(page.is_album);
    let (tracks, append, has_more) = h.list(ListId::Playlist(PlaylistId(5)));
    assert_eq!(
        tracks.iter().map(|t| t.id.0).collect::<Vec<_>>(),
        [100, 101, 102]
    );
    assert_eq!(tracks[1].title, "Any");
    assert!(!append && !has_more);

    h.core.send(Command::OpenPlaylist(PlaylistId(6)));
    let list = ListId::Playlist(PlaylistId(6));
    let (tracks, _, _) = h.list(list);
    assert_eq!(tracks.len(), 120);
    let batches: Vec<_> = h
        .api
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("tracks "))
        .collect();
    assert_eq!(batches, ["tracks 2", "tracks 50", "tracks 50", "tracks 20"]);

    h.core.send(Command::Play {
        list,
        track: TrackId(1002),
    });
    let queue = h.queue_where(|_| true);
    assert_eq!(queue.tracks.len(), 120);
    assert_eq!(queue.current, Some(2));
}

#[test]
fn pasted_links_open_profiles_and_playlists() {
    let h = Harness::new("open-url");
    h.core
        .send(Command::OpenUrl("https://soundcloud.com/a-user".into()));
    let user = h.wait(|e| match e {
        Event::UserPage(page) => Some(page),
        _ => None,
    });
    assert_eq!(user.id, UserId(7));
    h.list(ListId::UserTracks(UserId(7)));

    h.core
        .send(Command::OpenUrl("https://soundcloud.com/a-set".into()));
    let playlist = h.wait(|e| match e {
        Event::PlaylistPage(page) => Some(page),
        _ => None,
    });
    assert_eq!(playlist.id, PlaylistId(5));
}

#[test]
fn the_history_lists_played_tracks_newest_first() {
    let h = Harness::new("history-list");
    wait_for_store(&h);
    h.search();
    h.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    h.wait(|e| matches!(e, Event::NowPlaying(_)).then_some(()));
    for second in 1..=31 {
        h.audio_events
            .send(sc_audio::Event::Position(Duration::from_secs(second)))
            .unwrap();
    }
    // The write is asynchronous: ask again until the play shows up.
    let mut rows = Vec::new();
    for _ in 0..30 {
        h.core.send(Command::OpenHistory);
        let (tracks, append, has_more) = h.list(ListId::History);
        assert!(!append && !has_more);
        rows = tracks;
        if !rows.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "One");
    assert_eq!(rows[0].duration, Duration::from_secs(200));

    h.core.send(Command::Play {
        list: ListId::History,
        track: TrackId(1),
    });
    let queue = h.queue_where(|q| queue_ids(q) == [1]);
    assert_eq!(queue.current, Some(0));
}
