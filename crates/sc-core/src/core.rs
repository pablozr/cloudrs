//! The core actor: one loop that owns all state and handles one input at a
//! time. Slow work (HTTP, disk) runs in spawned tasks that report back as
//! inputs, so state is never shared or locked.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sc_api::models::{Page, Resource, Track};
use sc_api::{SoundCloudApi, StreamProtocol, StreamSource};
use tokio::task::JoinHandle;

use crate::listen::Listened;
use crate::queue::{Queue, Step};
use crate::store::{self, Session, SessionTrack};
use crate::types::{PlayState, Playback, Problem, TrackId, TrackSummary};
use crate::{Command, CoreConfig, Event, artwork, waveform};

/// How long the search waits for more typing.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
/// Results per page.
const PAGE_SIZE: u32 = 30;
/// Past this position "previous" restarts the track instead of going back.
const RESTART_AFTER: Duration = Duration::from_secs(3);
/// How often the session is saved while playing.
const SAVE_EVERY: Duration = Duration::from_secs(5);
/// How long after the last volume change the session is saved.
const VOLUME_SAVE_DELAY: Duration = Duration::from_secs(1);
/// Tracks fetched when the queue runs out.
const AUTOPLAY_COUNT: u32 = 20;

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
        generation: u64,
        start_at: Option<Duration>,
        result: sc_api::Result<StreamSource>,
    },
    /// A queued track that was not in this session's cache (a restored queue).
    TrackFetched {
        track: TrackId,
        generation: u64,
        start_at: Option<Duration>,
        result: sc_api::Result<Box<Track>>,
    },
    RelatedDone {
        generation: u64,
        result: sc_api::Result<Page<Track>>,
    },
    /// The debounced session save after a volume change.
    SaveDue,
    /// The database opened (or not) off the actor loop, with the saved session.
    StoreReady(Option<(SharedStore, Option<Session>)>),
    WaveformReady {
        track: TrackId,
        generation: u64,
        bars: Vec<f32>,
    },
    ArtworkReady {
        track: TrackId,
        path: PathBuf,
    },
}

/// The connection plus the sequence of the last session written, so a slow
/// older write never overwrites a newer one.
struct Store {
    conn: rusqlite::Connection,
    last_seq: u64,
}

type SharedStore = Arc<Mutex<Store>>;

