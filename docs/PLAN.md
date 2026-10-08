# cloudrs — Project plan

Status: accepted plan for v0.1. Decisions are summarized here and recorded in
[`adr/`](./adr/) as they are implemented.

cloudrs is an **open source, native and lightweight** desktop client for SoundCloud, written in
Rust with GPUI.

> Background research: [`RESEARCH.md`](./RESEARCH.md) · Visual rules:
> [`design/VISUAL-IDENTITY.md`](./design/VISUAL-IDENTITY.md)

---

## 1. Vision

Listen to SoundCloud on the desktop with the feel of a native app: no webview, no Electron.

**Targets:**
- Opens in under 300 ms.
- Uses less than 100 MB of RAM while playing.
- Renders at 120 fps.

**Audience:** people who listen to SoundCloud all day (long mixes, DJ sets, underground
tracks) and want:
- OS integration (media keys, MPRIS, notifications);
- a real queue;
- caching;
- keyboard shortcuts.

### Goals (v1.0)
- Search and play tracks, playlists, albums and profiles.
- Sign in with the user's account for likes, playlists, feed and followings.
- Queue, history, shuffle, repeat and session restore.
- SoundCloud's waveform as the seek bar, with timed comments.
- Media keys and OS controls: MPRIS on Linux, Now Playing on macOS, SMTC on Windows.
- Linux, Windows and macOS.
- English by default, with more interface languages added over time ([`design/i18n.md`](./design/i18n.md)).

### Non-goals (for now)
- Uploads and artist tools.
- Downloading tracks to files. Left out on purpose so the repository gives no reason for a DMCA
  takedown.
- Mobile and web.
- Other services (Spotify, YouTube…). **SoundCloud only** is the point of the project.

---

## 2. Architecture decisions (summary)

| # | Decision | Why |
|---|---|---|
| D1 | **GPUI pinned to a Zed monorepo revision** (the same `rev` as xemnas: `244023605536a412ab6b8d5b658466b89fb15401`), with `gpui_platform`'s `wayland` and `x11` features on Linux | Proven in xemnas, includes AccessKit support. The crates.io `gpui` is frozen at 0.2.2. See [ADR 0001](./adr/0001-gpui-pinned-to-zed.md) |
| D2 | **Own UI kit, `crates/cloudrs-ui`** (tokens, theme, motion, primitives) | `gpui-component` depends on `gpui-pre`, a different crate from Zed's `gpui`, so the two cannot be mixed. Same approach as xemnas's `ui/` |
| D3 | **SoundCloud's internal `api-v2`**, behind a trait | Free and needs no approval. The official API requires a manual review and a paid account. The trait lets us switch later. See [ADR 0002](./adr/0002-soundcloud-api-v2.md) |
| D4 | **Own audio pipeline**: `symphonia` (decode) + `cpal` (output) | Full control over buffering, seeking, gapless and EQ. `rodio` is simpler but limits gapless and seeking on streams |
| D5 | **`sc-core` runs a small Tokio runtime on its own thread** for all I/O; the UI talks to it only through a `Command` channel and an `Event` channel (`flume`). `sc-audio` has no async runtime: its fetcher is a plain thread with `reqwest::blocking` | GPUI has its own executor. The UI stays free of I/O and business rules, and audio never depends on the UI or an async runtime (approved 2026-10-06) |
| D6 | **Central state in `sc-core`**, generic over the `SoundCloudApi` trait (`Core<A: SoundCloudApi>`). The UI reads snapshots and sends commands | Logic is testable with a fake API and no HTTP; generics cost nothing at runtime (approved 2026-10-06) |
| D7 | **SQLite** (`rusqlite`) for persistence + disk cache | History, session, metadata and image cache |
| D8 | **Tokens in the OS keychain** (`keyring`) | Never store the `oauth_token` in plain text |
| D9 | **English everywhere in the repository** (code, docs, commits). UI strings go through `i18n` from day one | Open source reach. Other languages ship later without touching screens |
| D10 | **MIT license** | Same as xemnas. Simple for contributors |

---

## 3. Workspace layout

