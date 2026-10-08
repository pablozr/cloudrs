//! The core actor: one loop that owns all state and handles one input at a
//! time. Slow work (HTTP, disk) runs in spawned tasks that report back as
//! inputs, so state is never shared or locked.
//!
//! This file holds the state, the inputs and the two dispatchers (`handle`,
//! `command`). The work is split by topic, each an `impl Core` block:
//! `paging` (lists), `pages` (screen headers and links), `playback`,
//! `session` (saving, restoring, history, volume) and `account`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_api::models::{Playlist, Resource, Track, User};
use sc_api::{SoundCloudApi, StreamSource};
use tokio::task::JoinHandle;

use crate::listen::Listened;
use crate::lists::{Fetched, ListState};
use crate::queue::Queue;
use crate::store::SessionTrack;
use crate::types::{
    Account, ArtKey, ListId, PlayState, Playback, PlaylistId, Problem, SearchKind, TrackId, UserId,
};
use crate::{Command, CoreConfig, Event, Settings, artwork};
use session::{OpenedStore, SharedStore, open_store};

mod account;
mod comments;
mod jam;
mod pages;
mod paging;
mod playback;
mod playlists;
mod session;
mod settings;

/// How long the search waits for more typing.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
/// Past this position "previous" restarts the track instead of going back.
const RESTART_AFTER: Duration = Duration::from_secs(3);

type AudioLink = (
    flume::Sender<sc_audio::Command>,
    flume::Receiver<sc_audio::Event>,
);

