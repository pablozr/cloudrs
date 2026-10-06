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
| D1 | **GPUI pinned to a Zed monorepo revision** (the same `rev` as xemnas: `244023605536a412ab6b8d5b658466b89fb15401`) | Proven in xemnas, includes AccessKit support. The crates.io `gpui` is frozen at 0.2.2. See [ADR 0001](./adr/0001-gpui-pinned-to-zed.md) |
| D2 | **Own UI kit, `crates/cloudrs-ui`** (tokens, theme, motion, primitives) | `gpui-component` depends on `gpui-pre`, a different crate from Zed's `gpui`, so the two cannot be mixed. Same approach as xemnas's `ui/` |
| D3 | **SoundCloud's internal `api-v2`**, behind a trait | Free and needs no approval. The official API requires a manual review and a paid account. The trait lets us switch later. See [ADR 0002](./adr/0002-soundcloud-api-v2.md) |
| D4 | **Own audio pipeline**: `symphonia` (decode) + `cpal` (output) | Full control over buffering, seeking, gapless and EQ. `rodio` is simpler but limits gapless and seeking on streams |
| D5 | **Tokio** for networking, a **dedicated thread** for audio, channels (`flume`) in between | GPUI has its own executor. Audio must never depend on the UI or the network |
| D6 | **Central state in `sc-core`**. The UI reads snapshots and sends commands | Logic is testable without UI, and a fake runtime enables UI work without credentials |
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
│   ├── sc-core/            # app state, queue, commands/events, persistence, cache
│   ├── sc-platform/        # OS integration: media keys, MPRIS, keychain, notifications
│   └── cloudrs-ui/         # design system: tokens, theme, motion, primitives (GPUI only here and in the app)
├── apps/
│   └── cloudrs/            # GPUI binary: screens, i18n, composition root
├── assets/                 # brand (logo, icon, banner), fonts, SVG icons
└── docs/
```

**Dependencies flow downwards, with no cycles:**

```
apps/cloudrs ──► cloudrs-ui
     │
     └──► sc-core ──► sc-api
     │       └──────► sc-audio
     └──► sc-platform
```

`sc-api` and `sc-audio` do not know each other: `sc-core` resolves the stream URL and hands it
to the player. Only `apps/cloudrs` and `cloudrs-ui` may depend on GPUI, and a test in
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
- api-v2 offers no OAuth flow for third-party apps. The first version asks the user to paste
  their `oauth_token`, with a step-by-step guide to find the `oauth_token` cookie in the browser.
- Possible later: a small sign-in window (`wry`) that reads the cookie after login. Nice to
  have only, because it brings a webview back.
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
[fetcher (tokio)]  m3u8 → fetch segments ahead (~30 s) → byte ring buffer
        │
[decoder thread]   symphonia (isomp4/adts + aac | mp3 | ogg/opus) → f32 PCM
        │          resample (rubato) to the device rate, volume, EQ (later)
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
  - `settings` (including the interface language).
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

---

## 8. `sc-platform` — OS integration
- **Media keys and Now Playing:** `souvlaki` (MPRIS / macOS / SMTC).
- **Tokens:** `keyring`.
- **Track-change notifications:** `notify-rust` (Linux/Windows), optional.
- **Discord Rich Presence:** `discord-rich-presence`, optional and off by default (M5).
- **Tray icon:** `tray-icon` (M5).

---

## 9. Milestones

### M0 — Technical spike (de-risk) · ~1 week
- [ ] Cargo workspace + CI (fmt, clippy, test) on GitHub Actions for Linux, macOS and Windows.
- [ ] `sc-api`: extract the `client_id` and run `search/tracks` from the terminal
      (`cargo run --example search`).
- [ ] `sc-audio`: play an HLS AAC track end to end without UI (`cargo run --example play <url>`).
- [ ] "Hello" GPUI window on the pinned Zed revision, with `cloudrs-ui` tokens and the embedded
      fonts, building on all three platforms.
- [ ] Record the findings (formats, container, URL expiry) in an ADR.

### M1 — Playable MVP
- [ ] Search tracks → list → click to play.
- [ ] PlayerBar: play/pause, seek, volume, time.
- [ ] Waveform in the PlayerBar.
- [ ] Paste a SoundCloud URL and play it.
- [ ] Artwork with cache.

### M2 — Navigation
- [ ] Track, Profile, Playlist and Album screens.
- [ ] Full queue (next/previous, play next, shuffle, repeat, reorder).
- [ ] Autoplay from related tracks.
- [ ] History and session restore.

### M3 — User account
- [ ] Sign in with a token + keychain.
- [ ] Likes, Library, Feed, Following.
- [ ] Like/unlike, follow/unfollow.

### M4 — Desktop integration
- [ ] MPRIS / Now Playing / SMTC.
- [ ] Full keyboard shortcuts.
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
