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
6. **Devices are identified by cpal 0.18's id** (`DeviceId`, for example
   `wasapi:{0.0.0.00000000}.{guid}`), which cpal documents as stable across runs and reboots,
   not by name: two devices can share a name, and a name can change. The name is only for display.
   Never call `to_string()` on a `Device`: its `Display` fails (and panics) once the device is gone.
7. **Listing is a plain function**, `sc_audio::output_devices()`, which the core runs in
   `spawn_blocking` (WASAPI opens each device). An error gives an empty list and a log line.
8. **`Command::SetDevice(Option<String>)`** switches on the engine thread through the same path as
   a recovery, keeping the state and position; `None` follows the system default.
   `Player::spawn(device)` opens the saved device at start. A device that cannot be opened plays
   on the default and emits `Event::DeviceMissing`.
9. **The choice is a `Settings` field** (`output_device`, schema v4, a nullable column). The core
   sends `SetDevice` only when it changes. `Event::OutputDevices(Vec<OutputDevice>)` carries no
   `current`: the chosen one is already in `Settings` (ADR 0017 section 4).
10. **A device that disappears resets the choice** to System default (approved by the
    maintainer): on `DeviceLost` or `DeviceMissing` the core clears `output_device`, saves it and
    shows an info toast. After replugging, the person picks the device again. The player does not
    go back to it by itself.
11. **The Output section in Settings** shows System default and one pill per device, as the
    theme does. The core lists devices when the screen opens (a skeleton until it answers). An
    empty list shows a message and a Refresh button, which also serves as the recoverable error
    state, since a listing error gives an empty list.

## Consequences

- No new dependency; the 20 ms engine loop gains one atomic read.
- Settings schema goes to v4; older builds refuse the newer database, as for any migration.
- Recovery may download an HLS segment again (the Seek path).
- `is_present` enumerates devices on the engine thread, but only after a stream error; the
  half-second ring buffer covers that.
- Not tried by the maintainer yet on Windows with real hardware, nor on macOS and Linux.
