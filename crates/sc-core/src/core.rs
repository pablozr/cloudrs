//! The core actor: one loop that owns all state and handles one input at a
//! time. Slow work (HTTP, disk) runs in spawned tasks that report back as
//! inputs, so state is never shared or locked.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sc_api::models::{Page, Resource, Track};
use sc_api::{SoundCloudApi, StreamProtocol, StreamSource};
use tokio::task::JoinHandle;

use crate::types::{PlayState, Playback, Problem, TrackId, TrackSummary};
use crate::{Command, CoreConfig, Event, artwork, waveform};

/// How long the search waits for more typing.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
/// Results per page.
const PAGE_SIZE: u32 = 30;

type AudioLink = (
    flume::Sender<sc_audio::Command>,
    flume::Receiver<sc_audio::Event>,
);

enum Input {
    Ui(Command),
    UiClosed,
    Audio(sc_audio::Event),
    SearchDone {
        generation: u64,
        append: bool,
        result: sc_api::Result<Page<Track>>,
    },
    Resolved(sc_api::Result<Resource>),
    StreamReady {
        track: TrackId,
        result: sc_api::Result<StreamSource>,
    },
    WaveformReady {
        track: TrackId,
        bars: Vec<f32>,
    },
    ArtworkReady {
        track: TrackId,
        path: PathBuf,
    },
}

pub(crate) fn run_on_thread<A: SoundCloudApi + 'static>(
    api: A,
    audio: AudioLink,
    config: CoreConfig,
    commands: flume::Receiver<Command>,
    events: flume::Sender<Event>,
) {
    std::thread::Builder::new()
        .name("cloudrs-core".into())
        .spawn(move || {
            runtime().block_on(run(api, audio, config, commands, events));
        })
        .expect("the core thread starts");
}

/// The real API client opens sockets on this runtime, so it needs the IO
/// driver as well as timers (without it every request panics).
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("the core runtime starts")
}

async fn run<A: SoundCloudApi + 'static>(
    api: A,
    (audio, audio_events): AudioLink,
    config: CoreConfig,
    commands: flume::Receiver<Command>,
    events: flume::Sender<Event>,
) {
    let (inputs, input_rx) = flume::unbounded();

    let ui = inputs.clone();
    tokio::spawn(async move {
        while let Ok(command) = commands.recv_async().await {
            let _ = ui.send(Input::Ui(command));
        }
        let _ = ui.send(Input::UiClosed);
    });
    let from_audio = inputs.clone();
    tokio::spawn(async move {
        while let Ok(event) = audio_events.recv_async().await {
            let _ = from_audio.send(Input::Audio(event));
        }
    });

    let mut core = Core {
        api: Arc::new(api),
        inputs,
        events,
        audio,
        artwork_dir: config.cache_dir.join("artwork"),
        tracks: HashMap::new(),
        query: String::new(),
        generation: 0,
        search: None,
        next_page: None,
        current: None,
        playback: Playback {
            volume: 1.0,
            ..Playback::default()
        },
        artwork_requested: HashSet::new(),
    };
    while let Ok(input) = input_rx.recv_async().await {
        if matches!(input, Input::UiClosed) {
            break;
        }
        core.handle(input);
    }
}

struct Core<A> {
    api: Arc<A>,
    /// For spawned tasks to report back.
    inputs: flume::Sender<Input>,
    events: flume::Sender<Event>,
    audio: flume::Sender<sc_audio::Command>,
    artwork_dir: PathBuf,
    /// Every track seen in this session, by id.
    tracks: HashMap<TrackId, Track>,
    /// The query the current results belong to.
    query: String,
    /// Bumped on every search; results from older searches are dropped.
    generation: u64,
    search: Option<JoinHandle<()>>,
    /// The last page received, kept for its `next_href`.
    next_page: Option<Page<Track>>,
    current: Option<TrackId>,
    playback: Playback,
    artwork_requested: HashSet<TrackId>,
}