```
cloudrs/
├── Cargo.toml              # [workspace]; shared versions in [workspace.dependencies]
├── crates/
│   ├── sc-api/             # api-v2 HTTP client: models, client_id, auth, pagination
│   ├── sc-audio/           # audio engine: HLS/progressive → decode → output
│   ├── sc-session/         # Jam: peer-to-peer listening sessions over iroh (ADR 0011)
│   ├── sc-core/            # app state, queue, commands/events, persistence, cache (M1)
│   ├── sc-platform/        # OS integration: keychain and sign-in window (M3); media keys, MPRIS, notifications (M4)
│   └── cloudrs-ui/         # design system: tokens, theme, motion, primitives (GPUI only here and in the app)
├── apps/
│   └── cloudrs/            # GPUI binary: screens, i18n, composition root
├── assets/                 # brand (logo, icon, banner), fonts, SVG icons
├── tests/architecture/     # layering rules, checked in CI
└── docs/
```

**Dependencies flow downwards, with no cycles:**

```
apps/cloudrs ──► cloudrs-ui
     │
     └──► sc-core ──► sc-api
     │       ├──────► sc-audio
     │       └──────► sc-session   (Jam, feature `jam`, ADR 0011)
     └──► sc-platform
```

`sc-api`, `sc-audio` and `sc-session` do not know each other: `sc-core` resolves the stream URL
and hands it to the player, and turns session messages into its own commands. Only `apps/cloudrs` and `cloudrs-ui` may depend on GPUI, and a test in
`tests/architecture` will enforce it (as in xemnas).

---

## 4. `sc-api` — SoundCloud client

### 4.1 `client_id`
1. `GET https://soundcloud.com/` and find the `<script src="https://a-v2.sndcdn.com/assets/*.js">` tags.
2. Fetch the scripts **from last to first** (the id usually lives in the last ones) and match
   `client_id\s*[:=]\s*"([a-zA-Z0-9]{32})"`.
3. Cache the id in SQLite with a timestamp.
4. On **401/403**, extract again **once** and retry the request.
5. Allow overriding it from the config, for debugging.

### 4.2 User authentication
- api-v2 offers no OAuth flow for third-party apps; the app needs the `oauth_token` cookie of
  soundcloud.com. M3 signs in through a small window (`wry`, in a child process) that reads the
  cookie after login, with pasting the token as the fallback
  ([ADR 0010](./adr/0010-m3-sign-in-and-account.md)).
- Header: `Authorization: OAuth <token>`.
- Validate with `GET /me` and store the token in the keychain.

### 4.3 Initial endpoints (all with `?client_id=`)

| Use | Endpoint |
|---|---|
| Search | `/search?q=`, `/search/tracks`, `/search/users`, `/search/playlists`, `/search/albums` |
| Resolve a pasted URL | `/resolve?url=https://soundcloud.com/...` |
| Track | `/tracks/{id}`, `/tracks?ids=1,2,3` (batch, up to ~50) |
| Related | `/tracks/{id}/related` |
| Comments | `/tracks/{id}/comments?threaded=0` |
| Playlist | `/playlists/{id}` (tracks come partially: fill in with `/tracks?ids=`) |
| User | `/users/{id}`, `/users/{id}/tracks`, `/users/{id}/playlists`, `/users/{id}/likes` |
| Signed in | `/me`, `/me/library/all`, `/users/{me}/track_likes`, `/stream` (feed), `/me/followings` |
| Actions | `PUT/DELETE /users/{me}/track_likes/{id}`, follow/unfollow |
| Stream | `media.transcodings[i].url` + `client_id` + `track_authorization` → `{ "url": "<m3u8 or mp3>" }` |

### 4.4 Implementation notes
- **Pagination:** `linked_partitioning=1` and follow `next_href`, exposed as a small
  `Paginator<T>`.
- **Models:** `serde` with `#[serde(default)]` and optional fields everywhere, because the API
  changes without notice. **Tests use real JSON fixtures** in `crates/sc-api/tests/fixtures/`.
- **Errors:** `thiserror`, with `Unauthorized`, `RateLimited { retry_after }`, `NotFound`,
  `GeoBlocked`, `Network` and `Decode`.
- **Rate limiting:** exponential backoff on 429 and a semaphore of about 4 concurrent requests.
- **Artwork:** `artwork_url` comes as `-large` (100 px). Swap for `-t500x500` or `-t300x300`.
- **Public trait:**

```rust
#[async_trait]
pub trait SoundCloudApi: Send + Sync {
    async fn search_tracks(&self, q: &str, page: PageReq) -> Result<Page<Track>>;
    async fn resolve(&self, url: &str) -> Result<Resource>;
    async fn track(&self, id: TrackId) -> Result<Track>;
    async fn stream_url(&self, track: &Track) -> Result<StreamSource>;
    async fn me(&self) -> Result<User>;
    // ...
}
```

---

## 5. `sc-audio` — audio engine

