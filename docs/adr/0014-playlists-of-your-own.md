# ADR 0014: Playlists of your own

Status: accepted · 2026-10-07 (options chosen by the maintainer)

## Context

Signed in, people want to make playlists in cloudrs: create one, add tracks from anywhere,
and keep it in order. `api-v2` has no per-track endpoint; soundcloud.com's own web app sends
the whole list on every change. Its bundles show the calls; none could be tried live without
an account (the maintainer tries them).

## Decisions

1. **Endpoints** (from soundcloud.com's web app, verified only against mocks):
   - create: `POST /playlists` with `{"playlist": {"title", "sharing", "tracks": [ids]}}`;
   - change: `PUT /playlists/{id}` with `{"playlist": {...}}`, sending only what changes
     (`title`, `sharing`, or the whole `tracks` list);
   - delete: `DELETE /playlists/{id}`.
2. **No lost updates.** Every change to the tracks first reads the current list
   (`GET /playlists/{id}`), applies the change and sends the whole list. Adding a track already
   there sends nothing and answers `AlreadyThere`.
3. **New playlists are private.** The owner makes one public from its page.
4. **Adding:** a menu on track rows and on the track page, with "New playlist…" on top and the
   person's own playlists under it. Needs a menu (popover) primitive in `cloudrs-ui`, which the
   visual identity already plans (floating, with shadow).
5. **On their own playlist's page the owner can:**
   - remove a track (with an Undo toast);
   - reorder by dragging, as in the queue;
   - rename it and switch it between public and private;
   - delete it (after a confirmation).
6. **Core contract:**
   - `Command::{CreatePlaylist { title, track }, AddToPlaylist, RemoveFromPlaylist,
     MoveInPlaylist, RenamePlaylist, SetPlaylistPublic, DeletePlaylist}`;
   - `Event::PlaylistSaved { playlist, change: PlaylistChange }`, then the page and list again
     (a created or deleted one reloads the library);
   - `Problem::PlaylistNotSaved`;
   - `PlaylistPage::public`.

   Signed out, these answer `SignInRequired`. A refused token signs out as for likes.

## Consequences

- Two calls per track change (read, then write). That is cheap for something done by hand.
- A playlist over SoundCloud's 500-track limit is refused by SoundCloud and shows
  `PlaylistNotSaved`.
