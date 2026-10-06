# ADR 0004: M1 design — `sc-core` and the UI contract

Status: accepted · 2026-10-06 (all options chosen by the maintainer)

## Context

M1 turns the M0 parts into a playable app: search, click to play, a player bar with seek, volume
and the waveform, paste a link to play, and artwork. This needs the `sc-core` layer between the
UI and the SoundCloud and audio crates.

## Decisions

1. **UI ↔ core: commands and events.** The UI sends a `Command` and renders `Event`s over two
   `flume` channels. `sc-core` knows nothing about GPUI.
2. **Async bridge: Tokio inside `sc-core`.** `sc-core` runs a current-thread Tokio runtime on its
   own thread. `sc-api` stays async. The UI never touches the runtime.
3. **`sc-core` is generic over `SoundCloudApi`** (`Core<A: SoundCloudApi>`). Its tests use a fake
   API with no HTTP.
4. **Core-owned types for the UI.** The core sends its own small types (`TrackSummary`,
   `Playback`, `Problem`). The UI never sees `sc-api` models. Problems are an enum that the UI
   turns into text through `i18n`.
5. **Artwork: the core downloads, the UI reads files.** The core fetches artwork bytes through
   `SoundCloudApi::download` and keeps them in a disk cache under `dirs::cache_dir()/cloudrs/`. It
   sends file paths, so the UI does no networking.
6. **Seek by HLS segment.** `sc-audio` restarts the fetch at the segment that contains the
   target (from `#EXTINF`) and drops samples up to the exact time. Progressive streams decode and
   drop from the start.
7. **Search as you type**, 300 ms after the last key. A newer query cancels the older one.
8. **Search field adapted from xemnas** (`SearchField` + `SearchEdit`, MIT, same maintainer) into
   `cloudrs-ui`, using our tokens. It adds `unicode-segmentation`.
9. **`Event::SearchFailed`** (approved 2026-10-06). A failed search page or "load more" page is
   reported as `SearchFailed { query, append, problem }`, separate from `Problem`, so the UI can
   tell a failed search from a playback problem without guessing from the event order.
10. **New dependencies:** `dirs` (cache folder), `tokio` with `rt` and `time` in `sc-core`, and
   `unicode-segmentation` in `cloudrs-ui`.

## Consequences

- Each layer can be tested alone: `sc-core` with a fake API, `sc-audio` with local files, and the
  UI with recorded events.
- One more thread (the core runtime) besides the UI, audio engine and fetch threads.