### 5.1 Picking a format
Each track offers several `transcodings`. Preference order, with fallback:
1. `hls` + `audio/mp4; codecs="mp4a.40.2"` (AAC 160k, the current default)
2. `hls` + AAC 96k
3. `progressive` + `audio/mpeg` (MP3 128k), if it still exists
4. `hls` + `audio/mpeg` / `audio/ogg; codecs="opus"` (legacy)

Encrypted transcodings (`encrypted-hls`, `ctr-encrypted-hls`, `cbc-encrypted-hls`) and
`snipped` tracks (GO+ 30-second previews) are **skipped**, and the UI says why.

> ⚠️ **M0** exists to confirm, with real tracks, which formats show up today and which
> container the AAC segments use (fMP4 or ADTS).

### 5.2 Pipeline

```
[fetch thread]     m3u8 → fetch segments ahead (~30 s) → bounded channel of segments
        │
[decoder thread]   symphonia (isomp4/adts + aac | mp3 | ogg/opus) → f32 PCM
        │          resample (linear until M5, then evaluate rubato) to the device rate
        │
[cpal callback]    lock-free sample ring buffer (rtrb) → sound card
```

- **Seeking in HLS:** use each segment's `#EXTINF` to find the target segment, fetch from it
  and drop samples up to the exact time.
- **Gapless / preload:** with about 20 s left, the next track is resolved and buffered.
- **URL expiry:** stream URLs expire after a few minutes. After a long pause or a seek, ask
  `sc-core` for a fresh URL through a refresh channel.
- **Events out:** `Position(Duration)` at about 10 Hz, `Buffering(bool)`, `TrackEnded`,
  `Error(..)`, `NearEnd`.
- **Commands in:** `Load(Source)`, `Play`, `Pause`, `Seek(Duration)`, `SetVolume(f32)`,
  `Preload(Source)`, `Stop`.
- **Output device:** list and switch with cpal, and recover when the device disappears
  (headphones unplugged).
- **Never allocate or lock inside the cpal callback.**

---

## 6. `sc-core` — state and rules

- **`AppState`**:
  - session (user, tokens);
  - `PlayerState` (current track, position, volume, status);
  - `Queue`;
  - caches (tracks and users by id).
- **Queue:**
  - `Vec<TrackId>` with the current index;
  - source context (playlist X, search Y, likes);
  - shuffle that keeps the original order (so it can be undone);
  - repeat off/one/all;
  - "play next" and "add to queue".
- **Autoplay:** when the queue ends, fetch `/tracks/{id}/related` and keep playing, like the
  website does.
- **Persistence (SQLite in `dirs::data_dir()/cloudrs/`):**
  - `session` (queue, position, volume);
  - `history`;
  - `cache_tracks` (JSON + TTL);
  - `settings` (theme, language, Discord; ADR 0017).
- **Image cache:** files in `dirs::cache_dir()/cloudrs/img/`, keyed by URL hash, with an LRU
  size limit.
- **API for the UI:** a `Command` enum goes in, `Event`s and snapshots come out. The UI never
  calls `sc-api` directly.

---

## 7. UI (`apps/cloudrs` + `cloudrs-ui`)

The visual rules, tokens and motion catalog live in
[`design/VISUAL-IDENTITY.md`](./design/VISUAL-IDENTITY.md). The approved preview is
[`assets/readme/design-preview.png`](./assets/readme/design-preview.png).

### 7.1 Layout

```
┌──────────┬──────────────────────────────────────────────┐
│ Sidebar  │  Content (routes)                            │
│ · Home   │                                              │
│ · Feed   │                                              │
│ · Search │                                              │
│ · Likes  │                                              │
│ · History│                                              │
│ Playlists│                                              │
├──────────┴──────────────────────────────────────────────┤
│ ▶ ⏮ ⏭  [▁▃▅▇▅▃▁▃▅▇ waveform ▇▅▃▁]  01:23 / 58:10  🔊 ☰ │
└─────────────────────────────────────────────────────────┘
```

### 7.2 Screens
1. **Search:** 300 ms debounced input, tabs (All / Tracks / People / Playlists / Albums) and
   infinite scroll on a virtual list.
2. **Track:** artwork, large waveform, description, comments, related tracks.
3. **Playlist / Album:** header and track list.
4. **Profile:** header, tabs (Tracks / Playlists / Likes / Reposts) and a follow button.
5. **Likes and Library** (signed in).
6. **Feed** (signed in).
7. **Queue:** side panel with drag and drop.
8. **Settings:** account/token, audio device, theme, language, cache, shortcuts.

