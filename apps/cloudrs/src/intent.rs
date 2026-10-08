//! What the person asked for in the browsing screens. The shell turns each
//! intent into a route change, a model update and a core command, so screens
//! never talk to the core.

use sc_core::{ListId, PlaylistId, SearchKind, TrackId, UserId};

use crate::nav::Route;

#[derive(Debug, Clone, PartialEq)]
pub enum UiIntent {
    /// The search text changed (debounced by the core).
    Search(String),
    SetSearchKind(SearchKind),
    /// A pasted soundcloud.com link.
    OpenUrl(String),
    OpenTrack(TrackId),
    OpenUser(UserId),
    OpenPlaylist(PlaylistId),
    OpenHistory,
    /// Request the first page of a list that was not requested yet (a
    /// profile tab), or again after an error.
    OpenList(ListId),
    /// Play `track` with the queue becoming `list`.
    Play {
        list: ListId,
        track: TrackId,
    },
    PlayNext(TrackId),
    AddToQueue(TrackId),
    LoadMore(ListId),
    /// Sign in with a pasted token (the window flow sends the token it got).
    SignIn(String),
    SignOut,
    Like {
        track: TrackId,
        liked: bool,
    },
    Follow {
        user: UserId,
        following: bool,
    },
    /// Host a Jam, or join one from its link: both open the Jam screen.
    StartJam,
    JoinJam(String),
    LeaveJam,
    SetJamGuestsControl(bool),
    RemoveFromJam(u32),
}

impl UiIntent {
    /// What pasted text asks for when it is a Jam link or a soundcloud.com
    /// link; any other text is not a link.
    pub fn from_link(text: &str) -> Option<Self> {
        let text = text.trim();
        if sc_core::is_jam_link(text) {
            Some(Self::JoinJam(text.to_owned()))
        } else if crate::state::is_soundcloud_url(text) {
            Some(Self::OpenUrl(text.to_owned()))
        } else {
            None
        }
    }

    /// The screen this intent leads to, if it opens one.
    pub fn route(&self) -> Option<Route> {
        match self {
            Self::OpenUrl(url) => Some(Route::Resolving(url.clone())),
            Self::OpenTrack(id) => Some(Route::Track(*id)),
            Self::OpenUser(id) => Some(Route::User(*id)),
            Self::OpenPlaylist(id) => Some(Route::Playlist(*id)),
            Self::OpenHistory => Some(Route::History),
            Self::StartJam | Self::JoinJam(_) => Some(Route::Jam),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_intents_lead_to_their_screens() {
        assert_eq!(
            UiIntent::OpenTrack(TrackId(3)).route(),
            Some(Route::Track(TrackId(3)))
        );
        assert_eq!(
            UiIntent::OpenUser(UserId(4)).route(),
            Some(Route::User(UserId(4)))
        );
        assert_eq!(
            UiIntent::OpenPlaylist(PlaylistId(5)).route(),
            Some(Route::Playlist(PlaylistId(5)))
        );
        assert_eq!(UiIntent::OpenHistory.route(), Some(Route::History));
        assert_eq!(
            UiIntent::OpenUrl("https://soundcloud.com/a".into()).route(),
            Some(Route::Resolving("https://soundcloud.com/a".into()))
        );
    }

    #[test]
    fn playing_and_paging_do_not_change_screen() {
        let list = ListId::History;
        for intent in [
            UiIntent::Search("x".into()),
            UiIntent::Play {
                list,
                track: TrackId(1),
            },
            UiIntent::PlayNext(TrackId(1)),
            UiIntent::LoadMore(list),
            UiIntent::OpenList(list),
        ] {
            assert_eq!(intent.route(), None, "{intent:?}");
        }
    }

    #[test]
    fn pasted_links_become_intents() {
        assert_eq!(
            UiIntent::from_link(" https://soundcloud.com/a/b "),
            Some(UiIntent::OpenUrl("https://soundcloud.com/a/b".into()))
        );
        let jam = format!("cloudrs:jam/endpoint{}", "a".repeat(60));
        assert_eq!(
            UiIntent::from_link(&jam),
            Some(UiIntent::JoinJam(jam.clone()))
        );
        assert_eq!(UiIntent::from_link("lofi beats"), None);
        assert_eq!(UiIntent::from_link("https://example.com/x"), None);
    }
}
