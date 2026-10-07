//! The core's own types: what the UI sees instead of `sc-api` models.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlaylistId(pub u64);

/// What a search looks for (the tabs of the search screen).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SearchKind {
    #[default]
    Tracks,
    People,
    Playlists,
    Albums,
}

/// Every list the core serves and pages (ADR 0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListId {
    Search {
        kind: SearchKind,
    },
    UserTracks(UserId),
    UserPlaylists(UserId),
    UserLikes(UserId),
    /// The people a user follows.
    Followings(UserId),
    /// The signed-in user's feed (tracks posted or reposted by people they follow).
    Feed,
    /// The signed-in user's playlists and albums, made or liked.
    Library,
    /// What trends on SoundCloud in a genre (Home).
    Trending(Genre),
    Playlist(PlaylistId),
    Related(TrackId),
    History,
}

/// The rows of one list page.
#[derive(Debug, Clone, PartialEq)]
pub enum ListItems {
    Tracks(Vec<TrackSummary>),
    Users(Vec<UserSummary>),
    Playlists(Vec<PlaylistSummary>),
}

/// An image the core caches on disk and announces with `Event::Artwork`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtKey {
    Track(TrackId),
    User(UserId),
    Playlist(PlaylistId),
}

/// What a list row or the player bar needs to show a track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSummary {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    /// The uploader, to open their profile. `None` for tracks restored from
    /// a saved queue or the history.
    pub artist_id: Option<UserId>,
    pub duration: Duration,
    /// SoundCloud only allows a 30-second preview (GO+).
    pub preview_only: bool,
}

impl TrackSummary {
    pub(crate) fn from_api(track: &sc_api::models::Track) -> Self {
        Self {
            id: TrackId(track.id),
            title: track.title.clone(),
            artist: track
                .user
                .as_ref()
                .map(|user| user.username.clone())
                .unwrap_or_default(),
            artist_id: track
                .user
                .as_ref()
                .filter(|user| user.id != 0)
                .map(|user| UserId(user.id)),
            duration: Duration::from_millis(track.full_duration.unwrap_or(track.duration)),
            preview_only: track.is_preview_only(),
        }
    }
}

/// A person in a search result.
#[derive(Debug, Clone, PartialEq)]
pub struct UserSummary {
    pub id: UserId,
    pub username: String,
    pub followers: Option<u64>,
    pub track_count: Option<u64>,
    pub verified: bool,
}

impl UserSummary {
    pub(crate) fn from_api(user: &sc_api::models::User) -> Self {
        Self {
            id: UserId(user.id),
            username: user.username.clone(),
            followers: user.followers_count,
            track_count: user.track_count,
            verified: user.verified.unwrap_or(false),
        }
    }
}

/// A playlist or an album in a list.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistSummary {
    pub id: PlaylistId,
    pub title: String,
    pub owner: String,
    pub owner_id: Option<UserId>,
    pub track_count: u64,
    pub is_album: bool,
}

impl PlaylistSummary {
    pub(crate) fn from_api(playlist: &sc_api::models::Playlist) -> Self {
        Self {
            id: PlaylistId(playlist.id),
            title: playlist.title.clone(),
            owner: owner_name(playlist),
            owner_id: owner_id(playlist),
            track_count: playlist.track_count.unwrap_or(playlist.tracks.len() as u64),
            is_album: is_album(playlist),
        }
    }
}

fn is_album(playlist: &sc_api::models::Playlist) -> bool {
    playlist.is_album.unwrap_or(false) || playlist.set_type.as_deref() == Some("album")
}

fn owner_name(playlist: &sc_api::models::Playlist) -> String {
    playlist
        .user
        .as_ref()
        .map(|user| user.username.clone())
        .unwrap_or_default()
}

fn owner_id(playlist: &sc_api::models::Playlist) -> Option<UserId> {
    playlist
        .user
        .as_ref()
        .filter(|user| user.id != 0)
        .map(|user| UserId(user.id))
}

/// The header of the track screen. The cover arrives as `Event::Artwork`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPage {
    pub track: TrackSummary,
    pub description: Option<String>,
    pub plays: Option<u64>,
    pub likes: Option<u64>,
    pub comments: Option<u64>,
    /// As SoundCloud sends it (ISO 8601), for the UI to format.
    pub created_at: Option<String>,
    pub permalink: String,
}

