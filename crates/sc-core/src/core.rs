//! The core actor: one loop that owns all state and handles one input at a
//! time. Slow work (HTTP, disk) runs in spawned tasks that report back as
//! inputs, so state is never shared or locked.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sc_api::models::{Playlist, Resource, Track, User};
use sc_api::{SoundCloudApi, StreamProtocol, StreamSource};
use tokio::task::JoinHandle;

use crate::listen::Listened;
use crate::lists::{self, Fetched, ListState, SEARCH_KINDS};
use crate::queue::{Queue, Step};
use crate::store::{self, Session, SessionTrack};
use crate::types::{
    ArtKey, ListId, ListItems, PlayState, Playback, PlaylistId, PlaylistPage, PlaylistSummary,
    Problem, SearchKind, TrackId, TrackPage, TrackSummary, UserId, UserPage, UserSummary,
};
use crate::{Command, CoreConfig, Event, artwork, waveform};

/// How long the search waits for more typing.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
/// Tracks the history screen lists.
const HISTORY_LIMIT: u32 = 200;
/// Ids per `/tracks?ids=` call when filling a playlist.
const FILL_BATCH: usize = 50;
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
    ListDone {
        list: ListId,
        generation: u64,
        append: bool,
        result: sc_api::Result<Fetched>,
    },
    /// A screen's header answers carry the navigation counter of their request.
    TrackOpened {
        nav: u64,
        result: sc_api::Result<Box<Track>>,
    },
    UserOpened {
        nav: u64,
        result: sc_api::Result<Box<User>>,
    },
    PlaylistOpened {
        nav: u64,
        result: sc_api::Result<Box<Playlist>>,
    },
    /// Every track of a playlist, partial ones filled in, in playlist order.
    PlaylistTracks {
        id: PlaylistId,
        generation: u64,
        result: sc_api::Result<Vec<Track>>,
    },
    HistoryLoaded {
        generation: u64,
        result: rusqlite::Result<Vec<SessionTrack>>,
    },
    Resolved {
        nav: u64,
        result: sc_api::Result<Resource>,
    },
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
        result: sc_api::Result<sc_api::models::Page<Track>>,
    },
    /// The debounced session save after a volume change.
    SaveDue,
    /// The database opened (or not) off the actor loop, with the saved session.
    StoreReady(Option<OpenedStore>),
    WaveformReady {
        track: TrackId,
        generation: u64,
        bars: Vec<f32>,
    },
    ArtworkReady {
        key: ArtKey,
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

/// An open store, the session it held, and whether a damaged file was reset.
type OpenedStore = (SharedStore, Option<Session>, bool);

/// Opens the database and loads the saved session. Runs on a blocking thread.
fn open_store(dir: &std::path::Path) -> Option<OpenedStore> {
    if let Err(error) = std::fs::create_dir_all(dir) {
        tracing::warn!(%error, "no session database; continuing without saving");
        return None;
    }
    let (conn, reset) = match store::open_or_reset(&dir.join(store::FILE_NAME)) {
        Ok(opened) => opened,
        Err(error) => {
            tracing::warn!(%error, "no session database; continuing without saving");
            return None;
        }
    };
    let session = store::load_session(&conn)
        .inspect_err(|error| tracing::warn!(%error, "could not read the saved session"))
        .ok()
        .flatten();
    Some((
        Arc::new(Mutex::new(Store { conn, last_seq: 0 })),
        session,
        reset,
    ))
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
        kind: SearchKind::default(),
        lists: HashMap::new(),
        list_gen: 0,
        nav_gen: 0,
        other_art: HashMap::new(),
        queue: Queue::new(shuffle_seed()),
        stopped: false,
        dirty: false,
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
            core.shutdown();
            break;
        }
        core.handle(input);
        if core.stopped {
            break;
        }
    }
}

