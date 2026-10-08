# ADR 0022: Gapless, loudness and equalizer

Status: accepted · 2026-10-08 (approved by the maintainer)

This ADR groups the M5 playback-quality work. It grows with the commits that implement each
part: the sections for loudness normalization, the equalizer, the volume boost with a limiter,
the settings they need and the better resampler arrive with those commits. Only Gapless is
written so far.

## Gapless

### Context

Between two tracks the player used to stop: the engine emitted `Ended`, the core resolved the
next stream URL and sent `Load`, and the person heard a silence of a second or more. A
continuous album or a split DJ mix should play without a break.

### Decisions

1. **The core decides when the end is near.** It already sees `Position` and the track duration,
   and it owns the queue, so it knows what comes next. It sends `Preload(Source)` about 20 s
   before the end, `CancelPreload` when the plan changes, and the engine answers with
   `NextStarted` when the next source begins. These replace the `NearEnd` event in
   [PLAN](../PLAN.md) section 5.2 (a deviation, approved): the engine would need the duration
   and the queue to emit it. `TrackEnded` stays as `State(Ended)`.
2. **The preload opens on a short-lived helper thread** (`cloudrs-preload`). `Stream::open`
   blocks on the network (playlist, first segments) and the engine thread must keep feeding the
   ring buffer. The thread sends the opened track back through a bounded channel that the
   engine polls each turn; a replaced or cancelled preload is simply dropped.
3. **The next track follows the current one in the same ring buffer.** When the current track
   is fully decoded and written, the engine takes the preloaded track and keeps writing. If both
   have the same format the resampler is carried over, so the join is continuous. The ring holds
   about 0.5 s, so the handover happens at most that long before the old track's last sample
   plays.
4. **The handover is counted in samples.** `timeline.rs` records how many samples were written
   when the next track began (a write may stop in the middle of a frame), converts that to the
   frame count the callback reports, and reports `Position` against the old track until the
   device crosses that point. Then it emits `NextStarted`. The cpal callback is unchanged: it
   allocates and locks nothing, and still just counts frames.
5. **Seek keeps the preload.** A seek inside the current track does not change what follows it.
   A pending handover is completed before a seek or an output switch, so positions stay right.
   `Load`, `Prepare`, `Stop` and a failure drop the preload.
6. **No preload in a Jam.** Everyone starts each track together through `Prepare` and `Play`, so
   a track that starts by itself would break the sync. A Jam keeps its gap.
7. **Autoplay of related tracks keeps its gap for now.** The related list is fetched only when
   the queue ends, so there is nothing to preload.

### The core's part

8. **The trigger.** On each `Position`, while playing, outside a Jam, with a known duration and
   at most 20 s left, the core asks the queue what plays after the current track
   (`Queue::peek_next`, which mirrors `next` for a natural end without moving) and, unless it is
   a preview, resolves its stream URL (fetching the track first if it was never cached) and
   sends `Preload`. It acts once per play.
9. **Invalidation by key.** Each queue entry has a key. The prepared track remembers the key it
   was prepared for, and every queue change compares it with `peek_next`: a mismatch (reorder,
   Play next, shuffle, removal) sends `CancelPreload`. Answers from a resolve that finished late
   are ignored when the play or the key no longer matches.
10. **Pause cancels.** A long pause could outlive the segment URLs, so pausing cancels the
    preload; the next `Position` while playing prepares it again.
11. **`NextStarted` moves the core on without loading.** The queue advances, the screens show
    the new track, and no `Load` is sent. If the preload was cancelled but the engine had
    already started it, or the queue changed meanwhile, the core falls back to the normal path
    and loads whatever is next. A resolve error is logged and not retried: the track takes the
    normal path when the current one ends.
12. **Starting a Jam cancels the preload**, and a `NextStarted` that arrives in a Jam moves on
    through the normal path.

### Accepted limits

- **AAC priming.** Each AAC stream starts with about 46 ms of encoder delay that symphonia does
  not trim, so a join between two AAC tracks has a short, quiet gap. Trimming it is future work.
- **A queue change in the last 0.5 s.** Once the next track's samples are in the ring buffer they
  cannot be taken back; reordering or `PlayNext` in that window plays the old next track.
- **A seek or a device switch in that window** completes the handover early and drops the
  samples still buffered (at most 0.5 s).
- **A track shorter than the trigger**, or whose real length differs from its metadata, falls
  back to the normal `Ended` path, which has the old gap.
- **Expired URLs.** A preload is cancelled on pause, but a stream URL that expires before its
  turn is not refreshed.

### Validation