impl TrackPage {
    pub(crate) fn from_api(track: &sc_api::models::Track) -> Self {
        Self {
            track: TrackSummary::from_api(track),
            description: track.description.clone().filter(|text| !text.is_empty()),
            plays: track.playback_count,
            likes: track.likes_count,
            comments: track.comment_count,
            created_at: track.created_at.clone(),
            permalink: track.permalink_url.clone(),
        }
    }
}

/// The header of the profile screen. The avatar arrives as `Event::Artwork`.
#[derive(Debug, Clone, PartialEq)]
pub struct UserPage {
    pub id: UserId,
    pub username: String,
    pub full_name: Option<String>,
    pub city: Option<String>,
    pub description: Option<String>,
    pub followers: Option<u64>,
    pub followings: Option<u64>,
    pub track_count: Option<u64>,
    pub verified: bool,
}

impl UserPage {
    pub(crate) fn from_api(user: &sc_api::models::User) -> Self {
        let text = |value: &Option<String>| value.clone().filter(|text| !text.is_empty());
        Self {
            id: UserId(user.id),
            username: user.username.clone(),
            full_name: text(&user.full_name),
            city: text(&user.city),
            description: text(&user.description),
            followers: user.followers_count,
            followings: user.followings_count,
            track_count: user.track_count,
            verified: user.verified.unwrap_or(false),
        }
    }
}

/// The header of the playlist or album screen. The cover arrives as
/// `Event::Artwork`; the tracks as `Event::List` for `ListId::Playlist`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistPage {
    pub id: PlaylistId,
    pub title: String,
    pub owner: String,
    pub owner_id: Option<UserId>,
    pub track_count: u64,
    pub duration: Duration,
    pub is_album: bool,
    /// Public or private (only its owner sees a private one).
    pub public: bool,
}

impl PlaylistPage {
    pub(crate) fn from_api(playlist: &sc_api::models::Playlist) -> Self {
        Self {
            id: PlaylistId(playlist.id),
            title: playlist.title.clone(),
            owner: owner_name(playlist),
            owner_id: owner_id(playlist),
            track_count: playlist.track_count.unwrap_or(playlist.tracks.len() as u64),
            duration: Duration::from_millis(playlist.duration),
            is_album: is_album(playlist),
            public: playlist.sharing.as_deref() != Some("private"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlayState {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Ended,
}

/// The player bar's state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Playback {
    pub state: PlayState,
    pub position: Duration,
    pub duration: Duration,
    pub volume: f32,
}

/// Something the person should know. The UI turns each into text with i18n.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// SoundCloud could not be reached.
    Offline,
    /// SoundCloud asked us to slow down.
    RateLimited,
    /// The track, user or link does not exist or is private.
    NotFound,
    /// The pasted link is not a track, profile or playlist cloudrs can open.
    UnsupportedLink,
    /// Only a GO+ preview exists for this track.
    PreviewOnly,
    /// The track exists but cannot be played here (region, encrypted stream, format).
    CannotPlay,
    /// The audio engine failed; the detail is for logs, not for the UI.
    Audio(String),
    /// The saved session and history were damaged and have been reset.
    StorageReset,
    /// SoundCloud refused the token given to sign in.
    SignInFailed,
    /// SoundCloud no longer accepts the saved token: the person is signed out.
    SessionExpired,
    /// Liking or following needs an account.
    SignInRequired,
    /// A change to one of the person's playlists could not be saved.
    PlaylistNotSaved,
    /// The Jam could not start or its host could not be reached.
    JamUnreachable,
    /// The text is not a Jam link.
    JamBadLink,
    /// The host ended the Jam (or it was lost).
    JamEnded,
    /// The host removed this person from the Jam.
    JamRemoved,
    /// The Jam already has the most people it allows.
    JamFull,
    /// The host speaks another version of cloudrs.
    JamVersion,
    /// Only the host (or guests it allows) can do that in this Jam.
    JamNotAllowed,
}

/// The signed-in person. `Debug` hides the token so it never reaches a log.
#[derive(Clone, PartialEq)]
pub struct Account {
    pub user: UserSummary,
    /// For the app to keep in the OS keychain.
    pub token: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("user", &self.user)
            .field("token", &"<hidden>")
            .finish()
    }
}

impl Problem {
    pub(crate) fn from_api(error: &sc_api::Error) -> Self {
        use sc_api::Error as E;
        match error {
            E::Network(_) | E::ClientIdNotFound | E::Unauthorized => Self::Offline,
            E::RateLimited { .. } => Self::RateLimited,
            E::NotFound => Self::NotFound,
            E::NoPlayableStream("only a preview is available") => Self::PreviewOnly,
            E::NoPlayableStream(_) | E::GeoBlocked => Self::CannotPlay,
            E::Status(_) | E::Decode(_) => Self::Offline,
        }
    }
}

/// What happens when a track ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    /// Play the same track again.
    One,
    /// Go back to the first track after the last.
    All,
}