enum Input {
    /// SoundCloud's answer to a change of one of the person's playlists.
    PlaylistEdited {
        result: sc_api::Result<Box<playlists::Saved>>,
    },
    /// SoundCloud's home rows and charts, in that order.
    HomeFetched {
        selections: sc_api::Result<sc_api::models::Page<sc_api::models::Selection>>,
        charts: sc_api::Result<sc_api::models::Page<sc_api::models::Selection>>,
    },
    /// Something happened in the Jam's session.
    Jam {
        generation: u64,
        event: sc_session::SessionEvent,
    },
    JamTimer {
        generation: u64,
        timer: jam::JamTimer,
    },
    /// A track the Jam needed (a guest's request, or the host's prepare).
    JamTrack {
        generation: u64,
        then: jam::AfterFetch,
        result: sc_api::Result<Box<Track>>,
    },
    /// The tracks of the host's queue a guest had not seen.
    JamMirror {
        generation: u64,
        ids: Vec<u64>,
        current: Option<u32>,
        result: sc_api::Result<Vec<Track>>,
    },
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
    CommentsDone {
        track: TrackId,
        result: sc_api::Result<sc_api::models::Page<sc_api::models::Comment>>,
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
    /// The track that follows the current one has its stream URL (and, if it
    /// was not cached, its details), for gapless playback.
    PreloadReady {
        generation: u64,
        key: u64,
        track: Option<Box<Track>>,
        result: sc_api::Result<StreamSource>,
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
    /// `/me` answered for the token of a sign-in.
    SignInChecked {
        generation: u64,
        token: String,
        result: sc_api::Result<Box<User>>,
    },
    AccountIds {
        generation: u64,
        liked: sc_api::Result<Vec<u64>>,
        followed: sc_api::Result<Vec<u64>>,
    },
    LikeDone {
        track: TrackId,
        liked: bool,
        result: sc_api::Result<()>,
    },
    FollowDone {
        user: UserId,
        following: bool,
        result: sc_api::Result<()>,
    },
    /// The audio devices were listed.
    OutputDevices(Vec<sc_audio::OutputDevice>),
    /// The artwork folder was measured.
    CacheMeasured(u64),
    /// The artwork folder was cleared of what is not in use.
    CacheCleared {
        remaining: u64,
        complete: bool,
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
        preload: None,
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
        account: None,
        sign_in_gen: 0,
        jam: None,
        jam_gen: 0,
        jam_network: config.jam_network,
        settings: config.settings,
        settings_seq: 0,
        settings_changed: false,
    };
    if let Some(token) = config.oauth_token {
        core.sign_in(token);
    }
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
    /// The next track being prepared for gapless playback, if any (ADR 0022).
    preload: Option<playback::Preload>,
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
    /// The signed-in person, once `/me` accepted the token.
    account: Option<Account>,
    /// Bumped on every sign-in and sign-out: answers for an older one are dropped.
    sign_in_gen: u64,
    /// The Jam this person hosts or joined (ADR 0011).
    jam: Option<jam::Jam>,
    /// Bumped on every Jam: answers for an older one are dropped.
    jam_gen: u64,
    jam_network: crate::JamNetwork,
    settings: Settings,
    settings_seq: u64,
    /// The person changed a setting this run, so the database needs it.
    settings_changed: bool,
}

impl<A: SoundCloudApi + 'static> Core<A> {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn handle(&mut self, input: Input) {
        match input {
            Input::Ui(command) => self.command(command),
            Input::UiClosed => {}
            Input::PlaylistEdited { result } => self.playlist_edited(result),
            Input::HomeFetched { selections, charts } => self.home_fetched(selections, charts),
            Input::Jam { generation, event } => self.jam_event(generation, event),
            Input::JamTimer { generation, timer } => self.jam_timer(generation, timer),
            Input::JamTrack {
                generation,
                then,
                result,
            } => self.jam_track_fetched(generation, then, result),
            Input::JamMirror {
                generation,
                ids,
                current,
                result,
            } => self.jam_mirror_fetched(generation, ids, current, result),
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
            Input::CommentsDone { track, result } => self.comments_done(track, result),
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
                if nav == self.nav_gen {
                    self.resolved(result);
                }
            }
            Input::TrackFetched {
                track,
                generation,
                start_at,
                result,
            } => {
                if generation == self.play_gen {
                    self.track_fetched(track, start_at, result);
                }
            }
            Input::RelatedDone { generation, result } => self.related_done(generation, result),
            Input::PreloadReady {
                generation,
                key,
                track,
                result,
            } => self.preload_ready(generation, key, track, result),
            Input::SaveDue => self.save_session(),
            Input::StoreReady(opened) => self.store_ready(opened),
            Input::StreamReady {
                generation,
                start_at,
                result,
            } => {
                if generation == self.play_gen {
                    self.stream_ready(start_at, result);
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
            Input::SignInChecked {
                generation,
                token,
                result,
            } => self.sign_in_checked(generation, token, result),
            Input::AccountIds {
                generation,
                liked,
                followed,
            } => self.account_ids(generation, liked, followed),
            Input::LikeDone {
                track,
                liked,
                result,
            } => self.like_done(track, liked, result),
            Input::FollowDone {
                user,
                following,
                result,
            } => self.follow_done(user, following, result),
            Input::OutputDevices(devices) => self.emit(Event::OutputDevices(
                devices
                    .into_iter()
                    .map(|d| crate::OutputDevice {
                        id: d.id,
                        name: d.name,
                    })
                    .collect(),
            )),
            Input::CacheMeasured(bytes) => self.emit(Event::CacheSize(bytes)),
            Input::CacheCleared {
                remaining,
                complete,
            } => self.cache_cleared(remaining, complete),
        }
    }

    fn command(&mut self, command: Command) {
        if self.jam_command(&command) {
            return;
        }
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
            Command::OpenTrack(id) => self.open_track(id),
            Command::LoadComments(id) => self.load_comments(id),
            Command::LoadArtwork(key) => self.request_artwork(key),
            Command::OpenUser(id) => self.open_user(id),
            Command::OpenPlaylist(id) => self.open_playlist(id),
            Command::OpenHistory => self.open_history(),
            Command::OpenHome => self.open_home(),
            Command::OpenUrl(url) => self.open_url(url),
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
            Command::SignIn(token) => self.sign_in(token),
            Command::SignOut => self.sign_out(),
            Command::Like { track, liked } => self.like(track, liked),
            Command::Follow { user, following } => self.follow(user, following),
            Command::CreatePlaylist(new) => {
                self.edit_playlist(None, playlists::Edit::Create(Box::new(new)));
            }
            Command::SetPlaylistDescription {
                playlist,
                description,
            } => self.edit_playlist(Some(playlist), playlists::Edit::Describe(description)),
            Command::SetPlaylistCover { playlist, cover } => {
                self.edit_playlist(Some(playlist), playlists::Edit::Cover(cover));
            }
            Command::AddToPlaylist {
                playlist,
                track,
                at,
            } => {
                self.edit_playlist(Some(playlist), playlists::Edit::Add(track, at));
            }
            Command::RemoveFromPlaylist { playlist, index } => {
                self.edit_playlist(Some(playlist), playlists::Edit::Remove(index));
            }
            Command::MoveInPlaylist { playlist, from, to } => {
                self.edit_playlist(Some(playlist), playlists::Edit::Move { from, to });
            }
            Command::RenamePlaylist { playlist, title } => {
                let title = title.trim().to_owned();
                self.edit_playlist(Some(playlist), playlists::Edit::Rename(title));
            }
            Command::SetPlaylistPublic { playlist, public } => {
                self.edit_playlist(Some(playlist), playlists::Edit::Privacy(public));
            }
            Command::DeletePlaylist(playlist) => {
                self.edit_playlist(Some(playlist), playlists::Edit::Delete);
            }
            // Taken by `jam_command` above.
            Command::StartJam
            | Command::JoinJam(_)
            | Command::LeaveJam
            | Command::SetJamGuestsControl(_)
            | Command::RemoveFromJam(_) => {}
            Command::SetVolume(volume) => self.set_volume(volume),
            Command::SetSettings(settings) => self.set_settings(settings),
            Command::ListOutputDevices => self.list_output_devices(),
            Command::MeasureCache => self.measure_cache(),
            Command::ClearCache => self.clear_cache(),
        }
    }

    fn to_audio(&self, command: sc_audio::Command) {
        let _ = self.audio.send(command);
    }

    fn next_nav(&mut self) -> u64 {
        self.nav_gen += 1;
        self.nav_gen
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
