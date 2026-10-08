# ADR 0019: OS media controls (SMTC, MPRIS, Now Playing)

Status: accepted · 2026-10-07 (options chosen by the maintainer)

## Context

PLAN §1 promises media keys and OS controls. With cloudrs open, the OS should show the
current track (title, artist, cover, duration) and the playback state, and its keys and
buttons (play, pause, next, previous, seek) should drive the player.

## Decisions

1. **`souvlaki`** (MIT) in `sc-platform::media`, with `default-features = false` and
   `features = ["use_zbus"]`, so Linux needs no `libdbus-dev`. It is **pinned by git rev**
   (`93ff3d16…`, its master), not by version: that revision shares `windows 0.62`, `zbus 5` and
   `core-graphics 0.25` with the rest of the lock, while crates.io 0.8.3 would add `windows 0.44`,
   `zbus 3` and `cocoa 0.24`. Move back to crates.io when souvlaki 0.9 is published.
2. **The window handle** comes from `gpui::Window` through `raw-window-handle` 0.6 (app only,
   Windows only, the version GPUI already uses) and goes to `sc-platform` as an
   `Option<*mut c_void>`. SMTC needs it; the other platforms ignore it.
3. **Created on the main thread**, in `Shell::new`: SMTC needs the window and COM (GPUI
   initializes it), and macOS registers its handlers on the main run loop. On Linux, zbus runs
   on a thread of its own.
4. **Same pattern as ADR 0015.** The shell follows `NowPlaying`, `NowPlayingLinks` and
   `Playback`, and tells the OS only when the track or cover changes, play or pause flips, or
   the position jumps by 2 s or more. The ~10 Hz playback ticks never reach it.
5. **Keys arrive through a `flume::Receiver<MediaKey>`** and become `TogglePlay`, `Next`,
   `Previous`, `Seek` and `SetVolume`. A Jam guest is refused by the core
   (`Problem::JamNotAllowed`), as with the buttons. Without a current track, every key does
   nothing, like the keyboard shortcuts.
6. **Play, Pause and Stop** map onto the core's single `TogglePlay`, which does nothing while a
   track loads:

   | Key | Acts (as `TogglePlay`) when |
   |---|---|
   | Toggle | always |
   | Play | not playing and not loading |
   | Pause, Stop | playing |

   Fast-forward and rewind seek 5 s like the arrow keys; `SeekBy` and `SetPosition` are clamped
   to the track. Every seek is ignored while the duration is unknown. `OpenUri`, `Raise` and
   `Quit` are dropped.
7. **A failure only logs.** If the controls cannot start or stop answering, cloudrs writes one
   warning and carries on; no crash, no toast.
8. **OS notifications** (`notify-rust`) are deferred.

## Consequences

- One new dependency in `sc-platform` (`souvlaki`) and one in the app (`raw-window-handle`, Windows).
- Lock: `souvlaki` adds `pollster 1.0.1` as a third copy of `pollster` (the lock already has
  0.2.5 and 0.4.0); no other new duplicate in `cargo tree -d --target all -e normal`.
  There is no `deny.toml` and CI does not run `cargo deny`, so the new transitive licenses were
  not checked by a tool.
- Known limits:
  - on Windows, a track without a cover keeps the previous thumbnail, and the flyout's position
    only updates on play, pause and seek;
  - Stop pauses, since the core has no stop;
  - volume goes to the OS on Linux (MPRIS) only;
  - on Linux without a session bus, the souvlaki thread panics once and the controls stay inactive.
- Not tried by the maintainer yet on macOS and Linux; CI only compiles them.
