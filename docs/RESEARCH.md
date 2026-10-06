# Research: existing clients and API access

Status: background for [`PLAN.md`](./PLAN.md). Snapshot from October 2026.

## Existing projects

| Project | Stack | Notes |
|---|---|---|
| [fastcloud](https://github.com/COMF2222/fastcloud) | Tauri + React, Rust audio core | Webview UI, not native |
| [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop-EN) | Tauri 2 + React 19 | Popular, ~80–120 MB of RAM while playing |
| [Sonora](https://github.com/sonorahq/sonora) | Rust + GPUI | Multi-service music client, GPL-3. Good reference for a GPUI music app |
| [optionMusic](https://github.com/fireflylabss/optionMusic) | TUI + GPUI front end | SoundCloud is one of several providers |
| [rust-player](https://github.com/jhoogstraat/rust-player) | GPUI + spotatui | Clean app / core / adapter split |

Rust libraries for the API: [rsoundcloud](https://github.com/barthofu/rsoundcloud) and
[soundcloud-rs](https://github.com/emilsharkov/soundcloud-rs).

**Conclusion:** there is no mature client that is both **SoundCloud-only and fully native in
GPUI**. The dedicated clients use Tauri/webviews, and the GPUI clients cover many services.

## API access

- **Official API** (`api.soundcloud.com`):
  - OAuth 2.1 with PKCE; tokens last about one hour.
  - App registration is manual and slow, and is reported to require an Artist Pro
    subscription.
  - Streams come from `/tracks/:urn/streams` as `hls_aac_160_url` / `hls_aac_96_url`. MP3 and
    Opus transcodings were removed in 2025.
  - Limit of 15,000 plays per 24 hours per app.
- **Internal API** (`api-v2.soundcloud.com`):
  - Used by the website.
  - Needs a `client_id` taken from the site's JavaScript, which changes from time to time.
  - Richer than the official one (feed, likes, recommendations, stations).
  - Unofficial and against the terms of use, so it can break at any time.

Decision: [ADR 0002](./adr/0002-soundcloud-api-v2.md).

## Sources

- [SoundCloud API guide](https://developers.soundcloud.com/docs)
- [Deprecation notice: move to AAC HLS](https://github.com/soundcloud/api/issues/441)
- [Getting SoundCloud API access in 2026](https://publicapis.io/soundcloud-api)
- [gpui-component on crates.io](https://crates.io/crates/gpui-component)
- [gpui-unofficial on crates.io](https://crates.io/crates/gpui-unofficial)
