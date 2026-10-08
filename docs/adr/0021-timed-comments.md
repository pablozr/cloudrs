# ADR 0021: Timed comments on the track page

Status: accepted · 2026-10-07 (approved by the maintainer)

## Context

`docs/PLAN.md` (§7.2, §7.3 and milestone M4) asks for timed comments on the waveform. SoundCloud
comments carry the position in the track where they were left. The track page already shows the
large waveform, so that is where the comments belong. The player bar stays as it is.

## Decisions

1. **One request, no paging.** `SoundCloudApi::comments(track, limit)` calls
   `/tracks/{id}/comments?threaded=0` once, with `limit` capped at 200. The list keeps the API
   order (newest first). `Comment` carries `id`, `body`, `timestamp` (milliseconds, `None` when
   not timed), `created_at` and the embedded `UserSummary`. `Track` gains `commentable`.
2. **Commands and events.** `Command::LoadComments(TrackId)` fetches again after a failure.
   `Event::Comments { track, comments }` and `Event::CommentsFailed { track, problem }` answer it.
   The core sends the comments by itself when a track page opens, right after `Event::TrackPage`.
   It skips the request when `commentable` is `Some(false)` (nothing is sent; the page says
   comments are off) and when `comment_count` is `Some(0)` (an empty `Comments` is sent).
   `TrackPage.commentable` carries the flag.
3. **Avatars on demand.** The core registers each commenter's avatar URL (without downloading it)
   and `Command::LoadArtwork(ArtKey)` asks for the ones the UI shows: the visible list rows and
   the hovered pin's entries. The key is `ArtKey::User`, the same as the profile avatar, so the
   size is the same (`t300x300`) and the first URL registered wins.
4. **Bucketing happens once per bar**, in the app's seam, when `Event::Comments` arrives, using
   the track duration from the track page and `sc_core::WAVEFORM_BARS`. At most one pin per bar;
   comments in the same bar share it. A comment without a time is only in the list. Without a
   duration there are no pins.
5. **The pin lane is drawn by `Shell`, below `TrackWave`**, not inside it. `TrackWave` re-renders
   at about 10 Hz; the lane re-renders only when comments or hover change.
6. **Pins live only on the track page**, not in the player bar.
7. **Hover shows a popover** (avatar, name, time, text) with up to 3 comments of the pin and
   "+N more". Hover notifies only when the pin under the pointer changes.
8. **Click rule.** Clicking a pin or a timed comment of the track that is playing seeks to its
   time. For a track that is not playing, it plays the track from the start; playing from the
   comment's time is a follow-up (`Command::Play { at }`).
9. **Tabs.** The track page shows Related and Comments as tabs. The Comments tab is a virtual
   list with loading (skeleton), empty, error with retry, and "comments are turned off" states.
   Each timed row is focusable and Enter or a click seeks, which is the keyboard path.

## Consequences

- Only the first 200 comments are shown; there is no "load more".
- `models.comments` is unbounded, like `models.tracks`.
- Starting a stopped track at the comment's time needs `Command::Play { at }`, a follow-up.
- No new dependency and no new icon.
- The response fixture is hand-written and the live endpoint was not reachable when this was
  written, so the real JSON shape (for example `timestamp` as a number) should be checked with a
  real track.