### 7.3 Custom components
- `Waveform`: drawn from `waveform_url` (JSON with about 1800 samples), with progress color,
  hover preview and click or drag to seek.
- `TrackRow`, `TrackCard`, `UserCard`, `PlaylistCard`.
- `PlayerBar`.
- `AsyncImage`: loads from the cache and shows a placeholder.

### 7.4 Shortcuts
| Key | Action |
|---|---|
| `Space` | play/pause |
| `←` / `→` | seek ±5 s |
| `Shift+←/→` | previous/next |
| `Ctrl+L` | like |
| `Ctrl+K` / `/` | search |
| `Ctrl+V` anywhere | resolve a pasted SoundCloud URL and play it |

Outside text fields Space always plays/pauses, even over a focused control (Enter activates it); the arrows and Ctrl+V act outside text fields only; `secondary` = Cmd on macOS.

---

## 8. `sc-platform` — OS integration
- **Media keys and Now Playing:** `souvlaki` (MPRIS / macOS / SMTC).
- **Tokens:** `keyring`.
- **Track-change notifications:** `notify-rust` (Linux/Windows), optional.
- **Discord Rich Presence:** `discord-rich-presence`, on by default and switchable (ADR 0015).
- **Tray icon:** `tray-icon` (M5).

---

## 9. Milestones

### M0 — Technical spike (de-risk) · ~1 week
- [x] Cargo workspace + CI (fmt, clippy, test) on GitHub Actions for Linux, macOS and Windows.
- [x] `sc-api`: extract the `client_id` and run `search/tracks` from the terminal
      (`cargo run -p sc-api --example search`).
- [x] `sc-audio`: play an HLS AAC stream end to end without UI (`cargo run -p sc-audio --example play <url>`).
- [x] GPUI window on the pinned Zed revision, with `cloudrs-ui` tokens, motion and the embedded
      fonts (`cargo run -p cloudrs`).
- [x] Record the findings in [ADR 0003](./adr/0003-m0-spike-findings.md).
- [ ] Run the examples against the live SoundCloud from a normal machine and complete ADR 0003
      (the build environment cannot reach soundcloud.com).

### M1 — Playable MVP (done 2026-10-06)
Design: [ADR 0004](./adr/0004-m1-core-and-ui-contract.md) (core and contract),
[ADR 0005](./adr/0005-m1-app-shell.md) (app shell).

Done (layers below the UI):
- [x] `sc-audio`: seek by HLS segment (`Command::Seek`), `Player::into_channels`.
- [x] `sc-api`: `waveform` and `download` (CDN, no credentials).
- [x] `sc-core`: actor with debounced search and paging, play from results or a pasted link,
      playback state, waveform bars, artwork disk cache; tested with a fake API and fake audio.
- [x] `cloudrs-ui`: single-line `SearchField` adapted from xemnas (`bind_keys` registers its keys).

Done (the app, `apps/cloudrs`):
- [x] Composition root: `sc_core::start(CoreConfig { cache_dir: dirs::cache_dir()/cloudrs })`
      (`dirs` approved), full-window problem state with "Try again" if it fails (no audio device).
- [x] Shell view: owns the `CoreHandle`, pumps `events().recv_async()` into view state, and
      sends `Command::Search` / `Command::PlayUrl` (when the text is a soundcloud.com URL) from
      `SearchChanged`. `Ctrl K` and `/` focus the search field.
- [x] Results: `uniform_list` of track rows with artwork (`img(path)`), click to play,
      `Command::LoadMore` near the end of the list; skeleton, empty, no-results and error
      ("Try again") states.
- [x] Player bar: now playing + artwork, play/pause, time, waveform with click to seek, volume.
- [x] Problems as text through `i18n` (`Problem` → message) shown as a toast for 4 s.
- [x] Screenshots in both themes: [`m1-*.png`](./assets/readme/), taken in a Linux container
      under `Xvfb` and lavapipe against the live SoundCloud (empty, results, scrolled, toast,
      playing, offline error).

Before ticking M1:
- [x] Listen on a real machine: confirmed by the maintainer on Windows 11 (2026-10-06) with a
      release build cross-compiled in a Linux container from the user's Windows SDK.
- [x] Waveform hover preview of the seek target (motion catalog 3): the bars between the
      progress and the pointer get a muted accent tint.

### M2 — Navigation
Design: [ADR 0007](./adr/0007-m2-queue-and-persistence.md),
[ADR 0008](./adr/0008-m2-screens-and-lists.md).

