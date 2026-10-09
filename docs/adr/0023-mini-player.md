# ADR 0023: Mini player

Status: accepted · 2026-10-08 (options chosen by the maintainer)

## Context

M5 lists a mini player: a small window to keep playback controls in view while the main window
is out of the way. GPUI can open a second window; the question is who feeds it and how it
behaves on each OS.

## Decisions

1. **The Shell's single pump feeds it (D1).** The Shell forwards to the mini player the same
   events it gives the PlayerBar, through `WindowHandle<MiniPlayer>::update`. The mini emits
   `MiniAction`s (play or pause, previous, next, show main, closed) that the Shell turns into
   `Command`s. There is no second `flume` receiver: the channel is MPMC, so two receivers would
   split the events between them. The mini holds no core handle and no business rules, like the
   PlayerBar.
2. **Window kind per OS (D2).** `WindowKind::PopUp` on Windows: always on top and no taskbar
   button (`WS_EX_TOPMOST | WS_EX_TOOLWINDOW` in the pinned GPUI). `WindowKind::Normal` elsewhere:
   the pinned GPUI has no always-on-top for a normal window on Linux or macOS, so there the mini
   player is a small window like any other. The window is 360 x 88, not resizable or
   minimizable, with the app drawing its own surface (client decorations).
3. **Opening, closing and position (D3).**
   - Toggled by the palette command "Mini player", an icon button in the player bar (before the
     queue button) and `Ctrl Shift M` (`Shift Cmd M` on macOS), which also closes it from the
     mini player itself.
   - Opening it does not activate it (`WindowOptions::focus: false`, shown with
     `SW_SHOWNOACTIVATE` on Windows), so the main window keeps its shortcuts; a click activates
     it. Linux and macOS may ignore this. Closing it does not reactivate the main window, which
     could restore a window the person minimized; the OS picks the next active window.
   - Its X closes only the mini player. "Open cloudrs" activates the main window.
   - Closing the main window still quits the app and takes the mini player with it, so the app
     never lives on with only the mini player.
   - The position is not saved: it opens at the bottom right of the primary display, inside a
     margin, each time.
4. **Dragging.** The cover and the text are the drag area (`WindowControlArea::Drag`, and
   `start_window_move` on Linux). The buttons stay outside it, because on Windows a drag area
   answers the hit test before a button under it can.
5. **Keys.** The mini player has a `MiniPlayer` key context: Space, Shift Left and Shift Right
   do what they do in the main window once the mini player is clicked and active. It has no
   text field, so the keys need no guard.

## Consequences

- No new dependency. New: `apps/cloudrs/src/mini_player.rs` (the window), `shell/mini.rs` (the
  Shell's side), `Icon::MiniPlayer` (Lucide `picture-in-picture-2`, ISC) and four size tokens.
- The mini view re-renders only when what it draws changes (track, cover, play state, or the
  progress line by a whole pixel), about twice a second for a 3-minute track; the PlayerBar still
  follows every tick.
- The 360 px width leaves about 110 px for the title and artist next to the transport and the
  window buttons: long titles truncate.
- Not verified by running it: the GUI was not run. The maintainer should try it (PLAN M5).
