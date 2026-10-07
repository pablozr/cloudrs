# ADR 0013: Home, the Library and the app's own title bar

Status: accepted · 2026-10-07 (asked for by the maintainer for 0.1.0 beta 1)

## Context

Before the first beta the maintainer asked for a beautiful Home, playlists that are one click
away, a livelier Library and a title bar of the app's own with the logo. The approved design
preview already draws a Home (greeting, filter pills, cards, "Your playlists" in the sidebar).
A research pass (2026-10-07) checked live which `api-v2` endpoints soundcloud.com's own home
uses.

## Decisions

1. **Home is where cloudrs opens** (first sidebar item, the router's start). The search field
   no longer takes the focus at start; `Ctrl K` and `/` still do.
2. **What Home shows, top to bottom:**
   - a greeting with the person's name;
   - a highlight: what plays, else the last played, else the top trending track, on a wash of
     its cover's colour;
   - quick tiles to play right away: last played, then likes (whole rows of three only);
   - your playlists;
   - "Trending on SoundCloud" with genre pills;
   - recently played;
   - new from people you follow, liked tracks;
   - SoundCloud's own rows and charts;
   - artists you follow.

   A shelf with nothing to show is left out; one on its way shows skeleton cards.
3. **Endpoints (verified live, signed out):**
   - `/mixed-selections` gives SoundCloud's home rows. Only rows of three or more regular
     playlists are kept; system playlists open differently and wait.
   - `/charts/selections` gives the genre charts as playlists.
   - `/system-playlists/soundcloud:system-playlists:trending-by-genre:<slug>` gives trending by
     genre. Its tracks carry only ids and are filled with one `/tracks?ids=` call, kept in the
     ranking order.
   - `/charts?kind=top|new`, genre-specific `/charts`, `/featured_tracks/top`, `/selections`,
     `/stations` and `/personalized-tracks` are dead and not used.
4. **Core contract:** `ListId::Trending(Genre)` (twelve genres with verified slugs),
   `Command::OpenHome` → `Event::HomeShelves(Vec<HomeShelf>)`.
5. **New `cloudrs-ui` primitives:**
   - `card` and `skeleton_card`: square cover inside the card, so the hover follows it; the
     play button shows on hover;
   - `hero`, `quick_tile`, `sidebar_collection`;
   - `window_button`.

   Artwork images round their own corners, since a parent's corners do not clip them.
6. **Playlists at hand:**
   - The sidebar lists every playlist of the library under "Your playlists", the open one in
     the accent.
   - The Library is a grid of large covers with All, Playlists and Albums filters.
7. **The app's own title bar** (`appears_transparent`, client decorations on Linux).
   - **Layout:** the header becomes the title bar. Navigation, search and the theme toggle
     share it with minimize, maximize/restore and close (Lucide icons; close turns red on
     hover). The sidebar's logo block is the same height.
   - **Windows:** the bar and the logo are `WindowControlArea::Drag`, and the buttons are
     `Min`/`Max`/`Close` areas, so the system keeps its own behaviour (double-click to
     maximize, Snap layouts on maximize). Clickable things inside `occlude` the drag area.
   - **Linux:** the bar asks the window to move, and the buttons call the window.
   - **macOS:** keeps its traffic lights, placed over the sidebar's top.
8. **Greeting:** "Welcome back, <name>". "Good morning/evening" needs the local hour, which
   the standard library cannot give; that would be a new dependency (`time`), left for the
   maintainer.

## Consequences

- Three more calls when Home first shows (home rows, charts, one trending genre), then one
  per genre picked; the account's lists load as before.
- The title bar is ours on Windows and Linux. Window resizing at the edges stays the
  platform's (GPUI handles the hit test).
- Not yet captured in the light theme or on Linux and macOS.

## Later decision

- The maintainer approved `time` (0.3, `local-offset`, already in the lock through other
  crates) for the greeting: "Good morning" from 5:00, "Good afternoon" from 12:00, "Good
  evening" from 18:00, with the person's name when signed in. Where the system will not give
  the local hour, it stays "Welcome back".
