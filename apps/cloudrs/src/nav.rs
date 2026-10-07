//! Where the person is, and the back/forward history (ADR 0008 §10). Plain
//! data: no GPUI types.

use sc_core::{ListId, PlaylistId, TrackId, UserId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Where the app opens: greeting, shelves of what to play next.
    Home,
    Search,
    Track(TrackId),
    User(UserId),
    Playlist(PlaylistId),
    History,
    /// The signed-in person's feed, likes, library and followings.
    Feed,
    Likes(UserId),
    Library,
    Following(UserId),
    /// Sign in, or the signed-in account and "Sign out".
    Account,
    /// Start, share or leave a Jam.
    Jam,
    /// A pasted link the core is still resolving: shows a loading page until
    /// the core answers with the screen to open.
    Resolving(String),
}

/// The sidebar entry a route belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Home,
    Search,
    History,
    Feed,
    Likes,
    Library,
    Following,
    Jam,
    Account,
}

impl Route {
    /// Track, profile and playlist pages are reached by searching, so they
    /// keep Search lit in the sidebar.
    pub fn section(&self) -> Section {
        match self {
            Self::Home => Section::Home,
            Self::History => Section::History,
            Self::Feed => Section::Feed,
            Self::Likes(_) => Section::Likes,
            Self::Library => Section::Library,
            Self::Following(_) => Section::Following,
            Self::Account => Section::Account,
            Self::Jam => Section::Jam,
            _ => Section::Search,
        }
    }

    /// The list an account screen shows.
    pub fn account_list(&self) -> Option<ListId> {
        match self {
            Self::Feed => Some(ListId::Feed),
            Self::Likes(me) => Some(ListId::UserLikes(*me)),
            Self::Library => Some(ListId::Library),
            Self::Following(me) => Some(ListId::Followings(*me)),
            _ => None,
        }
    }
}

/// The routes visited, and which one is showing.
#[derive(Debug)]
pub struct Router {
    stack: Vec<Route>,
    at: usize,
}

impl Router {
    pub fn new() -> Self {
        Self {
            stack: vec![Route::Home],
            at: 0,
        }
    }

    pub fn current(&self) -> &Route {
        &self.stack[self.at]
    }

    pub fn can_back(&self) -> bool {
        self.at > 0
    }

    pub fn can_forward(&self) -> bool {
        self.at + 1 < self.stack.len()
    }

    /// Goes to `route`, dropping whatever was ahead. Returns whether the
    /// route changed (going where the person already is does nothing).
    pub fn push(&mut self, route: Route) -> bool {
        if *self.current() == route {
            return false;
        }
        self.stack.truncate(self.at + 1);
        self.stack.push(route);
        self.at += 1;
        true
    }

    /// A pasted link was answered: its loading step is dropped, and the
    /// screen it leads to (if any) takes its place.
    pub fn resolve(&mut self, route: Option<Route>) {
        if !matches!(self.current(), Route::Resolving(_)) {
            return;
        }
        // The loading step is never the first one: the router starts on Home.
        self.stack.truncate(self.at);
        self.at -= 1;
        if let Some(route) = route {
            self.push(route);
        }
    }

    pub fn back(&mut self) -> bool {
        if !self.can_back() {
            return false;
        }
        self.at -= 1;
        true
    }

    pub fn forward(&mut self) -> bool {
        if !self.can_forward() {
            return false;
        }
        self.at += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u64) -> Route {
        Route::User(UserId(id))
    }

    #[test]
    fn it_starts_on_home_with_nowhere_to_go() {
        let router = Router::new();
        assert_eq!(*router.current(), Route::Home);
        assert!(!router.can_back() && !router.can_forward());
    }

    #[test]
    fn back_and_forward_walk_the_history() {
        let mut router = Router::new();
        router.push(user(1));
        router.push(Route::History);

        assert!(router.back());
        assert_eq!(*router.current(), user(1));
        assert!(router.back());
        assert_eq!(*router.current(), Route::Home);

        assert!(router.forward());
        assert!(router.forward());
        assert_eq!(*router.current(), Route::History);
    }

    #[test]
    fn back_and_forward_stay_within_bounds() {
        let mut router = Router::new();
        assert!(!router.back());
        assert!(!router.forward());

        router.push(user(1));
        assert!(!router.forward(), "nothing ahead");
        assert!(router.back());
        assert!(!router.back(), "already at the start");
        assert_eq!(*router.current(), Route::Home);
    }

    #[test]
    fn pushing_drops_the_forward_steps() {
        let mut router = Router::new();
        router.push(user(1));
        router.push(user(2));
        router.back();
        assert!(router.can_forward());

        router.push(Route::History);
        assert!(!router.can_forward());
        router.back();
        assert_eq!(*router.current(), user(1), "user 2 is gone");
    }

    #[test]
    fn going_where_you_already_are_adds_no_step() {
        let mut router = Router::new();
        assert!(!router.push(Route::Home));
        assert!(router.push(user(1)));
        assert!(!router.push(user(1)));
        assert!(router.back());
        assert_eq!(*router.current(), Route::Home);
    }

    #[test]
    fn a_resolved_link_replaces_its_loading_step() {
        let mut router = Router::new();
        router.push(Route::Resolving("https://soundcloud.com/a".into()));
        router.resolve(Some(user(5)));
        assert_eq!(*router.current(), user(5));
        assert!(router.back());
        assert_eq!(*router.current(), Route::Home);
        assert!(!router.back(), "no extra step was left behind");
    }

    #[test]
    fn a_link_with_no_screen_returns_to_where_it_came_from() {
        let mut router = Router::new();
        router.push(user(1));
        router.push(Route::Resolving("https://soundcloud.com/a".into()));
        router.resolve(None);
        assert_eq!(*router.current(), user(1));
        assert!(!router.can_forward(), "the loading step is gone");

        router.resolve(Some(Route::History));
        assert_eq!(*router.current(), user(1), "only a loading step resolves");
    }

    #[test]
    fn account_screens_have_their_section_and_list() {
        let me = UserId(9);
        assert_eq!(Route::Feed.account_list(), Some(ListId::Feed));
        assert_eq!(Route::Likes(me).section(), Section::Likes);
        assert_eq!(Route::Likes(me).account_list(), Some(ListId::UserLikes(me)));
        assert_eq!(
            Route::Following(me).account_list(),
            Some(ListId::Followings(me))
        );
        assert_eq!(Route::Library.section(), Section::Library);
        assert_eq!(Route::Account.section(), Section::Account);
        assert_eq!(Route::Account.account_list(), None);
    }

    #[test]
    fn pages_belong_to_a_sidebar_section() {
        assert_eq!(Route::History.section(), Section::History);
        for route in [Route::Search, user(1), Route::Resolving(String::new())] {
            assert_eq!(route.section(), Section::Search);
        }
    }
}
