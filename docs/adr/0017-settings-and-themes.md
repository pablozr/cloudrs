# ADR 0017: settings, themes, language and the Discord choice

Status: accepted · 2026-10-07 (approved by the maintainer)

## Context

M4 asks for a Settings screen, light and dark themes remembered between runs, and a language
picker. Until now the theme was a global set to dark at start and flipped by the title bar
without being saved, the language was a fixed constant, and the Discord choice was an empty
file (`discord-off`) in the data folder. `sc-core` already owns the SQLite file (ADR 0007).

## Decisions

1. **A typed `Settings` in `sc-core`.** `Settings { theme: ThemeChoice, language: Language,
   discord: bool }` is `Debug + Clone + PartialEq + Eq`, deliberately not `Copy`, so later
   slices can add fields such as an output device name. The default is `{ System, English,
   discord: true }` (Discord on by default follows ADR 0015). The enum is `ThemeChoice`
   (`System`, `Dark`, `Light`), so it does not collide with `cloudrs_ui::ThemeMode`.
2. **One row in schema version 3.** Table `settings (id = 1, theme, language, discord)`, like
   `session`. The theme is stored as a code (0 system, 1 dark, 2 light; unknown reads as
   system) and the language as its tag (`en`; unknown reads as the default), so a file written
   by a later build still opens.
3. **Read synchronously before the window.** `sc_core::read_settings(data_dir)` opens the
   database on the calling thread and returns the settings, or the defaults when there are
   none or the file cannot be read (a damaged or newer file is left to the core, which resets
   it and reports `Problem::StorageReset`). It costs one SQLite open, about 1-3 ms against
   the 300 ms start-up target, and means the first frame already has the right theme.
4. **One command, one event.** `Command::SetSettings(Settings)` carries the whole struct; the
   UI is the only writer and sends the changed snapshot. The core always answers
   `Event::Settings(Settings)` with what is in effect, and the UI applies effects (theme,
   language, Discord) only from that event, along a single path. The write happens off the
   actor loop with a sequence number, like the session, and `Shutdown` writes it last.
   `CoreConfig::settings` hands the core what the app read.

## Consequences

- A new setting is a field, a column and a line in the UI; the command and event do not change.
- Every change rewrites the single row, which is tiny and rare.
- Settings made before the database opened are written as soon as it does.
