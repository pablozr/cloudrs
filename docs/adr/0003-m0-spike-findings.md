# ADR 0003: M0 spike findings

Status: accepted · 2026-10-06

## Context

M0 had to de-risk three things before building screens: the `api-v2` client, HLS playback in pure
Rust, and GPUI on the pinned Zed revision ([ADR 0001](./0001-gpui-pinned-to-zed.md)).

## Findings

**GPUI**
- The pinned revision builds and runs on Linux. The app opened under Xvfb with Vulkan from Mesa
  lavapipe: embedded fonts resolved as `Bricolage Grotesque`, `Geist` and `Geist Mono`, and
  clicks, hover, timers and the theme switch all work.
- On Linux, `gpui_platform` needs the `wayland` and `x11` features. Without them the app panics at
  startup. xemnas never hit this because it only targets Windows. The workspace dependency now
  enables both.
- Linux build packages: `libasound2-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev
  libx11-xcb-dev libxcb1-dev libvulkan-dev libfontconfig-dev libzstd-dev`.
- GPUI has no scale transform on `div`, so "pop" motions are a fade plus a few pixels of travel
  (as in xemnas), and a spring overshoot is shown through position, not size.

**Audio**
- symphonia 0.6 (`aac`, `isomp4`, `mp3`) decodes a non-seekable byte stream made of the fMP4 init
  segment followed by the media segments. This is the shape of SoundCloud's current AAC HLS
  streams. ADTS segments and progressive MP3 decode too.
- The ADTS reader accepts only one AAC frame per ADTS packet. ffmpeg's default HLS muxer writes
  MPEG-TS, not ADTS, so the test assets use `-f segment -segment_format adts`.
- The full player path works end to end on the ALSA `null` device: fetch thread → decode →
  resample → lock-free ring → cpal callback, with the states Loading → Playing → Ended.
- The resampler is linear for now. A band-limited one (`rubato`) is planned when the output path
  is tuned (M5).

**API**
- The client extracts the `client_id`, refreshes it once on 401/403, follows `next_href`, reports
  `Retry-After` on 429, and picks AAC HLS, then progressive MP3, then HLS MP3. It never picks
  encrypted, preview or Opus renditions.

## Not validated yet

soundcloud.com is unreachable from the build environment, so nothing has run against the live
service. The fixtures are hand-written in the `api-v2` shape. Before M1, run the examples from a
normal machine and record the results here:

```sh
cargo run -p sc-api --example search -- "charlotte de witte"
cargo run -p sc-audio --example play -- "$(cargo run -q -p sc-api --example stream -- "lights out")"
```

Open questions for that run: which transcodings appear today, whether AAC segments are fMP4, and
how long stream URLs stay valid.

## Decision

Keep the stack as planned. Enable `wayland` and `x11` on `gpui_platform`. Move on to M1 once the
live run above confirms the stream format.
