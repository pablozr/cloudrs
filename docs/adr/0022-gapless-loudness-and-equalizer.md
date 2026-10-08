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
