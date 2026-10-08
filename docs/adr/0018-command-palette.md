# ADR 0018: the command palette

Status: accepted · 2026-10-07 (approved by the maintainer)

## Context

`AGENTS.md` asks for every screen to be reachable from a command palette. M4 builds it. The
maintainer chose to keep `Ctrl K` (and `/`) for the search field, so the palette needs another
key.

## Decisions

1. **`Ctrl P` (`Cmd P` on macOS) opens it.** `Ctrl K` and `/` still focus search. The key
   toggles the palette, so pressing it again closes it.
2. **The surface is in the kit, the state is in the app.** `cloudrs_ui::palette` has plain
   functions: `palette` (the overlay, like `dialog`, with the field on top and a list), `palette_row`
   (a menu row with the keyboard selection and the key that does the same) and `matches` (the
   filter). The items, the typed text, the selection and what a row does live in
   `apps/cloudrs/src/shell/palette.rs`. No new crate and no trait.
3. **Items.** Every screen without parameters (Home, Search, History, Jam, Account, Settings;
   Feed, Likes, Library and Following with an account) and the actions that do not need a
   screen: play or pause, previous, next and like (with a current track), show or hide the
   queue, and the three theme choices. A test lists the routes so a new screen is not
   forgotten.
4. **Filter.** Each typed word must be a substring of the label, ignoring case. About twenty
   items, so there is no fuzzy search, no dependency and no virtual list. Filtering runs when
   the field changes, never in `render`.
5. **Keys.** Up, Down and Enter act in the context `CommandPalette`; Escape closes it from the
   field (`CommandPalette > SearchField`) and from the list. The field binds Escape to "clear
   the text" in its own context, and in gpui the deepest matching context wins, then the
   binding registered last, so the palette's bindings are registered after
   `search_field::bind_keys`. Space and the arrows of the playback shortcuts are bound outside
   `SearchField`, so they type inside the palette.
6. **Behaviour.** A click on the dim area closes it. Choosing a row closes it first and then
   runs the command; focus goes back to the window's root. It does not open over a dialog.

## Consequences

- `AGENTS.md` and the plan say `Ctrl P` for the palette.
- If `shell::bind_keys` ever runs before `search_field::bind_keys`, Escape goes back to only
  clearing the field.
- SoundCloud search results in the palette are out of scope.