/// Completes the tracks of a playlist that only carry an id, fetching them
/// `FILL_BATCH` at a time. Tracks SoundCloud no longer returns are dropped.
async fn fill_tracks<A: SoundCloudApi>(api: &A, tracks: Vec<Track>) -> sc_api::Result<Vec<Track>> {
    let missing: Vec<u64> = tracks
        .iter()
        .filter(|t| t.title.is_empty())
        .map(|t| t.id)
        .collect();
    let mut filled = HashMap::new();
    for batch in missing.chunks(FILL_BATCH) {
        for track in api.tracks(batch).await? {
            filled.insert(track.id, track);
        }
    }
    Ok(tracks
        .into_iter()
        .filter_map(|t| {
            if t.title.is_empty() {
                filled.remove(&t.id)
            } else {
                Some(t)
            }
        })
        .collect())
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
    /// The text of the current search.
    query: String,
    /// The search tab.
    kind: SearchKind,
    /// Every list served: paging state and the track rows of the context of
    /// `Play`. Re-opening a screen replaces its lists.
    lists: HashMap<ListId, ListState>,
    /// Source of list generations: a new or reset list takes the next number,
    /// so answers meant for its predecessor are dropped.
    list_gen: u64,
    /// Bumped on every screen open: a slow older header is dropped.
    nav_gen: u64,
    /// Where avatars and playlist covers come from (track covers live in `tracks`).
    other_art: HashMap<ArtKey, String>,
    queue: Queue,
    /// `Shutdown` was handled: the loop ends.
    stopped: bool,
    /// Something worth saving happened. A core that only started and closed
    /// (a second instance, say) must not overwrite the saved session.
    dirty: bool,
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
    artwork_requested: HashSet<ArtKey>,
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
            Input::ListDone {
                list,
                generation,
                append,
                result,
            } => self.list_done(list, generation, append, result),
            Input::TrackOpened { nav, result } => {
                if nav == self.nav_gen {
                    match result {
                        Ok(track) => self.track_opened(*track),
                        Err(error) => self.emit(Event::Problem(Problem::from_api(&error))),
                    }
                }
            }
            Input::UserOpened { nav, result } => {
                if nav == self.nav_gen {
                    match result {
                        Ok(user) => self.user_opened(&user),
                        Err(error) => self.emit(Event::Problem(Problem::from_api(&error))),
                    }
                }
            }
            Input::PlaylistOpened { nav, result } => {
                if nav == self.nav_gen {
                    match result {
                        Ok(playlist) => self.playlist_opened(*playlist),
                        Err(error) => self.emit(Event::Problem(Problem::from_api(&error))),
                    }
                }
            }
            Input::PlaylistTracks {
                id,
                generation,
                result,
            } => self.playlist_tracks(id, generation, result),
            Input::HistoryLoaded { generation, result } => self.history_loaded(generation, result),
            Input::Resolved { nav, result } => {
                if nav != self.nav_gen {
                    return;
                }
                match result {
                    Ok(Resource::Track(track)) => {
                        let id = TrackId(track.id);
                        let summary = TrackSummary::from_api(&track);
                        self.tracks.insert(id, *track);
                        self.queue.set_context(vec![summary], 0);
                        self.queue_changed();
                        self.play_current(None, false);
                    }
                    Ok(Resource::User(user)) => self.user_opened(&user),
                    Ok(Resource::Playlist(playlist)) => self.playlist_opened(*playlist),
                    Ok(Resource::Unknown) => {
                        self.emit(Event::Problem(Problem::UnsupportedLink));
                    }
                    Err(error) => self.emit(Event::Problem(Problem::from_api(&error))),
                }
            }
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
                let Some((store, session, reset)) = opened else {
                    return;
                };
                self.store = Some(store);
                if reset {
                    self.emit(Event::Problem(Problem::StorageReset));
                }
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
            Input::ArtworkReady { key, path } => self.emit(Event::Artwork { key, path }),
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Search(query) => {
                self.query = query.trim().to_owned();
                self.start_search(SEARCH_DEBOUNCE);
            }
            Command::SetSearchKind(kind) => {
                if kind != self.kind {
                    self.kind = kind;
                    self.start_search(Duration::ZERO);
                }
            }
            Command::LoadMore(list) => self.load_more(list),
            Command::Play { list, track } => self.play_from_list(list, track),
            Command::OpenTrack(id) => {
                let nav = self.next_nav();
                let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
                tokio::spawn(async move {
                    let result = api.track(id.0).await.map(Box::new);
                    let _ = inputs.send(Input::TrackOpened { nav, result });
                });
            }
            Command::OpenUser(id) => {
                let nav = self.next_nav();
                let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
                tokio::spawn(async move {
                    let result = api.user(id.0).await.map(Box::new);
                    let _ = inputs.send(Input::UserOpened { nav, result });
                });
            }
            Command::OpenPlaylist(id) => self.open_playlist(id),
            Command::OpenHistory => self.open_history(),
            Command::OpenUrl(url) => {
                let nav = self.next_nav();
                let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
                tokio::spawn(async move {
                    let result = api.resolve(url.trim()).await;
                    let _ = inputs.send(Input::Resolved { nav, result });
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
            Command::Shutdown => self.shutdown(),
            Command::SetVolume(volume) => {
                let volume = volume.clamp(0.0, 1.0);
                self.to_audio(sc_audio::Command::SetVolume(volume));
                self.playback.volume = volume;
                self.dirty = true;
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

    fn next_nav(&mut self) -> u64 {
        self.nav_gen += 1;
        self.nav_gen
    }

    /// Replaces the state of `list` with a fresh, loading one. Dropping the
    /// old state cancels its fetch; the new generation drops its late answers.
    fn reset_list(&mut self, list: ListId) -> u64 {
        self.list_gen += 1;
        self.lists.insert(list, ListState::new(self.list_gen));
        self.list_gen
    }

    /// Restarts the search of the current kind, waiting `delay` first. A new
    /// search invalidates the lists of every tab.
    fn start_search(&mut self, delay: Duration) {
        for kind in SEARCH_KINDS {
            self.lists.remove(&ListId::Search { kind });
        }
        let list = ListId::Search { kind: self.kind };
        if self.query.is_empty() {
            self.emit(Event::List {
                list,
                items: ListItems::empty(self.kind),
                append: false,
                has_more: false,
            });
            return;
        }
        let generation = self.reset_list(list);
        self.fetch_list(list, generation, None, delay);
    }

    /// Fetches a page of an API-backed list in the background.
    fn fetch_list(&mut self, list: ListId, generation: u64, next: Option<String>, delay: Duration) {
        let (api, inputs, events) = (
            Arc::clone(&self.api),
            self.inputs.clone(),
            self.events.clone(),
        );
        let query = self.query.clone();
        let task = tokio::spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let append = next.is_some();
            if let ListId::Search { kind } = list
                && !append
            {
                let _ = events.send(Event::Searching {
                    query: query.clone(),
                    kind,
                });
            }
            let result = lists::fetch(&*api, list, &query, next).await;
            let _ = inputs.send(Input::ListDone {
                list,
                generation,
                append,
                result,
            });
        });
        if let Some(state) = self.lists.get_mut(&list) {
            state.task = Some(task);
        }
    }

    /// The next page of a list. A list never served loads its first page.
    fn load_more(&mut self, list: ListId) {
        let Some(state) = self.lists.get_mut(&list) else {
            self.load_first(list);
            return;
        };
        if state.loading {
            return;
        }
        let Some(next) = state.next_href.clone() else {
            return;
        };
        state.loading = true;
        let generation = state.generation;
        self.fetch_list(list, generation, Some(next), Duration::ZERO);
    }

    fn load_first(&mut self, list: ListId) {
        match list {
            ListId::Playlist(id) => self.open_playlist(id),
            ListId::History => self.open_history(),
            ListId::Search { .. } if self.query.is_empty() => {}
            _ => {
                let generation = self.reset_list(list);
                self.fetch_list(list, generation, None, Duration::ZERO);
            }
        }
    }

    fn list_done(
        &mut self,
        list: ListId,
        generation: u64,
        append: bool,
        result: sc_api::Result<Fetched>,
    ) {
        if self
            .lists
            .get(&list)
            .is_none_or(|s| s.generation != generation)
        {
            return;
        }
        let fetched = match result {
            Ok(fetched) => fetched,
            Err(error) => {
                // A failed first page can be asked for again (no state); a
                // failed next page is not offered again.
                if append {
                    if let Some(state) = self.lists.get_mut(&list) {
                        state.loading = false;
                        state.task = None;
                        state.next_href = None;
                    }
                } else {
                    self.lists.remove(&list);
                }
                self.emit(Event::ListFailed {
                    list,
                    append,
                    problem: Problem::from_api(&error),
                });
                return;
            }
        };
        let next_href = fetched.next_href();
        let (items, art) = self.absorb(fetched);
        if let Some(state) = self.lists.get_mut(&list) {
            state.loading = false;
            state.task = None;
            state.next_href.clone_from(&next_href);
            if let ListItems::Tracks(tracks) = &items {
                if !append {
                    state.tracks.clear();
                }
                state.tracks.extend(tracks.iter().cloned());
            }
        }
        self.emit(Event::List {
            list,
            items,
            append,
            has_more: next_href.is_some(),
        });
        for key in art {
            self.request_artwork(key);
        }
    }

    /// Remembers what a page carries (tracks to play later, image URLs) and
    /// turns it into rows.
    fn absorb(&mut self, fetched: Fetched) -> (ListItems, Vec<ArtKey>) {
        match fetched {
            Fetched::Tracks(page) => self.absorb_tracks(page.collection),
            Fetched::Likes(page) => self.absorb_tracks(
                page.collection
                    .into_iter()
                    .filter_map(|l| l.track)
                    .collect(),
            ),
            Fetched::Users(page) => {
                let mut art = Vec::new();
                let users = page
                    .collection
                    .iter()
                    .map(|user| {
                        let id = UserId(user.id);
                        if let Some(url) = user.avatar(artwork::SIZE) {
                            self.other_art.insert(ArtKey::User(id), url);
                            art.push(ArtKey::User(id));
                        }
                        UserSummary::from_api(user)
                    })
                    .collect();
                (ListItems::Users(users), art)
            }
            Fetched::Playlists(page) => {
                let mut art = Vec::new();
                let playlists = page
                    .collection
                    .iter()
                    .map(|playlist| {
                        let id = PlaylistId(playlist.id);
                        if let Some(url) = playlist.artwork(artwork::SIZE) {
                            self.other_art.insert(ArtKey::Playlist(id), url);
                            art.push(ArtKey::Playlist(id));
                        }
                        PlaylistSummary::from_api(playlist)
                    })
                    .collect();
                (ListItems::Playlists(playlists), art)
            }
        }
    }

    fn absorb_tracks(&mut self, tracks: Vec<Track>) -> (ListItems, Vec<ArtKey>) {
        let summaries = tracks.iter().map(TrackSummary::from_api).collect();
        let art = tracks
            .iter()
            .map(|t| ArtKey::Track(TrackId(t.id)))
            .collect();
        for track in tracks {
            self.tracks.insert(TrackId(track.id), track);
        }
        (ListItems::Tracks(summaries), art)
    }

    /// A track summary from any list or the tracks seen so far.
    fn find_summary(&self, id: TrackId) -> Option<TrackSummary> {
        self.tracks
            .get(&id)
            .map(TrackSummary::from_api)
            .or_else(|| {
                self.lists
                    .values()
                    .find_map(|state| state.tracks.iter().find(|t| t.id == id).cloned())
            })
    }

    fn track_opened(&mut self, track: Track) {
        let id = TrackId(track.id);
        self.emit(Event::TrackPage(TrackPage::from_api(&track)));
        if let Some(url) = track.waveform_url.clone() {
            let (api, events) = (Arc::clone(&self.api), self.events.clone());
            tokio::spawn(async move {
                if let Ok(wave) = api.waveform(&url).await {
                    let bars = waveform::to_bars(&wave.samples, wave.height, waveform::BARS);
                    let _ = events.send(Event::Waveform { track: id, bars });
                }
            });
        }
        self.tracks.insert(id, track);
        self.request_artwork(ArtKey::Track(id));

        let list = ListId::Related(id);
        let generation = self.reset_list(list);
        self.fetch_list(list, generation, None, Duration::ZERO);
    }

    /// The profile header, then the first page of its tracks. Playlists and
    /// likes start over: they load when the UI asks.
    fn user_opened(&mut self, user: &User) {
        let id = UserId(user.id);
        self.emit(Event::UserPage(UserPage::from_api(user)));
        if let Some(url) = user.avatar(artwork::SIZE) {
            self.other_art.insert(ArtKey::User(id), url);
            self.request_artwork(ArtKey::User(id));
        }
        self.lists.remove(&ListId::UserPlaylists(id));
        self.lists.remove(&ListId::UserLikes(id));

        let list = ListId::UserTracks(id);
        let generation = self.reset_list(list);
        self.fetch_list(list, generation, None, Duration::ZERO);
    }

    fn open_playlist(&mut self, id: PlaylistId) {
        let nav = self.next_nav();
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            let result = api.playlist(id.0).await.map(Box::new);
            let _ = inputs.send(Input::PlaylistOpened { nav, result });
        });
    }

    /// The playlist header, then every track: the ones that only carry an id
    /// are filled in batches before the list is sent.
    fn playlist_opened(&mut self, playlist: Playlist) {
        let id = PlaylistId(playlist.id);
        self.emit(Event::PlaylistPage(PlaylistPage::from_api(&playlist)));
        if let Some(url) = playlist.artwork(artwork::SIZE) {
            self.other_art.insert(ArtKey::Playlist(id), url);
            self.request_artwork(ArtKey::Playlist(id));
        }

        let list = ListId::Playlist(id);
        let generation = self.reset_list(list);
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        let task = tokio::spawn(async move {
            let result = fill_tracks(&*api, playlist.tracks).await;
            let _ = inputs.send(Input::PlaylistTracks {
                id,
                generation,
                result,
            });
        });
        if let Some(state) = self.lists.get_mut(&list) {
            state.task = Some(task);
        }
    }

    fn playlist_tracks(
        &mut self,
        id: PlaylistId,
        generation: u64,
        result: sc_api::Result<Vec<Track>>,
    ) {
        let list = ListId::Playlist(id);
        if self
            .lists
            .get(&list)
            .is_none_or(|s| s.generation != generation)
        {
            return;
        }
        match result {
            Ok(tracks) => {
                let (items, art) = self.absorb_tracks(tracks);
                if let Some(state) = self.lists.get_mut(&list) {
                    state.loading = false;
                    state.task = None;
                    if let ListItems::Tracks(tracks) = &items {
                        state.tracks.clone_from(tracks);
                    }
                }
                self.emit(Event::List {
                    list,
                    items,
                    append: false,
                    has_more: false,
                });
                for key in art {
                    self.request_artwork(key);
                }
            }
            Err(error) => {
                self.lists.remove(&list);
                self.emit(Event::ListFailed {
                    list,
                    append: false,
                    problem: Problem::from_api(&error),
                });
            }
        }
    }

    fn open_history(&mut self) {
        let generation = self.reset_list(ListId::History);
        let Some(store) = self.store.clone() else {
            // The database is not open (yet): nothing was played that we know of.
            self.history_loaded(generation, Ok(Vec::new()));
            return;
        };
        let inputs = self.inputs.clone();
        tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(PoisonError::into_inner);
            let result = store::recent_history(&store.conn, HISTORY_LIMIT);
            let _ = inputs.send(Input::HistoryLoaded { generation, result });
        });
    }

    fn history_loaded(&mut self, generation: u64, result: rusqlite::Result<Vec<SessionTrack>>) {
        let list = ListId::History;
        if self
            .lists
            .get(&list)
            .is_none_or(|s| s.generation != generation)
        {
            return;
        }
        let rows = match result {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "could not read the history");
                self.lists.remove(&list);
                self.emit(Event::ListFailed {
                    list,
                    append: false,
                    problem: Problem::StorageReset,
                });
                return;
            }
        };
        let mut tracks = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(url) = row.artwork_url {
                self.restored_artwork.entry(row.track.id).or_insert(url);
            }
            tracks.push(row.track);
        }
        if let Some(state) = self.lists.get_mut(&list) {
            state.loading = false;
            state.tracks.clone_from(&tracks);
        }
        let ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        self.emit(Event::List {
            list,
            items: ListItems::Tracks(tracks),
            append: false,
            has_more: false,
        });
        for id in ids {
            self.request_artwork(ArtKey::Track(id));
        }
    }

    /// A click in a list: the queue becomes the rows loaded so far, starting here.
    fn play_from_list(&mut self, list: ListId, id: TrackId) {
        let context = self.lists.get(&list).map(|state| &state.tracks);
        if let Some(context) = context
            && let Some(start) = context.iter().position(|t| t.id == id)
        {
            self.queue.set_context(context.clone(), start);
        } else if let Some(summary) = self.find_summary(id) {
            self.queue.set_context(vec![summary], 0);
        } else {
            return;
        }
        self.queue_changed();
        self.play_current(None, false);
    }

    /// Queues a track seen in any list. With nothing playing it starts.
    fn enqueue(&mut self, id: TrackId, next: bool) {
        let Some(summary) = self.find_summary(id) else {
            return;
        };
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

    /// The state worth keeping across a restart.
    fn session(&self) -> Session {
        let snapshot = self.queue.snapshot();
        Session {
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
        }
    }

    /// Saves for the last time, on the actor thread, and says so. Taking the
    /// store lock waits for a write in progress, and the sequence number makes
    /// any write still queued skip itself.
    fn shutdown(&mut self) {
        if let Some(store) = self.store.clone().filter(|_| self.dirty) {
            let session = self.session();
            self.save_seq += 1;
            let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
            store.last_seq = self.save_seq;
            if let Err(error) = store::save_session(&mut store.conn, &session) {
                tracing::warn!(%error, "could not save the session on exit");
            }
        }
        self.emit(Event::Stopped);
        self.stopped = true;
    }

    /// Writes the session off the actor loop. Newer writes win.
    fn save_session(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.last_save = Instant::now();
        self.dirty = true;
        let session = self.session();
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
        let artwork_url = self.artwork_url(track.id);
        tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(error) =
                store::record_play(&store.conn, &track, artwork_url.as_deref(), played_at)
            {
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
            if path.exists() && self.artwork_requested.insert(ArtKey::Track(id)) {
                self.emit(Event::Artwork {
                    key: ArtKey::Track(id),
                    path,
                });
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
        // Not `set_state`: restoring is not a change worth saving, and writing
        // here could overwrite a newer session another instance just saved.
        self.playback.state = PlayState::Paused;
        self.emit(Event::Playback(self.playback));
        self.request_artwork(ArtKey::Track(id));
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

    fn related_done(
        &mut self,
        generation: u64,
        result: sc_api::Result<sc_api::models::Page<Track>>,
    ) {
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
            self.request_artwork(ArtKey::Track(id));
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
        self.request_artwork(ArtKey::Track(id));
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

    fn request_artwork(&mut self, key: ArtKey) {
        if !self.artwork_requested.insert(key) {
            return;
        }
        let url = match key {
            ArtKey::Track(id) => self.artwork_url(id),
            ArtKey::User(_) | ArtKey::Playlist(_) => self.other_art.get(&key).cloned(),
        };
        let Some(url) = url else {
            return;
        };
        let path = artwork::path_for(&self.artwork_dir, &url);
        if path.exists() {
            self.emit(Event::Artwork { key, path });
            return;
        }
        let (api, inputs) = (Arc::clone(&self.api), self.inputs.clone());
        tokio::spawn(async move {
            match api.download(&url).await {
                Ok(bytes) => match artwork::store(&path, &bytes) {
                    Ok(()) => {
                        let _ = inputs.send(Input::ArtworkReady { key, path });
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