impl<A: SoundCloudApi + 'static> Core<A> {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn handle(&mut self, input: Input) {
        match input {
            Input::Ui(command) => self.command(command),
            Input::UiClosed => {}
            Input::Audio(event) => self.audio_event(event),
            Input::SearchDone {
                generation,
                append,
                result,
            } => {
                if generation == self.generation {
                    self.search = None;
                    self.search_done(append, result);
                }
            }
            Input::Resolved(result) => match result {
                Ok(Resource::Track(track)) => {
                    let id = TrackId(track.id);
                    self.tracks.insert(id, *track);
                    self.play(id);
                }
                Ok(_) => self.emit(Event::Problem(Problem::NotATrack)),
                Err(error) => self.emit(Event::Problem(Problem::from_api(&error))),
            },
            Input::StreamReady { track, result } => {
                if self.current != Some(track) {
                    return;
                }
                match result {
                    Ok(stream) => {
                        let kind = match stream.protocol {
                            StreamProtocol::Hls => sc_audio::SourceKind::Hls,
                            StreamProtocol::Progressive => sc_audio::SourceKind::Progressive,
                        };
                        let source = sc_audio::Source {
                            url: stream.url,
                            kind,
                        };
                        let _ = self.audio.send(sc_audio::Command::Load(source));
                    }
                    Err(error) => {
                        self.set_state(PlayState::Idle);
                        self.emit(Event::Problem(Problem::from_api(&error)));
                    }
                }
            }
            Input::WaveformReady { track, bars } => {
                if self.current == Some(track) {
                    self.emit(Event::Waveform { track, bars });
                }
            }
            Input::ArtworkReady { track, path } => self.emit(Event::Artwork { track, path }),
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Search(query) => self.search(query.trim().to_owned()),
            Command::LoadMore => self.load_more(),
            Command::Play(id) => self.play(id),
            Command::PlayUrl(url) => {
                let api = Arc::clone(&self.api);
                let inputs = self.inputs.clone();
                tokio::spawn(async move {
                    let result = api.resolve(url.trim()).await;
                    let _ = inputs.send(Input::Resolved(result));
                });
            }
            Command::TogglePlay => match self.playback.state {
                PlayState::Playing => self.to_audio(sc_audio::Command::Pause),
                PlayState::Paused => self.to_audio(sc_audio::Command::Play),
                // The engine dropped the finished or failed track: start it again.
                PlayState::Ended | PlayState::Idle => {
                    if let Some(id) = self.current {
                        self.play(id);
                    }
                }
                PlayState::Loading => {}
            },
            Command::Seek(at) => self.to_audio(sc_audio::Command::Seek(at)),
            Command::SetVolume(volume) => {
                let volume = volume.clamp(0.0, 1.0);
                self.to_audio(sc_audio::Command::SetVolume(volume));
                self.playback.volume = volume;
                self.emit(Event::Playback(self.playback));
            }
        }
    }

    fn to_audio(&self, command: sc_audio::Command) {
        let _ = self.audio.send(command);
    }

    fn search(&mut self, query: String) {
        self.generation += 1;
        if let Some(task) = self.search.take() {
            task.abort();
        }
        self.next_page = None;
        self.query = query.clone();
        if query.is_empty() {
            self.emit(Event::Results {
                query,
                tracks: Vec::new(),
                append: false,
                has_more: false,
            });
            return;
        }
        let (api, inputs, events) = (
            Arc::clone(&self.api),
            self.inputs.clone(),
            self.events.clone(),
        );
        let generation = self.generation;
        self.search = Some(tokio::spawn(async move {
            tokio::time::sleep(SEARCH_DEBOUNCE).await;
            let _ = events.send(Event::Searching {
                query: query.clone(),
            });
            let result = api.search_tracks(&query, PAGE_SIZE).await;
            let _ = inputs.send(Input::SearchDone {
                generation,
                append: false,
                result,
            });
        }));
    }

    fn load_more(&mut self) {
        if self.search.is_some() {
            return;
        }
        let Some(page) = self.next_page.take() else {
            return;
        };
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        let generation = self.generation;
        self.search = Some(tokio::spawn(async move {
            let result = match api.next_page(&page).await {
                Ok(Some(next)) => Ok(next),
                Ok(None) => Ok(Page {
                    collection: Vec::new(),
                    next_href: None,
                    total_results: None,
                }),
                Err(error) => Err(error),
            };
            let _ = inputs.send(Input::SearchDone {
                generation,
                append: true,
                result,
            });
        }));
    }

    fn search_done(&mut self, append: bool, result: sc_api::Result<Page<Track>>) {
        let page = match result {
            Ok(page) => page,
            Err(error) => {
                self.emit(Event::SearchFailed {
                    query: self.query.clone(),
                    append,
                    problem: Problem::from_api(&error),
                });
                return;
            }
        };
        let summaries: Vec<TrackSummary> =
            page.collection.iter().map(TrackSummary::from_api).collect();
        let ids: Vec<TrackId> = summaries.iter().map(|t| t.id).collect();
        for track in &page.collection {
            self.tracks.insert(TrackId(track.id), track.clone());
        }
        let has_more = page.next_href.is_some();
        self.emit(Event::Results {
            query: self.query.clone(),
            tracks: summaries,
            append,
            has_more,
        });
        self.next_page = has_more.then(|| Page {
            collection: Vec::new(),
            next_href: page.next_href,
            total_results: page.total_results,
        });
        for id in ids {
            self.request_artwork(id);
        }
    }

    fn play(&mut self, id: TrackId) {
        let Some(track) = self.tracks.get(&id).cloned() else {
            return;
        };
        let summary = TrackSummary::from_api(&track);
        self.current = Some(id);
        self.playback.position = Duration::ZERO;
        self.playback.duration = summary.duration;
        self.emit(Event::NowPlaying(summary));
        self.set_state(PlayState::Loading);
        self.request_artwork(id);

        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        let waveform_url = track.waveform_url.clone();
        tokio::spawn(async move {
            let result = api.stream_url(&track).await;
            let _ = inputs.send(Input::StreamReady { track: id, result });
            if let Some(url) = waveform_url
                && let Ok(wave) = api.waveform(&url).await
            {
                let bars = waveform::to_bars(&wave.samples, wave.height, waveform::BARS);
                let _ = inputs.send(Input::WaveformReady { track: id, bars });
            }
        });
    }

    fn request_artwork(&mut self, id: TrackId) {
        if !self.artwork_requested.insert(id) {
            return;
        }
        let Some(url) = self.tracks.get(&id).and_then(|t| t.artwork(artwork::SIZE)) else {
            return;
        };
        let path = artwork::path_for(&self.artwork_dir, &url);
        if path.exists() {
            self.emit(Event::Artwork { track: id, path });
            return;
        }
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            match api.download(&url).await {
                Ok(bytes) => match artwork::store(&path, &bytes) {
                    Ok(()) => {
                        let _ = inputs.send(Input::ArtworkReady { track: id, path });
                    }
                    Err(error) => tracing::warn!(%error, "could not cache artwork"),
                },
                Err(error) => tracing::debug!(%error, "artwork download failed"),
            }
        });
    }

    fn set_state(&mut self, state: PlayState) {
        self.playback.state = state;
        self.emit(Event::Playback(self.playback));
    }

    fn audio_event(&mut self, event: sc_audio::Event) {
        match event {
            sc_audio::Event::State(state) => self.set_state(match state {
                sc_audio::PlaybackState::Idle => PlayState::Idle,
                sc_audio::PlaybackState::Loading => PlayState::Loading,
                sc_audio::PlaybackState::Playing => PlayState::Playing,
                sc_audio::PlaybackState::Paused => PlayState::Paused,
                sc_audio::PlaybackState::Ended => PlayState::Ended,
            }),
            sc_audio::Event::Position(position) => {
                self.playback.position = position;
                self.emit(Event::Playback(self.playback));
            }
            sc_audio::Event::Error(detail) => {
                tracing::warn!(%detail, "audio error");
                self.emit(Event::Problem(Problem::Audio(detail)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::runtime;

    #[test]
    fn the_runtime_can_open_sockets() {
        runtime().block_on(async {
            tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("the IO driver is enabled");
        });
    }
}
