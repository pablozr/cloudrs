# ADR 0008: M2 — screens, lists and navigation

Status: accepted · 2026-10-06 (all options chosen by the maintainer)

## Context

The second half of M2 adds the Track, Profile, Playlist/Album and History screens, search
tabs and a sidebar. Every screen shows lists that page, play and feed the queue, so lists need
one shape in the core contract instead of one event per screen.

## Decisions

1. **Every list has a `ListId`:**
   `Search { kind }`, `UserTracks(UserId)`, `UserPlaylists(UserId)`, `UserLikes(UserId)`,
   `Playlist(PlaylistId)`, `Related(TrackId)`, `History`.
   `SearchKind` is `Tracks | People | Playlists | Albums`.
2. **One list event.** `Event::List { list, items, append, has_more }` where `items` is
   `ListItems::{Tracks(Vec<TrackSummary>), Users(Vec<UserSummary>), Playlists(Vec<PlaylistSummary>)}`;
   `Command::LoadMore(ListId)`; `Event::ListFailed { list, append, problem }`. They replace
   `Results`, `LoadMore` and `SearchFailed`. `Event::Searching` gains `kind`.
3. **Search tabs.** `Command::SetSearchKind(SearchKind)`; `Command::Search(String)` searches
   the current kind.
4. **Playing from any list.** `Command::Play { list, track }` replaces `Play(TrackId)`: the queue
   becomes that list starting at the track. `PlayNext` and `AddToQueue` accept any track the
   core has seen.
5. **Screens.** `OpenTrack(TrackId)` → `Event::TrackPage(TrackPage)`,
   `OpenUser(UserId)` → `Event::UserPage(UserPage)`,
   `OpenPlaylist(PlaylistId)` → `Event::PlaylistPage(PlaylistPage)`,
   `OpenHistory` → a `List` for `ListId::History`. Pages carry the header; their lists arrive as
   `List` events (a track page asks for `Related`, a profile for `UserTracks`, a playlist for
   `Playlist`, whose partial tracks the core fills with `/tracks?ids=`). Opening a track also
   sends its `Waveform`. Core-owned types: `UserId`, `PlaylistId`, `UserSummary`,
   `PlaylistSummary` (with `is_album`), `TrackPage`, `UserPage`, `PlaylistPage`.
6. **Pasted links.** `PlayUrl` becomes `OpenUrl(String)`: a track plays, a profile or playlist
   opens its screen (the core answers with the page event).
7. **Images.** `Event::Artwork { key, path }` with `ArtKey::{Track, User, Playlist}` covers
   track artwork, avatars and playlist covers.
8. **Profile tabs:** Tracks, Playlists, Likes. Reposts and comments come later (comments with the
   timed comments in M4); follow arrives with sign-in (M3).
9. **Sidebar:** Search and History, with the accent rail on the active item. Items that need an
   account arrive in M3.
10. **Navigation:** a back/forward stack in the UI only. Title opens the track, artist opens the
    profile, a playlist or album opens its list. Back/forward buttons in the header,
    `Alt ←`/`Alt →` and the mouse's back/forward buttons. Screen changes use `motion::PAGE`.

## Consequences

- One list component in the UI renders, pages and plays every list.
- The contract change touches the app's existing search and play paths.

## Refinements made while implementing the core

- `TrackSummary` gains `artist_id: Option<UserId>` and `PlaylistSummary`/`PlaylistPage` carry
  `owner_id`, so the UI can open a profile from a row (decision 10). Tracks restored from a
  saved queue or the history have `artist_id: None`.
- `Problem::NotATrack` is now `Problem::UnsupportedLink` (a link that is not a track, profile
  or playlist).
- Paging state lives per `ListId`. A new search, a tab change or re-opening a screen replaces
  that list's state with a new generation, so late pages are dropped. `LoadMore` on a list the
  core never served loads its first page. A failed first page can be retried with `LoadMore`;
  a failed next page ends the list. Opening a profile resets its playlists and likes lists.
- `SetSearchKind` re-runs the query for the new kind without the debounce; typing keeps the
  300 ms debounce.
- A playlist's list is sent once, with every track, after the partial ones were filled with
  `/tracks?ids=` in batches of 50. Tracks SoundCloud no longer returns are dropped.
- The history table (schema version 2) now stores duration, preview flag and cover URL, so
  `OpenHistory` lists each track once, newest first, up to 200. Rows written before version 2
  have a zero duration and no cover. If the database is not open yet the list is empty.
- `Event::Waveform` is also sent for the track whose page opens, not only the playing one.
- `/users/{id}/likes` is decoded as `Like { track }`; the core keeps the tracks and skips the
  rest.