- [x] Track, Profile, Playlist, Album and History screens, search tabs (tracks, people,
      playlists, albums), sidebar and back/forward navigation.
- [x] Full queue (next/previous, play next, shuffle, repeat, reorder): side panel with drag and
      drop (lifted row with an accent outline and a highlighted drop target; neighbors do not
      spring aside yet).
- [x] Autoplay from related tracks.
- [x] History (recorded after 30 s of listening; its screen comes with the other screens) and
      session restore (queue, position and volume, paused at start).

### M3 — User account
Design: [ADR 0010](./adr/0010-m3-sign-in-and-account.md).

- [x] Sign in through a SoundCloud web sign-in window (`cloudrs --sign-in`, `wry`), with a pasted
      token as the fallback; token in the keychain (`sc-platform`).
- [x] Likes, Library, Feed, Following.
- [x] Like/unlike, follow/unfollow.
- [x] Tried on Windows 11 with a real account by the maintainer (2026-10-07).
- [ ] Screenshots of the account screens in both themes.

### M4 — Desktop integration
- [ ] MPRIS / Now Playing / SMTC.
- [x] Full keyboard shortcuts (Ctrl K still focuses search; the command palette awaits a decision).
- [ ] Audio device selection and recovery.
- [ ] Timed comments on the waveform.
- [ ] Settings + light/dark themes + language picker.

### M5 — Polish and 0.1 release
- [ ] Gapless, loudness normalization, equalizer.
- [ ] Mini player, tray, Discord RPC.
- [ ] Packaging: `.AppImage`/`.deb`/Flatpak, `.dmg`, `.msi` (via `cargo-dist` or
      `cargo-packager`).
- [ ] First translations beyond English.
- [ ] Website/README with GIFs and a download page.

### Later — Listen together (after M2)
Design: [ADR 0006](./adr/0006-listen-together-p2p.md) (direction),
[ADR 0011](./adr/0011-jam-iroh-and-protocol.md) (iroh, `sc-session`, protocol v1).
Peer-to-peer, no cloudrs server; state is synced, audio never leaves SoundCloud.
- [x] Spike: iroh 1.3 on one machine (connect, size, idle cost), recorded in ADR 0011.
- [ ] Traversal test between two home networks and an hour-long relay-only session.
- [x] `sc-session`: host creates an invite link, guests join with it (loopback tests; the
      example `jam` tries two machines).
- [x] `sc-audio`: `Prepare` loads a track paused at a position.
- [x] `sc-core`: host mode (broadcast, requests, start barrier) and guest mode (mirror,
      clock offset, drift correction, pre-Jam queue restored).
- [x] UI: Jam screen in the sidebar, copy link, people, cannot-play badges, permission, end or
      leave; a pasted link joins. (No command palette exists yet for Ctrl K.)

---

## 10. Quality and process
- **Stable Rust**, edition 2024, MSRV pinned in `rust-toolchain.toml`.
- **CI:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` on all three
  platforms, and `cargo deny` (licenses and advisories).
- **Tests:**
  - `sc-api`: JSON fixtures + `wiremock`. No real network in CI.
  - `sc-audio`: decode small test files in `tests/assets/`.
  - `sc-core`: queue, shuffle and restore (pure logic).
  - An optional, manual smoke test against the real SoundCloud, to catch API breakage.
- **Logs:** `tracing` + `tracing-subscriber`, `RUST_LOG=cloudrs=debug`.
- **Commits:** small, one topic each, in English, following Conventional Commits (`feat:`,
  `fix:`, `docs:`…) so the changelog can be generated.
- **Issues:** bug/feature templates and `good first issue` labels.

---

## 11. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| SoundCloud changes `api-v2` or the `client_id` | High | Automatic re-extraction, tolerant models, weekly smoke test, trait to switch backends |
| Streams become encrypted only (DRM) | High | Monitor. Never circumvent DRM. Tell the user when a track cannot play |
| GPUI API changes (pre-1.0) | Medium | Pinned revision. Upgrades in dedicated PRs, coordinated with xemnas |
| Takedown request (DMCA/ToS) | Medium | No downloads, no ad or paywall removal, no GO+ bypass, a clear "unofficial client" notice, a logo unlike SoundCloud's |
| Audio stutter | Medium | Dedicated thread, lock-free buffers, no allocation in the cpal callback |

---

## 12. Next step

Start **M0**: create the workspace and the three examples (`search`, `play`, `hello-window`).
