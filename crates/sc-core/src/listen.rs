//! Counts how long a track has really been listened to, for the history.

use std::time::Duration;

/// A track counts as played after this long.
const THRESHOLD: Duration = Duration::from_secs(30);
/// Position jumps larger than this are seeks, not listening time.
const MAX_STEP: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub struct Listened {
    total: Duration,
    last: Duration,
    recorded: bool,
}

impl Listened {
    /// Starts counting again for a new play.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Feeds a position report. Returns true once per play, when the
    /// threshold is crossed.
    pub fn tick(&mut self, position: Duration) -> bool {
        if let Some(step) = position.checked_sub(self.last)
            && step <= MAX_STEP
        {
            self.total += step;
        }
        self.last = position;
        if self.recorded || self.total < THRESHOLD {
            return false;
        }
        self.recorded = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn counts_once_after_thirty_seconds_of_listening() {
        let mut listened = Listened::default();
        let fired: Vec<u64> = (1..=40).filter(|s| listened.tick(secs(*s))).collect();
        assert_eq!(fired, [30]);
    }

    #[test]
    fn seeking_forward_does_not_count_as_listening() {
        let mut listened = Listened::default();
        listened.tick(secs(1));
        assert!(!listened.tick(secs(120)));
        assert!(!listened.tick(secs(121)));
    }

    #[test]
    fn seeking_back_keeps_the_time_already_heard() {
        let mut listened = Listened::default();
        for s in 1..=20 {
            listened.tick(secs(s));
        }
        listened.tick(secs(0));
        let fired: Vec<u64> = (1..=15).filter(|s| listened.tick(secs(*s))).collect();
        assert_eq!(fired, [10]);
    }

    #[test]
    fn reset_allows_another_record_for_the_next_play() {
        let mut listened = Listened::default();
        assert!((1..=30).any(|s| listened.tick(secs(s))));
        listened.reset();
        assert!((1..=30).any(|s| listened.tick(secs(s))));
    }
}
