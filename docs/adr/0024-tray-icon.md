# ADR 0024: Tray icon

Status: accepted · 2026-10-08 (options chosen by the maintainer)

## Context

M5 lists a tray icon: a presence in the notification area with the controls a person reaches for
without opening the window (show it, play or pause, previous, next, quit). GPUI has no tray
support, so it comes from a crate in `sc-platform`, which already holds the other OS
integrations (ADR 0019 media controls, ADR 0015 Discord).

## Decisions

4. **`tray-icon` 0.26** (MIT or Apache-2.0) in `sc-platform::tray`, with
   `default-features = false, features = ["ksni"]`. On Linux the `ksni` backend speaks
   StatusNotifierItem over D-Bus on a thread of its own, so there is no GTK main loop to run next
   to GPUI and no `libappindicator` to install. Windows and macOS use their native tray. The app
   starts it on the main thread, like the media controls, and a failure only logs.
5. **No separate spike.** The API was read in the crate sources before coding; the first real
   test is the maintainer's.
6. **No close-to-tray.** The pinned GPUI cannot hide a window, so closing the main window still
   quits, and the tray lives only while the app does. The menu is: Show cloudrs, Play or Pause,
   Previous track, Next track, Quit cloudrs. A left click on the icon shows the window; the menu
   opens on the right click. The playback items are disabled until a track exists, and the toggle
   reads "Pause" while playing. Quit goes through the same shutdown as closing the window, so the
   session is saved first.

The tray follows the core's events like the media controls do, and only the play or pause flip and
the first track reach it; the 10 Hz playback ticks do not. Its texts come from `i18n::tray` and are
set again when the language changes.

## Consequences

- New crates in the lock: `tray-icon`, `muda` and `keyboard-types` (all OSes), and, from `ksni` on
  Linux only, `ksni` (Unlicense), `task-local` and `pastey` 0.2. Licenses were checked by hand in
  the crates' manifests (there is no `cargo deny`). `pastey` 0.2.3 is the only new duplicate
  (the lock already has `pastey` 0.1.1 through `rav1e`); `cargo tree -d --target all` shows no
  other new one.
- `sc-platform` still depends on no workspace crate and only the app uses it.
- Limits:
  - GNOME shows StatusNotifier icons only with the AppIndicator extension.
  - There is no close-to-tray.
  - The macOS icon is the colored app icon, not a template image.
  - Compiled on Windows only here; Linux and macOS are built by CI.
