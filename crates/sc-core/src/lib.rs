//! The cloudrs application core (ADR 0004).
//!
//! The UI sends [`Command`]s and renders [`Event`]s. Everything else (talking
//! to SoundCloud, driving the audio engine, caching artwork, debouncing the
//! search) happens here, on the core's own thread. The UI never sees `sc-api`
//! or `sc-audio` types.

mod artwork;
mod core;
mod listen;
mod lists;
mod queue;
mod store;
mod types;
mod waveform;

use std::path::PathBuf;
use std::time::Duration;

pub use types::{
    Account, ArtKey, Genre, HomeShelf, JamPerson, JamRole, JamState, ListId, ListItems, PlayState,
    Playback, PlaylistChange, PlaylistId, PlaylistPage, PlaylistSummary, Problem, QueueSnapshot,
    Repeat, SearchKind, TrackId, TrackPage, TrackSummary, UserId, UserPage, UserSummary,
};

/// What the UI asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// The search text changed. The core waits 300 ms for more typing; an
    /// empty query clears the results. Searches the current [`SearchKind`].
    Search(String),
    /// Switch the search tab. A non-empty query runs again for the new kind.
    SetSearchKind(SearchKind),
    /// Fetch the next page of a list. For a list the core has not served yet
    /// (a profile's playlists or likes) this loads the first page.
    LoadMore(ListId),
    /// Play a track from a list. The queue becomes the items of that list
    /// loaded so far, starting at this track.
    Play {
        list: ListId,
        track: TrackId,
    },
    /// Open the track screen: answers with [`Event::TrackPage`], the
    /// [`Event::Waveform`] and a [`Event::List`] for `ListId::Related`.
    OpenTrack(TrackId),
    /// Open a profile: [`Event::UserPage`], then `ListId::UserTracks`.
    OpenUser(UserId),
    /// Open a playlist or album: [`Event::PlaylistPage`], then all its tracks
    /// as `ListId::Playlist`.
    OpenPlaylist(PlaylistId),
    /// The played tracks, newest first, as a single `ListId::History` page.
    OpenHistory,
    /// SoundCloud's own home rows: answers with [`Event::HomeShelves`].
    /// Trending tracks are `ListId::Trending(genre)`, asked with `LoadMore`.
    OpenHome,
    /// Open whatever a pasted soundcloud.com URL points to: a track plays, a
    /// profile or playlist opens its screen.
    OpenUrl(String),
    /// The next track (the queue may fetch more when it ends).
    Next,
    /// Restarts the track when it is past 3 s, otherwise the previous track.
    Previous,
    /// Plays right after the current track (a track seen in the results).
    PlayNext(TrackId),
    /// Plays after the other "up next" tracks, before the rest of the queue.
    AddToQueue(TrackId),
    /// The current track cannot be removed.
    RemoveFromQueue(usize),
    MoveInQueue {
        from: usize,
        to: usize,
    },
    PlayQueueIndex(usize),
    SetShuffle(bool),
    SetRepeat(Repeat),
    /// The window is closing: save the session now, answer with
    /// [`Event::Stopped`] and stop. Later commands are ignored.
    Shutdown,
    /// Pause if playing, otherwise play (restarting a finished track).
    TogglePlay,
    Seek(Duration),
    /// 0.0 to 1.0.
    SetVolume(f32),
    /// Sign in with a soundcloud.com `oauth_token`: answers with
    /// [`Event::SignedIn`], or `Problem::SignInFailed` when it is refused.
    SignIn(String),
    /// Forget the token: answers with [`Event::SignedOut`].
    SignOut,
    /// Like or unlike a track. Answers at once with [`Event::Liked`] and
    /// reverts it if SoundCloud refuses.
    Like {
        track: TrackId,
        liked: bool,
    },
    /// Follow or unfollow someone, like [`Command::Like`].
    Follow {
        user: UserId,
        following: bool,
    },
    /// Create a playlist of the signed-in person, private, with this track in
    /// it if given. Answers with [`Event::PlaylistSaved`].
    CreatePlaylist {
        title: String,
        track: Option<TrackId>,
    },
    /// Add a track to one of the person's playlists: at the end, or at this
    /// place (to undo a removal).
    AddToPlaylist {
        playlist: PlaylistId,
        track: TrackId,
        at: Option<usize>,
    },
    /// Remove the track at this place of one of the person's playlists.
    RemoveFromPlaylist {
        playlist: PlaylistId,
        index: usize,
    },
    MoveInPlaylist {
        playlist: PlaylistId,
        from: usize,
        to: usize,
    },
    RenamePlaylist {
        playlist: PlaylistId,
        title: String,
    },
    SetPlaylistPublic {
        playlist: PlaylistId,
        public: bool,
    },
    DeletePlaylist(PlaylistId),
    /// Host a Jam (ADR 0011): [`Event::Jam`] carries the link once online.
    /// The queue and playback become everyone's.
    StartJam,
    /// Join the Jam behind a `cloudrs:jam/` link (see [`is_jam_link`]). The
    /// queue mirrors the host's; queue and playback commands become
    /// requests to the host. The own queue comes back when the Jam ends.
    JoinJam(String),
    /// End the Jam (host) or leave it (guest).
    LeaveJam,
    /// Host: let guests play/pause, skip, seek and reorder, not only add.
    SetJamGuestsControl(bool),
    /// Host: remove a person (`JamPerson::id`) from the Jam.
    RemoveFromJam(u32),
}

/// Whether the text is a Jam link, to send it as [`Command::JoinJam`].
pub fn is_jam_link(text: &str) -> bool {
    sc_session::is_link(text)
}

