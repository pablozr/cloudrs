# ADR 0025: Brazilian Portuguese

Status: accepted · 2026-10-08 (options chosen by the maintainer)

Supersedes the part of [ADR 0017](./0017-settings-and-themes.md) that keeps the language picker
hidden until a second language exists.

## Context

The interface text already goes through `i18n` (`strings!` and `formats!`), and the language is a
saved setting. M5 asks for the first translation beyond English, and the maintainer's first
language is Brazilian Portuguese.

## Decisions

7. **`Language::PtBr`**, tag `"pt-BR"`. The settings table keeps the language as TEXT, so there is
   no migration. The picker in Settings shows one pill per language, each in its own name
   ("English", "Português (Brasil)"), right after the theme.
8. **The system language on a first run only.** `sc_core::read_saved_settings` returns `Ok(None)`
   when nothing is saved, and only then `sys-locale` 0.3 in the app picks the language, with any
   `pt*` locale mapping to `PtBr` and anything else to English. A saved choice always wins. When
   the database cannot be read (`Err`: locked by another instance past the busy timeout, damaged,
   or newer), the app starts with the defaults (English), as before this ADR, not the system
   language. If the read failed and the core later opens the store, the next settings change
   saves the whole in-memory `Settings` over the saved ones (true since ADR 0017). `sys-locale`
   is already in the lock through `cosmic-text`, so no crate is added. Only the pure mapping
   (`language_for_locale`) is unit-tested, not the call into the OS.
9. **Numbers follow the language.** `i18n::number::compact` gives "1,2 mil", "12 mil", "3,4 mi" in
   Portuguese (and "1.2K", "12K", "3.4M" in English as before), and `one_decimal` uses a decimal
   comma, which the cache size in Settings shows. They replace `state::compact_count`.
10. **How the texts landed.** The macros first took an optional `pt_br:` (transition), the
    translations came in three commits (navigation and settings; the player and problems; Jam,
    Home, Library and playlists), and the language arrived with `pt_br:` mandatory, so a new text
    without a translation does not compile. The translations are the assistant's; the maintainer
    reviews them (PLAN M5).

Changing the language takes effect at once. Text drawn in `render` follows by itself; the text that
widgets keep is written again by `Shell::relabel`: the placeholders and hint of the search,
playlist name, palette and token fields and of the playlist form, the window titles of the main
window and the mini player, and the tray menu. For that, `cloudrs-ui`'s `SearchField` gains one
small method, `set_texts(placeholder, hint)`.

## Consequences

- Every string now has two texts; Portuguese runs about 30% longer, so truncation in buttons,
  pills, tooltips and the mini player must be checked by eye.
- Content (titles, artists, descriptions, comments) is never translated.
- Not verified by running it: no screenshots in either theme or language, and the live switch is
  untried. The maintainer reviews the texts and the screens (PLAN M5).
- Discord presence texts follow the language at the moment the presence is sent.
