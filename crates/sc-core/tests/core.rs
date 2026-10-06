//! The core against a fake SoundCloud API and a fake audio engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sc_api::models::{Page, Resource, Track, UserSummary, Waveform};
use sc_api::{SoundCloudApi, StreamProtocol, StreamSource};
use sc_core::{Command, CoreConfig, CoreHandle, Event, PlayState, Problem, TrackId};

/// Records calls; answers from canned data.
#[derive(Clone, Default)]
struct FakeApi {
    calls: Arc<Mutex<Vec<String>>>,
    /// Makes the next `next_page` call fail.
    fail_next_page: Arc<AtomicBool>,
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

    async fn next_page<T>(&self, _page: &Page<T>) -> sc_api::Result<Option<Page<T>>>
    where
        T: serde::de::DeserializeOwned + Send + Sync,
    {
        self.log("next".into());
        if self.fail_next_page.load(Ordering::SeqCst) {
            return Err(sc_api::Error::NotFound);
        }
        let page = serde_json::json!({ "collection": [{ "id": 3, "title": "Three" }] });
        Ok(Some(serde_json::from_value(page).unwrap()))
    }

    async fn resolve(&self, url: &str) -> sc_api::Result<Resource> {
        self.log(format!("resolve {url}"));
        if url.ends_with("/a-user") {
            return Ok(Resource::Unknown);
        }
        Ok(Resource::Track(Box::new(track(9, "Pasted", "ALLOW"))))
    }

    async fn track(&self, id: u64) -> sc_api::Result<Track> {
        Ok(track(id, "Any", "ALLOW"))
    }

    async fn tracks(&self, ids: &[u64]) -> sc_api::Result<Vec<Track>> {
        Ok(ids.iter().map(|id| track(*id, "Any", "ALLOW")).collect())
    }

    async fn stream_url(&self, track: &Track) -> sc_api::Result<StreamSource> {
        self.log(format!("stream {}", track.id));
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
        let core = sc_core::spawn(
            api.clone(),
            (audio_tx, audio_rx),
            CoreConfig {
                cache_dir: cache.clone(),
            },
        );
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
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
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
        self.wait(|e| match e {
            Event::Results { tracks, .. } => Some(tracks),
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
    let (query, tracks, has_more) = h.wait(|e| match e {
        Event::Results {
            query,
            tracks,
            has_more,
            ..
        } => Some((query, tracks, has_more)),
        _ => None,
    });
    assert_eq!(query, "lights out");
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
    let tracks = h.wait(|e| match e {
        Event::Results { tracks, .. } => Some(tracks),
        _ => None,
    });
    assert!(tracks.is_empty());
    assert!(h.api.calls().is_empty());
}

#[test]
fn loads_the_next_page() {
    let h = Harness::new("more");
    h.search();
    h.core.send(Command::LoadMore);
    let (tracks, append) = h.wait(|e| match e {
        Event::Results { tracks, append, .. } => Some((tracks, append)),
        _ => None,
    });
    assert!(append);
    assert_eq!(tracks[0].title, "Three");
}

#[test]
fn plays_a_track_and_reports_playback() {
    let h = Harness::new("play");
    h.search();
    h.core.send(Command::Play(TrackId(1)));
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) => Some(track),
        _ => None,
    });
    assert_eq!(now.title, "One");
    assert_eq!(now.artist, "Artist");

    let load = h
        .audio_commands
        .recv_timeout(Duration::from_secs(3))
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
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(matches!(pause, sc_audio::Command::Pause));
}

#[test]
fn explains_preview_only_tracks() {
    let h = Harness::new("preview");
    h.search();
    h.core.send(Command::Play(TrackId(2)));
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
    h.core.send(Command::PlayUrl(
        "https://soundcloud.com/someone/pasted".into(),
    ));
    let now = h.wait(|e| match e {
        Event::NowPlaying(track) => Some(track),
        _ => None,
    });
    assert_eq!(now.title, "Pasted");

    h.core
        .send(Command::PlayUrl("https://soundcloud.com/a-user".into()));
    let problem = h.wait(|e| match e {
        Event::Problem(p) => Some(p),
        _ => None,
    });
    assert_eq!(problem, Problem::NotATrack);
}

#[test]
fn caches_artwork_on_disk() {
    let h = Harness::new("artwork");
    h.search();
    let path = h.wait(|e| match e {
        Event::Artwork {
            track: TrackId(1),
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
        Event::SearchFailed {
            query,
            append,
            problem,
        } => Some((query, append, problem)),
        _ => None,
    });
    assert_eq!(failed, ("fail".into(), false, Problem::NotFound));

    h.search();
    h.api.fail_next_page.store(true, Ordering::SeqCst);
    h.core.send(Command::LoadMore);
    let failed = h.wait(|e| match e {
        Event::SearchFailed {
            query,
            append,
            problem,
        } => Some((query, append, problem)),
        _ => None,
    });
    assert_eq!(failed, ("lights out".into(), true, Problem::NotFound));
}