/// The queue as the UI shows it. The single source of truth for what plays next.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueSnapshot {
    pub tracks: Vec<TrackSummary>,
    /// Index into `tracks` of the track that is playing (or loaded).
    pub current: Option<usize>,
    pub shuffle: bool,
    pub repeat: Repeat,
}

/// A Jam in progress, as the UI shows it (ADR 0011). `None` in
/// `Event::Jam` means no Jam.
#[derive(Debug, Clone, PartialEq)]
pub struct JamState {
    pub role: JamRole,
    /// The link to share; the host has it once online, a guest never.
    pub link: Option<String>,
    /// Everyone but this person (for a guest, the other guests).
    pub people: Vec<JamPerson>,
    /// Guests may play/pause, skip, seek and reorder, not only add.
    pub guests_control_playback: bool,
    /// Still going online (host) or connecting (guest).
    pub connecting: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JamRole {
    Host,
    /// `host` is the host's name, once it said hello.
    Guest {
        host: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct JamPerson {
    pub id: u32,
    pub name: String,
    /// Their SoundCloud account, for the avatar (`ArtKey::User`); `None`
    /// for someone not signed in.
    pub user: Option<UserId>,
    /// The host of the Jam (a guest sees the host in the list too).
    pub host: bool,
    /// Cannot play the current track with their account (preview, region).
    pub cannot_play: bool,
}

/// A genre of SoundCloud's trending playlists (Home's filter pills). The UI
/// names them; the core knows their system-playlist slug.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Genre {
    #[default]
    All,
    Electronic,
    House,
    HipHop,
    Dubstep,
    Ambient,
    Pop,
    Rock,
    Indie,
    Latin,
    RnB,
    Trap,
}

impl Genre {
    /// Every genre, in the order of the pills.
    pub const ALL: [Genre; 12] = [
        Genre::All,
        Genre::Electronic,
        Genre::House,
        Genre::HipHop,
        Genre::Dubstep,
        Genre::Ambient,
        Genre::Pop,
        Genre::Rock,
        Genre::Indie,
        Genre::Latin,
        Genre::RnB,
        Genre::Trap,
    ];

    /// `soundcloud:system-playlists:trending-by-genre:<slug>`.
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Genre::All => "all-genres",
            Genre::Electronic => "electronic",
            Genre::House => "house",
            Genre::HipHop => "hip-hop",
            Genre::Dubstep => "dubstep",
            Genre::Ambient => "ambient",
            Genre::Pop => "pop",
            Genre::Rock => "rock",
            Genre::Indie => "indie",
            Genre::Latin => "latin",
            Genre::RnB => "r-n-b",
            Genre::Trap => "trap",
        }
    }
}

/// A row of SoundCloud's own home ("Artists to watch out for", the charts):
/// its title as SoundCloud sends it, over playlists.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeShelf {
    pub title: String,
    pub playlists: Vec<PlaylistSummary>,
}

/// What changed on a playlist of the signed-in person (`Event::PlaylistSaved`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistChange {
    Created,
    Added(TrackId),
    /// The track was already there: nothing changed.
    AlreadyThere(TrackId),
    Removed,
    Moved,
    Renamed,
    Privacy {
        public: bool,
    },
    Deleted,
}