/// Opens the database and loads the saved session. Runs on a blocking thread.
fn open_store(dir: &std::path::Path) -> Option<(SharedStore, Option<Session>)> {
    let opened = std::fs::create_dir_all(dir)
        .map_err(|error| error.to_string())
        .and_then(|()| store::open(&dir.join("cloudrs.db")).map_err(|error| error.to_string()));
    let conn = match opened {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(%error, "no session database; continuing without saving");
            return None;
        }
    };
    let session = store::load_session(&conn)
        .inspect_err(|error| tracing::warn!(%error, "could not read the saved session"))
        .ok()
        .flatten();
    Some((Arc::new(Mutex::new(Store { conn, last_seq: 0 })), session))
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

    let data_dir = config.data_dir.clone();
    let opened = inputs.clone();
    tokio::task::spawn_blocking(move || {
        let _ = opened.send(Input::StoreReady(open_store(&data_dir)));
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
        results: Vec::new(),
        queue: Queue::new(shuffle_seed()),
        autoplay: None,
        autoplay_gen: 0,
        play_gen: 0,
        skip_on_failure: false,
        failed_in_row: 0,
        volume_save: None,
        store: None,
        save_seq: 0,
        last_save: Instant::now(),
        listened: Listened::default(),
        restored_artwork: HashMap::new(),
        pending_restore: None,
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

/// Any value works as a shuffle seed; the clock is enough.
fn shuffle_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
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
    /// The tracks of the current search, in order: the context of `Play`.
    results: Vec<TrackSummary>,
    queue: Queue,
    /// Generation of the related-tracks request in flight, if any. A newer
    /// play cancels it, so a stale answer is ignored.
    autoplay: Option<u64>,
    autoplay_gen: u64,
    /// Bumped on every play; stream and waveform answers of an older play are
    /// dropped (a double click, repeat one or a quick Previous).
    play_gen: u64,
    /// The current track was reached by moving on, so if it cannot play the
    /// core moves on again. A track the person picked does not skip.
    skip_on_failure: bool,
    /// Tracks in a row that could not play; a full pass stops the skipping.
    failed_in_row: usize,
    volume_save: Option<JoinHandle<()>>,
    store: Option<SharedStore>,
    save_seq: u64,
    last_save: Instant,
    listened: Listened,
    /// Artwork URLs of a restored queue, whose tracks are not fetched yet.
    restored_artwork: HashMap<TrackId, String>,
    /// A restored track waits paused at this position; its stream is
    /// resolved on the first play.
    pending_restore: Option<Duration>,
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
                    let summary = TrackSummary::from_api(&track);
                    self.tracks.insert(id, *track);
                    self.queue.set_context(vec![summary], 0);
                    self.queue_changed();
                    self.play_current(None, false);
                }
                Ok(_) => self.emit(Event::Problem(Problem::NotATrack)),
                Err(error) => self.emit(Event::Problem(Problem::from_api(&error))),
            },
            Input::TrackFetched {
                track,
                generation,
                start_at,
                result,
            } => {
                if generation != self.play_gen {
                    return;
                }
                match result {
                    Ok(api_track) => {
                        self.tracks.insert(track, (*api_track).clone());
                        self.start_stream(*api_track, start_at);
                    }
                    Err(error) => self.play_failed(Problem::from_api(&error)),
                }
            }
            Input::RelatedDone { generation, result } => self.related_done(generation, result),
            Input::SaveDue => self.save_session(),
            Input::StoreReady(opened) => {
                let Some((store, session)) = opened else {
                    return;
                };
                self.store = Some(store);
                if let Some(session) = session {
                    self.restore(session);
                }
            }
            Input::StreamReady {
                generation,
                start_at,
                result,
            } => {
                if generation != self.play_gen {
                    return;
                }
                match result {
                    Ok(stream) => {
                        self.failed_in_row = 0;
                        let kind = match stream.protocol {
                            StreamProtocol::Hls => sc_audio::SourceKind::Hls,
                            StreamProtocol::Progressive => sc_audio::SourceKind::Progressive,
                        };
                        let source = sc_audio::Source {
                            url: stream.url,
                            kind,
                        };
                        let _ = self.audio.send(sc_audio::Command::Load(source));
                        if let Some(at) = start_at {
                            let _ = self.audio.send(sc_audio::Command::Seek(at));
                        }
                    }
                    Err(error) => self.play_failed(Problem::from_api(&error)),
                }
            }
            Input::WaveformReady {
                track,
                generation,
                bars,
            } => {
                if generation == self.play_gen {
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
            Command::Play(id) => self.play_from_results(id),
            Command::PlayUrl(url) => {
                let api = Arc::clone(&self.api);
                let inputs = self.inputs.clone();
                tokio::spawn(async move {
                    let result = api.resolve(url.trim()).await;
                    let _ = inputs.send(Input::Resolved(result));
                });
            }
            Command::Next => self.skip_forward(false),
            Command::Previous => {
                if self.playback.position > RESTART_AFTER {
                    self.seek(Duration::ZERO);
                } else if self.queue.previous().is_some() {
                    self.queue_changed();
                    self.play_current(None, false);
                }
            }
            Command::PlayNext(id) => self.enqueue(id, true),
            Command::AddToQueue(id) => self.enqueue(id, false),
            Command::RemoveFromQueue(index) => {
                if self.queue.remove(index) {
                    self.queue_changed();
                }
            }
            Command::MoveInQueue { from, to } => {
                if self.queue.move_item(from, to) {
                    self.queue_changed();
                }
            }
            Command::PlayQueueIndex(index) => {
                if self.queue.play_index(index) {
                    self.queue_changed();
                    self.play_current(None, false);
                }
            }
            Command::SetShuffle(on) => {
                if self.queue.set_shuffle(on) {
                    self.queue_changed();
                }
            }
            Command::SetRepeat(repeat) => {
                if self.queue.set_repeat(repeat) {
                    self.queue_changed();
                }
            }
            Command::TogglePlay if self.pending_restore.is_some() => {
                self.play_current(self.pending_restore, true);
            }
            Command::TogglePlay => match self.playback.state {
                PlayState::Playing => self.to_audio(sc_audio::Command::Pause),
                PlayState::Paused => self.to_audio(sc_audio::Command::Play),
                // The engine dropped the finished or failed track: start it again.
                PlayState::Ended | PlayState::Idle => self.play_current(None, false),
                PlayState::Loading => {}
            },
            Command::Seek(at) => self.seek(at),
            Command::SetVolume(volume) => {
                let volume = volume.clamp(0.0, 1.0);
                self.to_audio(sc_audio::Command::SetVolume(volume));
                self.playback.volume = volume;
                self.emit(Event::Playback(self.playback));
                // Saved once the slider rests, not on every move.
                if let Some(timer) = self.volume_save.take() {
                    timer.abort();
                }
                let inputs = self.inputs.clone();
                self.volume_save = Some(tokio::spawn(async move {
                    tokio::time::sleep(VOLUME_SAVE_DELAY).await;
                    let _ = inputs.send(Input::SaveDue);
                }));
            }
        }
    }

    /// Seeks the audio, or moves the saved position of a restored track that
    /// has no stream yet.
    fn seek(&mut self, at: Duration) {
        if self.pending_restore.is_some() {
            self.pending_restore = Some(at);
            self.playback.position = at;
            self.emit(Event::Playback(self.playback));
        } else {
            self.to_audio(sc_audio::Command::Seek(at));
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
            self.results.clear();
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
        if append {
            self.results.extend(summaries.iter().cloned());
        } else {
            self.results.clone_from(&summaries);
        }
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

    /// A click in the results: the queue becomes the results, starting here.
    fn play_from_results(&mut self, id: TrackId) {
        if let Some(start) = self.results.iter().position(|t| t.id == id) {
            self.queue.set_context(self.results.clone(), start);
        } else if let Some(track) = self.tracks.get(&id) {
            self.queue
                .set_context(vec![TrackSummary::from_api(track)], 0);
        } else {
            return;
        }
        self.queue_changed();
        self.play_current(None, false);
    }

    /// Queues a track seen in the results. With nothing playing it starts.
    fn enqueue(&mut self, id: TrackId, next: bool) {
        let Some(track) = self.tracks.get(&id) else {
            return;
        };
        let summary = TrackSummary::from_api(track);
        if self.queue.current_track().is_none() {
            self.queue.set_context(vec![summary], 0);
            self.queue_changed();
            self.play_current(None, false);
            return;
        }
        if next {
            self.queue.play_next(summary);
        } else {
            self.queue.add_to_queue(summary);
        }
        self.queue_changed();
    }

    fn queue_changed(&mut self) {
        self.emit(Event::Queue(self.queue.snapshot()));
        self.save_session();
    }

    /// Writes the session off the actor loop. Newer writes win.
    fn save_session(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.last_save = Instant::now();
        let snapshot = self.queue.snapshot();
        let session = Session {
            tracks: snapshot
                .tracks
                .into_iter()
                .map(|track| SessionTrack {
                    artwork_url: self.artwork_url(track.id),
                    track,
                })
                .collect(),
            current: snapshot.current,
            position: self.playback.position,
            volume: self.playback.volume,
            shuffle: snapshot.shuffle,
            repeat: snapshot.repeat,
        };
        self.save_seq += 1;
        let seq = self.save_seq;
        tokio::task::spawn_blocking(move || {
            let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
            if seq <= store.last_seq {
                return;
            }
            store.last_seq = seq;
            if let Err(error) = store::save_session(&mut store.conn, &session) {
                tracing::warn!(%error, "could not save the session");
            }
        });
    }

    fn record_history(&self, track: TrackSummary) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let played_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(error) = store::record_play(&store.conn, &track, played_at) {
                tracing::warn!(%error, "could not record the history");
            }
        });
    }

    /// Brings back the saved queue, paused at the saved position. The stream
    /// is only resolved when the person presses play.
    fn restore(&mut self, session: Session) {
        // Something already started: the person's choice wins.
        if self.current.is_some() || session.tracks.is_empty() {
            return;
        }
        let mut tracks = Vec::with_capacity(session.tracks.len());
        for item in session.tracks {
            if let Some(url) = item.artwork_url {
                self.restored_artwork.insert(item.track.id, url);
            }
            tracks.push(item.track);
        }
        self.queue
            .restore(tracks, session.current, session.shuffle, session.repeat);
        self.playback.volume = session.volume;
        self.to_audio(sc_audio::Command::SetVolume(session.volume));
        self.emit(Event::Queue(self.queue.snapshot()));

        // Covers already on disk show at once; only the current one may download.
        for id in self.queue.snapshot().tracks.iter().map(|t| t.id) {
            let Some(url) = self.artwork_url(id) else {
                continue;
            };
            let path = artwork::path_for(&self.artwork_dir, &url);
            if path.exists() && self.artwork_requested.insert(id) {
                self.emit(Event::Artwork { track: id, path });
            }
        }
        let Some(current) = self.queue.current_track().cloned() else {
            return;
        };
        self.current = Some(current.id);
        self.pending_restore = Some(session.position);
        self.playback.position = session.position;
        self.playback.duration = current.duration;
        let id = current.id;
        self.emit(Event::NowPlaying(current));
        self.set_state(PlayState::Paused);
        self.request_artwork(id);
    }

    /// Where the artwork of a track comes from: this session's tracks, or a
    /// restored queue.
    fn artwork_url(&self, id: TrackId) -> Option<String> {
        self.tracks
            .get(&id)
            .and_then(|t| t.artwork(artwork::SIZE))
            .or_else(|| self.restored_artwork.get(&id).cloned())
    }

    /// The next track, or related tracks when the queue is over.
    fn skip_forward(&mut self, ended: bool) {
        match self.queue.next(ended) {
            Step::Play(_) => {
                self.queue_changed();
                self.play_current(None, true);
            }
            Step::End => self.autoplay(),
        }
    }

    fn autoplay(&mut self) {
        let Some(last) = self.queue.last_track().map(|t| t.id) else {
            return;
        };
        if self.autoplay.is_some() {
            return;
        }
        self.autoplay_gen += 1;
        let generation = self.autoplay_gen;
        self.autoplay = Some(generation);
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.related(last.0, AUTOPLAY_COUNT).await;
            let _ = inputs.send(Input::RelatedDone { generation, result });
        });
    }

    fn related_done(&mut self, generation: u64, result: sc_api::Result<Page<Track>>) {
        if self.autoplay != Some(generation) {
            return;
        }
        self.autoplay = None;
        let page = match result {
            Ok(page) => page,
            Err(error) => {
                self.emit(Event::Problem(Problem::from_api(&error)));
                return;
            }
        };
        let summaries: Vec<TrackSummary> =
            page.collection.iter().map(TrackSummary::from_api).collect();
        for track in page.collection {
            self.tracks.insert(TrackId(track.id), track);
        }
        let ids: Vec<TrackId> = summaries.iter().map(|t| t.id).collect();
        let added = self.queue.extend_context(summaries);
        if added == 0 {
            return;
        }
        self.queue_changed();
        for id in ids {
            self.request_artwork(id);
        }
        // Moving on started this fetch (the track ended or Next was pressed on
        // the last one), and any newer play would have cancelled it.
        self.skip_forward(true);
    }

    /// The current track cannot be fetched or streamed. Tell the person, and
    /// move on unless they picked this track or a full pass already failed.
    fn play_failed(&mut self, problem: Problem) {
        self.set_state(PlayState::Idle);
        self.emit(Event::Problem(problem));
        self.failed_in_row += 1;
        if self.skip_on_failure && self.failed_in_row < self.queue.len() {
            self.skip_forward(false);
        }
    }

    /// Plays the track at the queue's current position.
    /// `moving_on`: reached by Next, a track end or a restore, so a failure
    /// skips ahead.
    fn play_current(&mut self, start_at: Option<Duration>, moving_on: bool) {
        let Some(summary) = self.queue.current_track().cloned() else {
            return;
        };
        let id = summary.id;
        self.current = Some(id);
        self.autoplay = None;
        self.play_gen += 1;
        self.skip_on_failure = moving_on;
        if !moving_on {
            self.failed_in_row = 0;
        }
        self.pending_restore = None;
        self.listened.reset();
        self.playback.position = start_at.unwrap_or_default();
        self.playback.duration = summary.duration;
        self.emit(Event::NowPlaying(summary));
        self.set_state(PlayState::Loading);
        self.request_artwork(id);
        self.save_session();

        let generation = self.play_gen;
        match self.tracks.get(&id).cloned() {
            Some(track) => self.start_stream(track, start_at),
            None => {
                let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
                tokio::spawn(async move {
                    let result = api.track(id.0).await.map(Box::new);
                    let _ = inputs.send(Input::TrackFetched {
                        track: id,
                        generation,
                        start_at,
                        result,
                    });
                });
            }
        }
    }

    fn start_stream(&mut self, track: Track, start_at: Option<Duration>) {
        let id = TrackId(track.id);
        let generation = self.play_gen;
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        let waveform_url = track.waveform_url.clone();
        tokio::spawn(async move {
            let result = api.stream_url(&track).await;
            let _ = inputs.send(Input::StreamReady {
                generation,
                start_at,
                result,
            });
            if let Some(url) = waveform_url
                && let Ok(wave) = api.waveform(&url).await
            {
                let bars = waveform::to_bars(&wave.samples, wave.height, waveform::BARS);
                let _ = inputs.send(Input::WaveformReady {
                    track: id,
                    generation,
                    bars,
                });
            }
        });
    }

    fn request_artwork(&mut self, id: TrackId) {
        if !self.artwork_requested.insert(id) {
            return;
        }
        let Some(url) = self.artwork_url(id) else {
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
        if state == PlayState::Paused {
            self.save_session();
        }
    }

    fn audio_event(&mut self, event: sc_audio::Event) {
        match event {
            sc_audio::Event::State(state) => {
                let state = match state {
                    sc_audio::PlaybackState::Idle => PlayState::Idle,
                    sc_audio::PlaybackState::Loading => PlayState::Loading,
                    sc_audio::PlaybackState::Playing => PlayState::Playing,
                    sc_audio::PlaybackState::Paused => PlayState::Paused,
                    sc_audio::PlaybackState::Ended => PlayState::Ended,
                };
                self.set_state(state);
                if state == PlayState::Ended {
                    self.skip_forward(true);
                }
            }
            sc_audio::Event::Position(position) => {
                self.playback.position = position;
                self.emit(Event::Playback(self.playback));
                if self.listened.tick(position)
                    && let Some(track) = self.queue.current_track().cloned()
                {
                    self.record_history(track);
                }
                if self.last_save.elapsed() >= SAVE_EVERY {
                    self.save_session();
                }
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
