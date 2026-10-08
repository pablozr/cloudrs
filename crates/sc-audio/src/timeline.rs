//! Where the sound card is in the queue of tracks it was fed.
//!
//! The engine writes the next track into the ring buffer right behind the
//! current one, so the frames the device has played no longer map to a single
//! track. The timeline remembers where the second track's samples begin and
//! which track a played-frame count belongs to. Pure counting, no I/O.

use std::time::Duration;

/// The point where the buffered audio changes from one track to the next.
#[derive(Debug, Clone, Copy)]
struct Handover {
    /// Frame count (since the last flush) at which the new track starts.
    at: u64,
    /// Start of the track that plays before `at`.
    previous_start: Duration,
    /// The frame count where that track's own timing began.
    previous_base: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Timeline {
    /// Frame count (since the last flush) where the current track began.
    base: u64,
    /// Samples written to the ring buffer since the last flush. Samples, not
    /// frames: a write may stop in the middle of a frame.
    written_samples: u64,
    handover: Option<Handover>,
}

impl Timeline {
    /// After a flush or an output switch: the ring buffer and the played-frame
    /// count both start from zero.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn wrote(&mut self, samples: usize) {
        self.written_samples += samples as u64;
    }

    /// The next track's samples start right after everything written so far.
    /// `previous_start` is the start of the track being left.
    pub(crate) fn begin_handover(&mut self, previous_start: Duration, channels: usize) {
        let at = self.written_samples / channels.max(1) as u64;
        self.handover = Some(Handover {
            at,
            previous_start,
            previous_base: self.base,
        });
        self.base = at;
    }

    /// Position in the track that is playing, given the current track's
    /// `start` and the frames `played` by the device.
    pub(crate) fn position(&self, start: Duration, played: u64, rate: u32) -> Duration {
        let (start, since) = match self.handover {
            Some(h) if played < h.at => (h.previous_start, played.saturating_sub(h.previous_base)),
            _ => (start, played.saturating_sub(self.base)),
        };
        start + Duration::from_secs_f64(since as f64 / f64::from(rate.max(1)))
    }

    /// True exactly once: when the device has played up to the handover.
    pub(crate) fn crossed(&mut self, played: u64) -> bool {
        match self.handover {
            Some(h) if played >= h.at => {
                self.handover = None;
                true
            }
            _ => false,
        }
    }

    /// Treats a pending handover as done. Returns whether there was one.
    pub(crate) fn finish(&mut self) -> bool {
        self.handover.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    #[test]
    fn position_is_the_start_plus_the_frames_played() {
        let timeline = Timeline::default();
        let at = timeline.position(Duration::from_secs(10), 96_000, RATE);
        assert_eq!(at, Duration::from_secs(12));
    }

    #[test]
    fn position_follows_the_previous_track_until_the_handover() {
        let mut timeline = Timeline::default();
        // 200 s of stereo were written for the first track (started at 0).
        timeline.wrote(200 * RATE as usize * 2);
        timeline.begin_handover(Duration::ZERO, 2);
        let next = Duration::ZERO;
        // 1 s before the handover: still the first track, at 199 s.
        assert_eq!(
            timeline.position(next, 199 * u64::from(RATE), RATE),
            Duration::from_secs(199)
        );
        // 1 s after: the second track, at 1 s.
        assert_eq!(
            timeline.position(next, 201 * u64::from(RATE), RATE),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn crossed_is_true_only_once() {
        let mut timeline = Timeline::default();
        timeline.wrote(1000);
        timeline.begin_handover(Duration::ZERO, 2);
        assert!(!timeline.crossed(499));
        assert!(timeline.crossed(500));
        assert!(!timeline.crossed(501));
        assert!(!timeline.finish());
    }

    #[test]
    fn the_handover_is_exact_with_writes_in_odd_chunks() {
        let mut timeline = Timeline::default();
        for _ in 0..7 {
            timeline.wrote(13);
        }
        // 91 samples of stereo: 45 whole frames.
        timeline.begin_handover(Duration::ZERO, 2);
        assert!(!timeline.crossed(44));
        assert!(timeline.crossed(45));
    }

    #[test]
    fn the_second_track_counts_from_its_own_base() {
        let mut timeline = Timeline::default();
        timeline.wrote(2 * RATE as usize * 2);
        timeline.begin_handover(Duration::ZERO, 2);
        assert!(timeline.crossed(2 * u64::from(RATE)));
        let start = Duration::from_secs(5);
        let at = timeline.position(start, 3 * u64::from(RATE), RATE);
        assert_eq!(at, Duration::from_secs(6));
    }

    #[test]
    fn reset_forgets_everything() {
        let mut timeline = Timeline::default();
        timeline.wrote(1000);
        timeline.begin_handover(Duration::from_secs(3), 2);
        timeline.reset();
        assert!(!timeline.crossed(10_000));
        assert!(!timeline.finish());
        assert_eq!(
            timeline.position(Duration::from_secs(1), u64::from(RATE), RATE),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn finish_completes_a_pending_handover() {
        let mut timeline = Timeline::default();
        timeline.wrote(1000);
        timeline.begin_handover(Duration::ZERO, 2);
        assert!(timeline.finish());
        assert!(!timeline.finish());
        assert!(!timeline.crossed(10_000));
    }
}
