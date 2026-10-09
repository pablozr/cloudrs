# Changelog

## Unreleased

- **Tray icon:** cloudrs sits in the notification area; click it to show the window, or use its
  menu for play or pause, previous, next and quit.
- **Mini player:** a small window with the cover, title, previous, play, next and a progress
  line; open it from the player bar, Ctrl P or Ctrl Shift M. Always on top on Windows.
- **Updates:** cloudrs checks GitHub once a day, downloads a new version in the background and
  installs it when you restart or quit (Windows installer, AppImage, macOS); the .deb shows a
  link. Turn it off in Settings › Updates, or run "Check for updates" from Ctrl P.
- **Sound:** loudness normalization (on by default), an equalizer with presets, and an optional
  volume boost up to 200% with a limiter, in Settings › Sound.
- **Gapless:** the next track is buffered about 20 s before the end and follows with no gap (not
  in a Jam, and not into autoplay yet).
- **Comments:** a track page lists its comments, and pins under its waveform show the timed
  ones; hover a pin to read them, click it to jump there while the track plays.
- **Audio output:** pick the output device in Settings; unplugging it pauses and moves to the
  system default, which cloudrs follows.
- **Media keys:** play, pause, next, previous and seek from the keyboard's media keys and the
  system's controls (Windows media flyout, MPRIS on Linux, Now Playing on macOS), which show
  the track, artist and cover.
- **Command palette:** Ctrl P jumps to any screen or runs an action.
- **Settings:** theme, Discord, cache size and clearing, and the keyboard shortcuts in one place.
  The gear in the title bar opens it.
- **Themes:** follows the system’s light or dark mode; the title bar toggle picks one and it is
  remembered.
- **Keyboard:** Space plays or pauses, ← → seek 5 s, Shift ← → go to the previous or next
  track, Ctrl L likes the playing track, and Ctrl V opens a copied SoundCloud or Jam link.
  Space and the arrows leave text fields alone.

## 0.1.0-beta.1 · 2026-10-07

The first public build.

- **Listen:** search tracks, people, playlists and albums. Paste a SoundCloud link to play or
  open it. The waveform is the seek bar. Track, profile, playlist and album pages, with back
  and forward.
- **Queue:**
  - play next, add, drag to reorder, remove;
  - shuffle that can be undone, repeat one or all;
  - autoplay from related tracks;
  - history;
  - the session comes back at start.
- **Account:**
  - Sign in through a SoundCloud window, or paste a token instead.
  - Feed, Likes, Library and Following. Like and follow.
  - The session is kept in the system keychain.
- **Jam:**
  - Listen together from anywhere, over peer-to-peer, with no server.
  - Share a link, and everyone hears the same moment.
  - Guests add tracks, and with the host's permission they also control playback.
  - Their own queue comes back when the Jam ends.
  - See who is listening, with avatars and names, and a pill in the title bar.
- **Home:**
  - a greeting by the time of day and a highlight of what plays;
  - quick tiles, trending tracks by genre, SoundCloud's curated rows and charts;
  - signed in: your playlists, feed, likes and artists.
- **Playlists of your own:**
  - create with a cover, description, genre, tags and privacy;
  - add from any track, reorder by dragging, change the cover;
  - remove with Undo, rename, public or private, delete;
  - in the sidebar and as a grid in the Library.
- **Discord:** shows what you are listening to, with the cover, a progress bar and buttons.
  It can be turned off on the Account screen.
- **Look:**
  - dark and light themes; pages take on the color of their artwork;
  - a title bar of cloudrs's own with the logo;
  - a branded Windows installer.
- **Install:**
  - Windows: a per-user installer. Linux: `.deb` and `.AppImage`. macOS: `.dmg`.
  - Linux and macOS are not tried yet.
  - Not code-signed yet.