Nobody could listen to this: the environment has no sound card. The timeline arithmetic is
unit-tested (position before and after the handover, a single crossing, odd write sizes, reset)
and `Track` is checked to be `Send`, but the audible result is not verified.

Manual listening checklist:

- a continuous album or a split mix: no gap at the join;
- repeat one: the same track follows itself without a gap;
- reorder the queue or use Play next in the last 20 s: the right track plays;
- a long pause near the end, then resume: the next track still follows;
- a Jam: there is a gap between tracks, and nothing hangs;
- a device switch near the end of a track: playback continues and positions stay right.

### Consequences

- No new dependency. Two commands and one event are added to `sc-audio`; the callback and the
  ring buffer are untouched.
- One short-lived thread per preload.

## Equalizer

### Decisions

1. **Own code, presets only.** `sc-audio/src/equalizer.rs` is ten RBJ peaking filters (31 Hz to
   16 kHz, one per octave, Q of the square root of 2) in cascade, as transposed direct form II
   biquads on interleaved `f32`. Coefficients are computed in `f64`. No dependency. The person
   picks a preset in Settings; there are no sliders yet.
2. **On the engine thread, never in the callback.** The equalizer runs on the converted samples
   (output rate and channel count) before they enter the ring buffer. The callback is untouched
   and still only multiplies by the volume. The state is allocated when the equalizer is built,
   so processing allocates nothing.
3. **A preamp stops boosts from clipping.** It lowers the signal by the largest boost, so the
   loudest band is at unity. A flat setting skips the equalizer entirely (the samples stay
   bit-identical).
4. **Bands that do not fit are skipped.** A band above 45% of the sample rate is not built (at
   32 kHz the 16 kHz band is left out), and a gain of 0 dB costs nothing.
5. **Changes keep the filter memory.** A new preset recomputes the coefficients and keeps the
   state, so switching presets while playing does not click. A seek, a load or a stop resets it;
   a device switch rebuilds it for the new rate.
6. **Latency.** The ring buffer holds about 0.5 s, so a preset is heard up to that long after it
   is chosen. Accepted.

### Validation

Unit tests on synthetic sines: +6 dB at 1 kHz comes out at the preamp's level, a distant
frequency gets only the preamp, flat gains are bit-identical, nothing turns to NaN from 22.05 to
96 kHz, and the top band is skipped at 32 kHz. Nobody listened to it, and the engine was not run
on a device. The preset tables (in `sc-core`) are proposals, to be tuned by ear.

## Loudness normalization

### Decisions

1. **Measured, not read.** SoundCloud gives no ReplayGain or loudness field, so the engine
   measures. The new dependency `ebur128` 0.1.10 (MIT, approved) computes the integrated loudness
   (EBU R128, histogram mode, so memory stays flat on long tracks).
2. **Progressive, before the resample.** Each track has a `Meter` fed with the decoded samples
   at the source's rate and channels while it is decoded. It answers nothing until 3 s have been
   measured; after that the integrated loudness so far is the estimate. A seek keeps the meter;
   a change of source format creates a new one.
3. **Attenuate only, toward -14 LUFS.** The target gain is `-14 - loudness`, limited to between
   -12 dB and 0 dB. Quiet tracks are left alone (a raise up to +12 dB exists only with the volume
   boost, see below).
4. **Slow.** The applied gain moves toward the target by at most 2 dB per second, and inside each
   chunk it ramps linearly per frame, so it never steps. During the 3 s warm-up the previous
   gain is kept.
5. **The gain carries across tracks, except a positive one.** A cut stays until the next track is
   measured (a loud track after a loud track needs no wait); a boost earned on a quiet track is
   dropped at the start of the next one (`gain_at_track_start`), because that track has not been
   measured and may be loud.
6. **On the engine thread, before the equalizer.** The gain stage runs on the converted samples,
   then the equalizer. The callback is untouched.
7. **Off at start.** The engine starts neutral (normalization off); the core sends the setting
   right after the player is spawned.

### Accepted limits

- The first 3 s of a loud track are not yet turned down, and turning it down takes about 2 dB per
  second afterwards (up to 6 s for -12 dB).
- The estimate is of the audio heard so far, so a track that gets louder late is corrected
  late.
- Turning normalization on or off is heard up to 0.5 s later (the ring buffer).

### Validation

Unit tests on synthetic signals: the target gain, the 2 dB per second limit, the ramp ending
exactly at its target, no inherited boost, a -20 dBFS sine measuring about -23 LUFS after 5 s and
nothing after 1 s. Nobody listened to it, and the engine was not run on a device.
