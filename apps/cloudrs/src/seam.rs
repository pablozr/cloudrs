//! The seam to the core: the only place that knows both the view models and
//! the core's contract. Events become model updates (`apply`); intents become
//! commands (`take`) after the models are told what to wait for.

use std::sync::Arc;

use sc_core::{ArtKey, Command, Event, ListId, PlayState, Problem};

use crate::intent::UiIntent;
use crate::models::{Models, Page, SEARCH_TABS};

/// Applies a core event. Returns whether anything the screens show changed,
/// so playback ticks never re-render them.
pub fn apply(models: &mut Models, event: &Event) -> bool {
    match event {
        Event::Searching { query, kind } => {
            models.query.clone_from(query);
            models.search_kind = *kind;
            remove_search_lists(models);
            models.begin(ListId::Search { kind: *kind });
            true
        }
        Event::List {
            list,
            items,
            append,
            has_more,
        } => {
            let view = models.list_mut(*list);
            if *append {
                view.append(items.clone(), *has_more);
            } else {
                view.replace(items.clone(), *has_more);
            }
            true
        }
        Event::ListFailed { list, append, .. } => {
            let Some(view) = models.lists.get_mut(list) else {
                return false;
            };
            view.fail(*append);
            true
        }
        Event::TrackPage(page) => {
            models
                .tracks
                .insert(page.track.id, Page::Ready(page.clone()));
            true
        }
        Event::UserPage(page) => {
            models.users.insert(page.id, Page::Ready(page.clone()));
            true
        }
        Event::PlaylistPage(page) => {
            models.playlists.insert(page.id, Page::Ready(page.clone()));
            true
        }
        Event::NowPlaying(track) => {
            models.current = Some(track.id);
            models.playing = false;
            true
        }
        Event::Playback(playback) => {
            let playing = playback.state == PlayState::Playing;
            std::mem::replace(&mut models.playing, playing) != playing
        }
        Event::Artwork { key, path } => {
            let path = Arc::from(path.as_path());
            match key {
                ArtKey::Track(id) => models.art.tracks.insert(*id, path),
                ArtKey::User(id) => models.art.users.insert(*id, path),
                ArtKey::Playlist(id) => models.art.playlists.insert(*id, path),
            };
            true
        }
        // A page still waiting for its header will not get one.
        Event::Problem(Problem::Offline | Problem::RateLimited | Problem::NotFound) => {
            models.fail_loading_pages()
        }
        // The player bar and the queue own these.
        Event::Problem(_) | Event::Waveform { .. } | Event::Queue(_) | Event::Stopped => false,
    }
}

/// The core command for an intent.
pub fn command(intent: &UiIntent) -> Command {
    match intent {
        UiIntent::Search(text) => Command::Search(text.clone()),
        UiIntent::SetSearchKind(kind) => Command::SetSearchKind(*kind),
        UiIntent::OpenUrl(url) => Command::OpenUrl(url.clone()),
        UiIntent::OpenTrack(id) => Command::OpenTrack(*id),
        UiIntent::OpenUser(id) => Command::OpenUser(*id),
        UiIntent::OpenPlaylist(id) => Command::OpenPlaylist(*id),
        UiIntent::OpenHistory => Command::OpenHistory,
        // The core serves a list it never sent by loading its first page.
        UiIntent::OpenList(list) | UiIntent::LoadMore(list) => Command::LoadMore(*list),
        UiIntent::Play { list, track } => Command::Play {
            list: *list,
            track: *track,
        },
        UiIntent::PlayNext(id) => Command::PlayNext(*id),
        UiIntent::AddToQueue(id) => Command::AddToQueue(*id),
    }
}