/// What the UI renders.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A search for this query and kind started (after the debounce).
    Searching { query: String, kind: SearchKind },
    /// A page of a list. `append: false` replaces the list, `true` adds to it.
    List {
        list: ListId,
        items: ListItems,
        append: bool,
        has_more: bool,
    },
    /// The header of the track screen.
    TrackPage(TrackPage),
    /// The header of the profile screen.
    UserPage(UserPage),
    /// The header of the playlist or album screen.
    PlaylistPage(PlaylistPage),
    /// This track is now the current one (it may still be loading).
    NowPlaying(TrackSummary),
    /// The queue changed. Sent on every change.
    Queue(QueueSnapshot),
    /// Bars of the current track's waveform, 0.0 to 1.0.
    Waveform { track: TrackId, bars: Vec<f32> },
    /// An image (cover, avatar) is available at `path`.
    Artwork { key: ArtKey, path: PathBuf },
    /// Player state, sent on every change and about ten times per second while playing.
    Playback(Playback),
    /// A list page (`append: false` is the first page, `true` a "load more")
    /// could not be fetched. A failed `append` page is dropped: the core will
    /// not offer it again, so the UI should stop asking for more. A failed
    /// first page can be retried with `LoadMore`.
    ListFailed {
        list: ListId,
        append: bool,
        problem: Problem,
    },
    /// The core saved its state and stopped, after [`Command::Shutdown`].
    Stopped,
    /// Signed in, after [`Command::SignIn`] or with the token from
    /// [`CoreConfig::oauth_token`]. The app keeps `token` in the keychain.
    SignedIn(Account),
    /// Signed out, asked for or because the token expired. The app forgets
    /// the token.
    SignedOut,
    /// Every track the person liked, sent after signing in.
    LikedIds(Vec<TrackId>),
    /// Everyone the person follows, sent after signing in.
    FollowedIds(Vec<UserId>),
    /// A track's like changed (or a failed change was reverted).
    Liked { track: TrackId, liked: bool },
    /// A follow changed (or a failed change was reverted).
    Followed { user: UserId, following: bool },
    /// SoundCloud's own home rows, after [`Command::OpenHome`]: curated and
    /// chart playlists. Covers arrive as `Artwork`.
    HomeShelves(Vec<HomeShelf>),
    /// A playlist of the person changed on SoundCloud. Its page and list
    /// follow when it still exists; a created or deleted one also reloads the
    /// library.
    PlaylistSaved {
        playlist: PlaylistSummary,
        change: PlaylistChange,
    },
    /// The Jam changed (people, link, permissions); `None` once it is over.
    Jam(Option<JamState>),
    /// Something the person should know about (playing, pasted links, audio).
    Problem(Problem),
}

/// The UI's side of the core.
pub struct CoreHandle {
    commands: flume::Sender<Command>,
    events: flume::Receiver<Event>,
}

impl CoreHandle {
    /// Sends a command. Returns `false` if the core has stopped.
    pub fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }

    /// Events in order. The UI awaits them with `recv_async`.
    pub fn events(&self) -> &flume::Receiver<Event> {
        &self.events
    }
}

/// Settings the composition root decides.
#[derive(Clone)]
pub struct CoreConfig {
    /// Folder for cached artwork, created if missing.
    pub cache_dir: PathBuf,
    /// Folder for the session and history database, created if missing.
    pub data_dir: PathBuf,
    /// The token saved in the keychain, checked with SoundCloud at start.
    pub oauth_token: Option<String>,
    /// How Jam peers reach each other: the internet for the app, this
    /// machine only for tests.
    pub jam_network: JamNetwork,
}

pub use sc_session::Network as JamNetwork;

/// Hides the token so it never reaches a log.
impl std::fmt::Debug for CoreConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreConfig")
            .field("cache_dir", &self.cache_dir)
            .field("data_dir", &self.data_dir)
            .field("jam_network", &self.jam_network)
            .field(
                "oauth_token",
                &self.oauth_token.as_ref().map(|_| "<hidden>"),
            )
            .finish()
    }
}

/// Starts the core with the real SoundCloud client and the default audio device.
pub fn start(config: CoreConfig) -> Result<CoreHandle, StartError> {
    let api = sc_api::ScClient::new(sc_api::ClientConfig::default())?;
    let audio = sc_audio::Player::spawn()?.into_channels();
    Ok(spawn(api, audio, config))
}

/// Starts the core with any API and any audio link (tests use fakes).
pub fn spawn<A>(
    api: A,
    audio: (
        flume::Sender<sc_audio::Command>,
        flume::Receiver<sc_audio::Event>,
    ),
    config: CoreConfig,
) -> CoreHandle
where
    A: sc_api::SoundCloudApi + 'static,
{
    let (commands, command_rx) = flume::unbounded();
    let (event_tx, events) = flume::unbounded();
    core::run_on_thread(api, audio, config, command_rx, event_tx);
    CoreHandle { commands, events }
}

/// Why the core could not start.
#[derive(Debug)]
pub enum StartError {
    /// The HTTP client could not be built.
    Network(String),
    /// No usable audio output device.
    Audio(String),
}

impl From<sc_api::Error> for StartError {
    fn from(error: sc_api::Error) -> Self {
        Self::Network(error.to_string())
    }
}

impl From<sc_audio::Error> for StartError {
    fn from(error: sc_audio::Error) -> Self {
        Self::Audio(error.to_string())
    }
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(error) => write!(f, "network setup failed: {error}"),
            Self::Audio(error) => write!(f, "audio setup failed: {error}"),
        }
    }
}

impl std::error::Error for StartError {}
