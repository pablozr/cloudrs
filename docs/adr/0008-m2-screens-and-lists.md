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