/// Tells the models what an intent is about to bring (skeletons for the pages
/// and lists the core will send), and returns the command to send.
pub fn take(models: &mut Models, intent: &UiIntent) -> Command {
    match intent {
        UiIntent::Search(text) => {
            models.query.clone_from(text);
            if text.is_empty() {
                remove_search_lists(models);
            }
        }
        UiIntent::SetSearchKind(kind) => {
            models.search_kind = *kind;
            // The core re-runs the query for the tab; until it says so, wait.
            let list = ListId::Search { kind: *kind };
            if !models.query.is_empty() && !models.lists.contains_key(&list) {
                models.begin(list);
            }
        }
        UiIntent::OpenTrack(id) => {
            models.expect_track(*id);
            expect_once(models, ListId::Related(*id));
        }
        UiIntent::OpenUser(id) => {
            models.expect_user(*id);
            expect_once(models, ListId::UserTracks(*id));
            // The core resets these when a profile opens; the tabs ask again.
            models.lists.remove(&ListId::UserPlaylists(*id));
            models.lists.remove(&ListId::UserLikes(*id));
        }
        UiIntent::OpenPlaylist(id) => {
            models.expect_playlist(*id);
            expect_once(models, ListId::Playlist(*id));
        }
        UiIntent::OpenHistory => models.begin(ListId::History),
        UiIntent::OpenList(list) => models.begin(*list),
        UiIntent::Play { .. }
        | UiIntent::PlayNext(_)
        | UiIntent::AddToQueue(_)
        | UiIntent::OpenUrl(_)
        | UiIntent::LoadMore(_) => {}
    }
    command(intent)
}

/// A new search invalidates the lists of every tab.
fn remove_search_lists(models: &mut Models) {
    for kind in SEARCH_TABS {
        models.lists.remove(&ListId::Search { kind });
    }
}

