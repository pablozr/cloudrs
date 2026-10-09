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
   the 300 ms start-up target, and means the first frame already has the right theme. If that
   read fails, the core adopts what is saved once it opens the store (see Refinements).
4. **One command, one event.** `Command::SetSettings(Settings)` carries the whole struct; the
   UI is the only writer and sends the changed snapshot. The core always answers
   `Event::Settings(Settings)` with what is in effect, and the UI applies effects (theme,
   language, Discord) only from that event, along a single path. The write happens off the
   actor loop with a sequence number, like the session, and `Shutdown` writes it last.
   `CoreConfig::settings` hands the core what the app read.
5. **The theme follows the system by default.** `ThemeChoice::System` maps the operating
   system's appearance to dark or light (`appearance::theme_mode`), read with
   `App::window_appearance` before the window opens and kept live with
   `observe_window_appearance`, which only acts while the choice is System. The title bar
   toggle writes the explicit choice opposite to the mode shown now (showing dark, it writes
   light), even when the setting was System. Going back to System is done in Settings or the
   command palette.
6. **Language plumbing.** `Language` lives in `sc-core`, because it is a saved setting, and
   `apps/cloudrs/src/i18n.rs` re-exports it. `i18n::set` stores the language in an atomic that
   `current()` reads, so the `strings!` macros are unchanged. The picker stays hidden until a
   second language exists (M5).
7. **Discord is a setting.** `Settings::discord` replaces the `discord-off` flag file. The
   first `read_settings` that finds the file saves `discord = false`, deletes the file and
   keeps the value in the database; later reads never look at it again. The presence follows
   `Event::Settings` (`DiscordPresence::set_enabled`) and no longer touches the disk. The
   switch lives on the Settings screen (§8).
8. **The Settings screen.** A gear button in the title bar, next to the theme toggle, opens it
   (the command palette reaches it too); there is no sidebar item. It holds:
   - **Theme:** three pills, System, Dark and Light, with visible focus;
   - **Discord:** the on/off pills, moved from the Account screen unchanged, hidden when the
     build has no Discord application id;
   - **Cache:** the size of the cover cache (a skeleton until the core measures it) and
     "Clear cache". Clearing deletes only the covers this session is not showing, so no image
     on screen points at a missing file and the UI never edits its artwork map; the toast
     confirms and the size shown is what remains. `Command::MeasureCache` and
     `Command::ClearCache` do the disk work on a blocking thread;
   - **Keyboard shortcuts:** the list of keys, taken from `shell/shortcuts.rs`, in key-cap style;
   - the language picker is not drawn until a second language exists.

## Refinements: the saved settings win once the store opens

The core reads `load_settings` together with the session. If nothing changed in memory and
the saved settings differ from `CoreConfig::settings`, it adopts them: only the changed audio
commands and `Event::Settings`, with no save. If a `SetSettings` (or a device fallback) came
first, memory wins and is saved as before; since a change carries the whole `Settings`, one
made in that window (a few seconds at most, only when the early read failed) keeps the
fallback for the other fields. An empty or reset database keeps what the app read.
An adopted device that is gone goes through the ADR 0020 fallback once (`DeviceMissing`, back
to the default, saving only `output_device = None`), with no loop, since the fallback does not
resend `SetDevice`.

## Consequences

- A new setting is a field, a column and a line in the UI; the command and event do not change.
- Every change rewrites the single row, which is tiny and rare.
- Settings made before the database opened are written as soon as it does; otherwise the saved
  ones replace what the app read.
