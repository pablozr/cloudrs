# ADR 0020: Audio output device

Status: accepted · 2026-10-07 (approved by the maintainer)

## Context

The engine opens the system default output once, at start, and only logs stream errors. If the
headphones are unplugged, or the person switches the default device in the OS, playback goes
silent or stops until cloudrs restarts. The person should also be able to choose a device in
Settings.

## Decisions

1. **The error callback only writes an atomic.** It stores a `Fault` (`Lost` outranks
   `Invalidated`) in `Shared`; it never allocates or locks. The engine thread reads it in its
   loop, at most every 20 ms.
2. **A loss reopens the default.** The engine opens the system default, rebuilds the decoder and
   resampler through the Seek path at the position reached, **pauses**, and emits
   `Event::DeviceLost`. The core turns it into an info toast. The dead output is dropped without
   a flush, since its callback may never run again.
3. **Following the default uses cpal 0.18's own notifications, without polling.** WASAPI reports
   `StreamInvalidated` (a new default exists) or `DeviceNotAvailable` (none) through the error
   callback; CoreAudio redirects by itself and reports `DeviceChanged`; ALSA's `default` follows
   the sound server. The maintainer approved this over a 2 s poll. A voluntary change (the old
   device is still present) keeps playing; a loss pauses.
4. **With no device at all** the engine keeps the dead output, stays paused and retries the
   default every second. `Play` does nothing in the meantime.
5. **Limit:** unplugging and switching the default cannot be tested without hardware, so they are
   a manual check.

## Consequences

- No new dependency; the 20 ms engine loop gains one atomic read.
- Recovery may download an HLS segment again (the Seek path).
- `is_present` enumerates devices on the engine thread, but only after a stream error; the
  half-second ring buffer covers that.
- Not tried by the maintainer yet on Windows with real hardware, nor on macOS and Linux.