/// Begins a list unless it already has (or is getting) its first page.
fn expect_once(models: &mut Models, list: ListId) {
    if !models.lists.contains_key(&list) {
        models.begin(list);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use sc_core::{
        ListItems, Playback, PlaylistId, PlaylistPage, PlaylistSummary, SearchKind, TrackId,
        TrackPage, TrackSummary, UserId, UserPage, UserSummary,
    };

    use super::*;
    use crate::models::ListView;

    const SEARCH: ListId = ListId::Search {
        kind: SearchKind::Tracks,
    };

    fn track(id: u64) -> TrackSummary {
        TrackSummary {
            id: TrackId(id),
            title: format!("Track {id}"),
            artist: "Artist".into(),
            artist_id: Some(UserId(9)),
            duration: Duration::from_secs(200),
            preview_only: false,
        }
    }

    fn list_event(list: ListId, ids: &[u64], append: bool, has_more: bool) -> Event {
        Event::List {
            list,
            items: ListItems::Tracks(ids.iter().map(|id| track(*id)).collect()),
            append,
            has_more,
        }
    }

    fn failed(list: ListId, append: bool) -> Event {
        Event::ListFailed {
            list,
            append,
            problem: Problem::Offline,
        }
    }

    fn searching(query: &str, kind: SearchKind) -> Event {
        Event::Searching {
            query: query.into(),
            kind,
        }
    }

    fn view(models: &Models, list: ListId) -> &ListView {
        &models.lists[&list]
    }

    fn ids(models: &Models, list: ListId) -> Vec<u64> {
        let ListItems::Tracks(items) = &view(models, list).items else {
            panic!("not tracks");
        };
        items.iter().map(|track| track.id.0).collect()
    }

    fn playback(state: PlayState) -> Event {
        Event::Playback(Playback {
            state,
            ..Playback::default()
        })
    }

    #[test]
    fn a_first_page_replaces_and_a_next_page_is_appended() {
        let mut models = Models::new();
        assert!(apply(
            &mut models,
            &list_event(SEARCH, &[1, 2], false, true)
        ));
        assert_eq!(ids(&models, SEARCH), [1, 2]);
        assert!(view(&models, SEARCH).has_more);

        models.list_mut(SEARCH).loading_more = true;
        apply(&mut models, &list_event(SEARCH, &[3], true, false));
        assert_eq!(ids(&models, SEARCH), [1, 2, 3]);
        assert!(!view(&models, SEARCH).has_more && !view(&models, SEARCH).loading_more);

        apply(&mut models, &list_event(SEARCH, &[7], false, false));
        assert_eq!(ids(&models, SEARCH), [7], "a new first page replaces");
    }

    #[test]
    fn every_kind_of_list_is_kept_under_its_id() {
        let mut models = Models::new();
        let people = ListId::Search {
            kind: SearchKind::People,
        };
        let playlists = ListId::UserPlaylists(UserId(1));
        apply(
            &mut models,
            &Event::List {
                list: people,
                items: ListItems::Users(vec![UserSummary {
                    id: UserId(2),
                    username: "Ana".into(),
                    followers: Some(10),
                    track_count: None,
                    verified: false,
                }]),
                append: false,
                has_more: false,
            },
        );
        apply(
            &mut models,
            &Event::List {
                list: playlists,
                items: ListItems::Playlists(vec![PlaylistSummary {
                    id: PlaylistId(3),
                    title: "Mix".into(),
                    owner: "Ana".into(),
                    owner_id: None,
                    track_count: 4,
                    is_album: true,
                }]),
                append: false,
                has_more: false,
            },
        );
        assert_eq!(view(&models, people).len(), 1);
        assert_eq!(view(&models, playlists).len(), 1);
        assert!(!models.lists.contains_key(&ListId::History));
    }

    #[test]
    fn a_search_clears_the_other_tabs_and_shows_loading() {
        let mut models = Models::new();
        let people = ListId::Search {
            kind: SearchKind::People,
        };
        apply(&mut models, &list_event(people, &[], false, false));

        assert!(apply(&mut models, &searching("house", SearchKind::Tracks)));
        assert_eq!(models.query, "house");
        assert!(view(&models, SEARCH).loading);
        assert!(!models.lists.contains_key(&people));
    }

    #[test]
    fn a_failed_first_page_shows_the_error_and_a_failed_next_page_ends_the_list() {
        let mut models = Models::new();
        apply(&mut models, &searching("house", SearchKind::Tracks));
        assert!(apply(&mut models, &failed(SEARCH, false)));
        assert!(view(&models, SEARCH).error && !view(&models, SEARCH).loading);

        apply(&mut models, &list_event(SEARCH, &[1, 2], false, true));
        assert!(models.list_mut(SEARCH).take_load_more(2));
        assert!(apply(&mut models, &failed(SEARCH, true)));
        assert!(!view(&models, SEARCH).loading_more && !view(&models, SEARCH).has_more);
        assert_eq!(ids(&models, SEARCH), [1, 2], "the list stays in place");
    }

    #[test]
    fn a_failure_of_a_list_never_shown_changes_nothing() {
        let mut models = Models::new();
        assert!(!apply(&mut models, &failed(ListId::History, false)));
        assert!(models.lists.is_empty());
    }

    #[test]
    fn page_events_fill_the_headers() {
        let mut models = Models::new();
        let track_page = TrackPage {
            track: track(5),
            description: Some("hi".into()),
            plays: Some(10),
            likes: None,
            comments: None,
            created_at: None,
            permalink: "https://soundcloud.com/a/b".into(),
        };
        let user_page = UserPage {
            id: UserId(9),
            username: "Ana".into(),
            full_name: None,
            city: Some("Lisbon".into()),
            description: None,
            followers: Some(3),
            followings: None,
            track_count: Some(2),
            verified: false,
        };
        let playlist_page = PlaylistPage {
            id: PlaylistId(4),
            title: "Mix".into(),
            owner: "Ana".into(),
            owner_id: Some(UserId(9)),
            track_count: 3,
            duration: Duration::from_secs(600),
            is_album: false,
        };
        models.expect_track(TrackId(5));
        assert!(apply(&mut models, &Event::TrackPage(track_page.clone())));
        assert!(apply(&mut models, &Event::UserPage(user_page.clone())));
        assert!(apply(
            &mut models,
            &Event::PlaylistPage(playlist_page.clone())
        ));
        assert_eq!(models.tracks[&TrackId(5)], Page::Ready(track_page));
        assert_eq!(models.users[&UserId(9)], Page::Ready(user_page));
        assert_eq!(models.playlists[&PlaylistId(4)], Page::Ready(playlist_page));
    }

    #[test]
    fn a_problem_fails_pages_still_waiting_but_not_a_shown_one() {
        let mut models = Models::new();
        models.expect_user(UserId(1));
        assert!(!apply(&mut models, &Event::Problem(Problem::CannotPlay)));
        assert_eq!(models.users[&UserId(1)], Page::Loading);

        assert!(apply(&mut models, &Event::Problem(Problem::NotFound)));
        assert_eq!(models.users[&UserId(1)], Page::Failed);
        assert!(!apply(&mut models, &Event::Problem(Problem::NotFound)));
    }

    #[test]
    fn artwork_is_kept_by_what_it_shows() {
        let mut models = Models::new();
        let art = |key| Event::Artwork {
            key,
            path: PathBuf::from("/cache/x.jpg"),
        };
        assert!(apply(&mut models, &art(ArtKey::Track(TrackId(1)))));
        apply(&mut models, &art(ArtKey::User(UserId(2))));
        apply(&mut models, &art(ArtKey::Playlist(PlaylistId(3))));
        assert!(models.art.tracks.contains_key(&TrackId(1)));
        assert!(models.art.users.contains_key(&UserId(2)));
        assert!(models.art.playlists.contains_key(&PlaylistId(3)));
    }

    #[test]
    fn the_active_row_follows_the_player_without_ticks_re_rendering() {
        let mut models = Models::new();
        assert!(apply(&mut models, &Event::NowPlaying(track(2))));
        assert_eq!(models.current, Some(TrackId(2)));

        assert!(apply(&mut models, &playback(PlayState::Playing)));
        assert!(models.playing);
        assert!(!apply(&mut models, &playback(PlayState::Playing)), "tick");
        assert!(apply(&mut models, &playback(PlayState::Paused)));
        assert!(!models.playing);
    }

    #[test]
    fn intents_map_to_their_commands() {
        let (id, list) = (TrackId(4), ListId::Playlist(PlaylistId(2)));
        let cases = [
            (
                UiIntent::Search("x".into()),
                Command::Search("x".to_owned()),
            ),
            (
                UiIntent::SetSearchKind(SearchKind::People),
                Command::SetSearchKind(SearchKind::People),
            ),
            (
                UiIntent::OpenUrl("https://soundcloud.com/a".into()),
                Command::OpenUrl("https://soundcloud.com/a".to_owned()),
            ),
            (UiIntent::OpenTrack(id), Command::OpenTrack(id)),
            (UiIntent::OpenUser(UserId(1)), Command::OpenUser(UserId(1))),
            (
                UiIntent::OpenPlaylist(PlaylistId(2)),
                Command::OpenPlaylist(PlaylistId(2)),
            ),
            (UiIntent::OpenHistory, Command::OpenHistory),
            (UiIntent::OpenList(list), Command::LoadMore(list)),
            (UiIntent::LoadMore(list), Command::LoadMore(list)),
            (
                UiIntent::Play { list, track: id },
                Command::Play { list, track: id },
            ),
            (UiIntent::PlayNext(id), Command::PlayNext(id)),
            (UiIntent::AddToQueue(id), Command::AddToQueue(id)),
        ];
        for (intent, expected) in cases {
            assert_eq!(command(&intent), expected, "{intent:?}");
        }
    }

    #[test]
    fn opening_a_profile_waits_for_its_header_and_tracks_and_resets_its_tabs() {
        let mut models = Models::new();
        let user = UserId(7);
        take(&mut models, &UiIntent::OpenUser(user));
        assert!(models.users.contains_key(&user));
        assert!(view(&models, ListId::UserTracks(user)).loading);

        apply(
            &mut models,
            &list_event(ListId::UserLikes(user), &[1], false, false),
        );
        apply(
            &mut models,
            &list_event(ListId::UserTracks(user), &[1], false, false),
        );
        assert_eq!(
            take(&mut models, &UiIntent::OpenUser(user)),
            Command::OpenUser(user)
        );
        assert!(
            !view(&models, ListId::UserTracks(user)).loading,
            "a visited list is not reset by the page opening again"
        );
        assert!(
            !models.lists.contains_key(&ListId::UserLikes(user)),
            "the tabs ask again"
        );

        take(&mut models, &UiIntent::OpenHistory);
        assert!(view(&models, ListId::History).loading, "history refreshes");
    }

    #[test]
    fn a_search_intent_keeps_the_query_and_an_empty_one_clears_every_tab() {
        let mut models = Models::new();
        apply(&mut models, &list_event(SEARCH, &[1], false, false));
        take(&mut models, &UiIntent::Search("house".into()));
        assert_eq!(models.query, "house");
        assert!(models.lists.contains_key(&SEARCH));

        take(&mut models, &UiIntent::Search(String::new()));
        assert!(models.query.is_empty() && models.lists.is_empty());
    }

    #[test]
    fn a_search_tab_waits_for_its_results() {
        let mut models = Models::new();
        take(&mut models, &UiIntent::SetSearchKind(SearchKind::Albums));
        let albums = ListId::Search {
            kind: SearchKind::Albums,
        };
        assert_eq!(models.search_kind, SearchKind::Albums);
        assert!(!models.lists.contains_key(&albums), "nothing searched yet");

        models.query = "house".into();
        take(&mut models, &UiIntent::SetSearchKind(SearchKind::Albums));
        assert!(view(&models, albums).loading);
    }
}
