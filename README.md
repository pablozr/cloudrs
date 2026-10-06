<p align="center">
  <img src="assets/brand/banner.png" alt="cloudrs: native SoundCloud for your desktop, built in Rust and drawn with GPUI" width="100%">
</p>

<p align="center">
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-ff5500?style=flat-square"></a>
  <img alt="Rust + GPUI" src="https://img.shields.io/badge/Rust-GPUI-ff5500?style=flat-square&logo=rust&logoColor=white">
  <img alt="Linux, macOS and Windows" src="https://img.shields.io/badge/platform-Linux%20%C2%B7%20macOS%20%C2%B7%20Windows-ff5500?style=flat-square">
  <img alt="Status: early" src="https://img.shields.io/badge/status-early%20%C2%B7%20building%20in%20the%20open-ff5500?style=flat-square">
</p>

<p align="center">
  <a href="#highlights">Highlights</a> ·
  <a href="#design">Design</a> ·
  <a href="#getting-started">Getting started</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#roadmap">Roadmap</a> ·
  <a href="#contributing">Contributing</a>
</p>

---

**cloudrs** is a native, open source desktop client for SoundCloud. There is no Electron and no
webview: the interface is drawn on the GPU with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui),
the UI framework behind the Zed editor, and the audio is decoded in Rust. The goal is an app that
opens instantly, stays light on memory and answers every click at 120 fps, made for people who
keep SoundCloud playing all day.

> **Status: early and built in the open.** The design is settled and the first milestone
> (search and play) is in progress. Nothing is installable yet. Ideas, issues and pull requests
> are very welcome; see [Contributing](#contributing).

## Highlights

These are the goals for the first releases; see the [roadmap](#roadmap) for what is done.

- **Native, for real.** Rust + GPUI, with a target of under 100 MB of RAM while playing.
- **The waveform is the seek bar.** Hover to preview, click to jump, with timed comments
  pinned along the track.
- **A real queue.** Drag to reorder, play next, shuffle that can be undone, and endless autoplay
  from related tracks.
- **Part of your desktop.** Media keys and system controls: MPRIS on Linux, Now Playing on
  macOS, SMTC on Windows.
- **Keyboard first.** `Ctrl K` to search, `Space` to play, and paste any SoundCloud link to play
  it right away.
- **Your library.** Likes, playlists, feed and followings once you sign in.
- **Remembers where you were.** Queue, position and volume come back when you reopen it.

## Design

<p align="center">
  <img src="docs/assets/readme/design-preview.png" alt="Design preview of the cloudrs home screen: sidebar, recently played, feed and the player bar with a waveform" width="92%">
  <br>
  <sub>Design preview (mockup with sample data). Screenshots of the real app will replace it once the first milestone lands.</sub>
</p>

SoundCloud's orange on warm, dark neutrals, with our own layout and a motion system built on
GPUI's native animations. Colors, type, motion and components are documented in
[docs/design/VISUAL-IDENTITY.md](docs/design/VISUAL-IDENTITY.md).

## Getting started

**Requirements:** stable Rust and the usual platform build tools. On Linux, the ALSA and
Wayland/X11 development packages.

```sh
git clone https://github.com/pablozr/cloudrs
cd cloudrs
cargo run --release
```

> The Cargo workspace lands with the first milestone (M0). Until then this repository holds the
> plan and the design.

## Architecture

A Rust workspace where only the app and the UI kit know about GPUI.

| Path | Role |
| --- | --- |
| `apps/cloudrs` | Desktop app (GPUI): screens, i18n, composition root |
| `crates/cloudrs-ui` | Design system: tokens, theme, motion, primitives |
| `crates/sc-core` | App state, queue, commands and events, persistence, cache |
| `crates/sc-api` | SoundCloud client: models, `client_id`, auth, pagination |
| `crates/sc-audio` | Audio engine: HLS → symphonia → cpal, on its own thread |
| `crates/sc-platform` | Media keys, MPRIS/Now Playing/SMTC, keychain, notifications |

The full plan, with endpoints, the audio pipeline and the risks, is in
[docs/PLAN.md](docs/PLAN.md). Decisions are recorded in [docs/adr/](docs/adr/).

## Roadmap

- [ ] **M0 · Spike.** Workspace, CI, `client_id` + search, HLS playback, the first GPUI window.
- [ ] **M1 · Playable MVP.** Search, play, player bar with waveform, paste a link to play.
- [ ] **M2 · Navigation.** Track, profile and playlist screens, the full queue, autoplay, history.
- [ ] **M3 · Your account.** Sign in, likes, library, feed, following.
- [ ] **M4 · Desktop integration.** Media keys, shortcuts, audio devices, timed comments, settings.
- [ ] **M5 · 0.1 release.** Gapless, EQ, mini player, packages for every platform, first translations.

## Contributing

cloudrs is early, so there is room to shape it. Good places to start:

- **Audio.** HLS, gapless playback and seeking with `symphonia` and `cpal`.
- **Platform integration.** MPRIS, Now Playing and SMTC, and packaging for each OS.
- **Translations.** The interface is English-first and built for more languages
  ([how it works](docs/design/i18n.md)).
- **Design.** New components that follow the [visual identity](docs/design/VISUAL-IDENTITY.md).

Open an issue to discuss an idea before a large change. Before sending a PR:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Small commits, one topic each, in English. See [CONTRIBUTING.md](CONTRIBUTING.md) and
[AGENTS.md](AGENTS.md).

## Disclaimer

cloudrs is an **unofficial** client and is not affiliated with, endorsed or sponsored by
SoundCloud. It streams what SoundCloud already makes available to you: it does not download
tracks, remove ads or unlock paid content. All trademarks belong to their owners.

## License

[MIT](LICENSE) © cloudrs contributors
