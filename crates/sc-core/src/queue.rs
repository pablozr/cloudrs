//! The play queue as pure data: no I/O, so every rule is a unit test.
//!
//! The list is what the person sees, in play order. "Up next" items sit right
//! after the current track, ahead of the rest of the context. Shuffle reorders
//! what comes after them and remembers the original order to restore it.

use crate::types::{QueueSnapshot, Repeat, TrackId, TrackSummary};

/// A queued track. The key tells apart the same track queued twice.
#[derive(Debug, Clone)]
struct Entry {
    key: u64,
    track: TrackSummary,
}

/// Where playback goes after [`Queue::next`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Play the track at this index.
    Play(usize),
    /// The queue is over: the caller may fetch more (autoplay).
    End,
}

#[derive(Debug)]
pub struct Queue {
    entries: Vec<Entry>,
    current: Option<usize>,
    /// How many entries right after `current` were queued by "play next" or
    /// "add to queue".
    up_next: usize,
    shuffle: bool,
    repeat: Repeat,
    /// Keys in the order the context had before shuffling.
    original: Vec<u64>,
    next_key: u64,
    rng: u64,
}

impl Queue {
    /// `seed` drives the shuffle; tests pass a fixed one.
    pub fn new(seed: u64) -> Self {
        Self {
            entries: Vec::new(),
            current: None,
            up_next: 0,
            shuffle: false,
            repeat: Repeat::Off,
            original: Vec::new(),
            next_key: 0,
            rng: seed | 1,
        }
    }

    fn entry(&mut self, track: TrackSummary) -> Entry {
        self.next_key += 1;
        Entry {
            key: self.next_key,
            track,
        }
    }

    /// xorshift64*: plenty for shuffling a playlist.
    fn random_below(&mut self, bound: usize) -> usize {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let value = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (value >> 33) as usize % bound
    }

    /// Fisher-Yates over `entries[from..]`.
    fn shuffle_from(&mut self, from: usize) {
        for i in (from + 1..self.entries.len()).rev() {
            let j = from + self.random_below(i - from + 1);
            self.entries.swap(i, j);
        }
    }

    pub fn current_track(&self) -> Option<&TrackSummary> {
        self.current.map(|i| &self.entries[i].track)
    }

    pub fn last_track(&self) -> Option<&TrackSummary> {
        self.entries.last().map(|e| &e.track)
    }

    pub fn contains(&self, id: TrackId) -> bool {
        self.entries.iter().any(|e| e.track.id == id)
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            tracks: self.entries.iter().map(|e| e.track.clone()).collect(),
            current: self.current,
            shuffle: self.shuffle,
            repeat: self.repeat,
        }
    }

    /// Brings back a saved queue. The pre-shuffle order is not saved, so turning
    /// shuffle off afterwards keeps the restored order.
    pub fn restore(
        &mut self,
        tracks: Vec<TrackSummary>,
        current: Option<usize>,
        shuffle: bool,
        repeat: Repeat,
    ) {
        self.entries = tracks.into_iter().map(|t| self.entry(t)).collect();
        self.current = current.filter(|i| *i < self.entries.len());
        self.up_next = 0;
        self.shuffle = shuffle;
        self.repeat = repeat;
        self.original = self.entries.iter().map(|e| e.key).collect();
    }

    /// Replaces the queue with a list, starting at `start`. With shuffle on,
    /// the start track goes first and the rest is shuffled.
    pub fn set_context(&mut self, tracks: Vec<TrackSummary>, start: usize) {
        self.entries = tracks.into_iter().map(|t| self.entry(t)).collect();
        self.up_next = 0;
        self.original = self.entries.iter().map(|e| e.key).collect();
        if self.entries.is_empty() {
            self.current = None;
            return;
        }
        let start = start.min(self.entries.len() - 1);
        if self.shuffle {
            self.entries.swap(0, start);
            self.shuffle_from(1);
            self.current = Some(0);
        } else {
            self.current = Some(start);
        }
    }

    /// Plays right after the current track, before everything else queued.
    pub fn play_next(&mut self, track: TrackSummary) {
        let at = self.current.map_or(0, |c| c + 1);
        self.insert_up_next(at, track);
    }

    /// Plays after the other "up next" items, before the rest of the context.
    pub fn add_to_queue(&mut self, track: TrackSummary) {
        let at = self.current.map_or(0, |c| c + 1 + self.up_next);
        self.insert_up_next(at, track);
    }

    fn insert_up_next(&mut self, at: usize, track: TrackSummary) {
        let entry = self.entry(track);
        self.original.push(entry.key);
        self.entries.insert(at, entry);
        self.up_next += 1;
    }

    /// Appends tracks at the end of the context (autoplay), skipping any that
    /// are already queued. Returns how many were added.
    pub fn extend_context(&mut self, tracks: Vec<TrackSummary>) -> usize {
        let mut added = 0;
        for track in tracks {
            if self.contains(track.id) {
                continue;
            }
            let entry = self.entry(track);
            self.original.push(entry.key);
            self.entries.push(entry);
            added += 1;
        }
        added
    }

    /// Removes a queued track. The current one cannot be removed.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.entries.len() || self.current == Some(index) {
            return false;
        }
        let entry = self.entries.remove(index);
        self.original.retain(|key| *key != entry.key);
        if let Some(current) = self.current {
            if index < current {
                self.current = Some(current - 1);
            } else if index <= current + self.up_next {
                self.up_next -= 1;
            }
        }
        true
    }

    /// Moves a track; the current track keeps playing wherever it ends up.
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        let len = self.entries.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        if let Some(current) = self.current {
            self.current = Some(if current == from {
                to
            } else if from < current && current <= to {
                current - 1
            } else if to <= current && current < from {
                current + 1
            } else {
                current
            });
        }
        // Reordered items are plain queue content from now on.
        self.up_next = 0;
        true
    }

    /// Makes `index` the current track.
    pub fn play_index(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.jump_to(index);
        true
    }

    fn jump_to(&mut self, index: usize) {
        match self.current {
            Some(current) if index > current => {
                self.up_next = self.up_next.saturating_sub(index - current);
            }
            Some(current) if index < current => self.up_next = 0,
            None => self.up_next = 0,
            _ => {}
        }
        self.current = Some(index);
    }

    /// Moves to the next track. `ended` is true when the track finished by
    /// itself, which is the only case where repeat-one stays on it.
    pub fn next(&mut self, ended: bool) -> Step {
        let Some(current) = self.current else {
            return Step::End;
        };
        if ended && self.repeat == Repeat::One {
            return Step::Play(current);
        }
        if current + 1 < self.entries.len() {
            self.jump_to(current + 1);
            return Step::Play(current + 1);
        }
        if self.repeat == Repeat::All {
            self.jump_to(0);
            return Step::Play(0);
        }
        Step::End
    }

    /// The track before the current one; the first track plays again.
    pub fn previous(&mut self) -> Option<usize> {
        let current = self.current?;
        let target = current.saturating_sub(1);
        self.jump_to(target);
        Some(target)
    }

    pub fn set_shuffle(&mut self, on: bool) -> bool {
        if on == self.shuffle {
            return false;
        }
        self.shuffle = on;
        if on {
            self.original = self.entries.iter().map(|e| e.key).collect();
            let from = self.current.map_or(0, |c| c + 1 + self.up_next);
            self.shuffle_from(from.min(self.entries.len()));
        } else {
            self.unshuffle();
        }
        true
    }

    fn unshuffle(&mut self) {
        let current_key = self.current.map(|i| self.entries[i].key);
        let position = |key: u64| {
            self.original
                .iter()
                .position(|k| *k == key)
                .unwrap_or(usize::MAX)
        };
        let mut keyed: Vec<(usize, Entry)> = std::mem::take(&mut self.entries)
            .into_iter()
            .map(|e| (position(e.key), e))
            .collect();
        // Stable: entries added since shuffling keep their relative order, last.
        keyed.sort_by_key(|(at, _)| *at);
        self.entries = keyed.into_iter().map(|(_, e)| e).collect();
        self.current = current_key.and_then(|key| self.entries.iter().position(|e| e.key == key));
        self.up_next = 0;
        self.original.clear();
    }

    pub fn set_repeat(&mut self, repeat: Repeat) -> bool {
        let changed = self.repeat != repeat;
        self.repeat = repeat;
        changed
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn track(id: u64) -> TrackSummary {
        TrackSummary {
            id: TrackId(id),
            title: format!("T{id}"),
            artist: "A".into(),
            duration: Duration::from_secs(200),
            preview_only: false,
        }
    }

    fn queue(ids: &[u64], start: usize) -> Queue {
        let mut q = Queue::new(7);
        q.set_context(ids.iter().map(|id| track(*id)).collect(), start);
        q
    }

    fn ids(q: &Queue) -> Vec<u64> {
        q.snapshot().tracks.iter().map(|t| t.id.0).collect()
    }

    fn current_id(q: &Queue) -> Option<u64> {
        q.current_track().map(|t| t.id.0)
    }

    #[test]
    fn a_context_starts_at_the_chosen_track() {
        let q = queue(&[1, 2, 3, 4], 2);
        assert_eq!(ids(&q), [1, 2, 3, 4]);
        assert_eq!(q.snapshot().current, Some(2));
    }

    #[test]
    fn next_walks_the_list_then_ends() {
        let mut q = queue(&[1, 2, 3], 1);
        assert_eq!(q.next(true), Step::Play(2));
        assert_eq!(q.next(true), Step::End);
        assert_eq!(current_id(&q), Some(3), "the last track stays current");
    }

    #[test]
    fn repeat_all_wraps_and_repeat_one_only_repeats_on_a_natural_end() {
        let mut q = queue(&[1, 2], 1);
        q.set_repeat(Repeat::All);
        assert_eq!(q.next(true), Step::Play(0));

        q.set_repeat(Repeat::One);
        assert_eq!(q.next(true), Step::Play(0));
        assert_eq!(q.next(false), Step::Play(1), "manual next moves on");
    }

    #[test]
    fn previous_goes_back_and_stops_at_the_first_track() {
        let mut q = queue(&[1, 2, 3], 2);
        assert_eq!(q.previous(), Some(1));
        assert_eq!(q.previous(), Some(0));
        assert_eq!(q.previous(), Some(0));
    }

    #[test]
    fn play_next_goes_first_and_add_to_queue_goes_after_the_up_next_block() {
        let mut q = queue(&[1, 2, 3], 0);
        q.add_to_queue(track(10));
        q.add_to_queue(track(11));
        q.play_next(track(12));
        assert_eq!(ids(&q), [1, 12, 10, 11, 2, 3]);
    }

    #[test]
    fn up_next_items_play_before_the_rest_of_the_context() {
        let mut q = queue(&[1, 2], 0);
        q.add_to_queue(track(10));
        assert_eq!(q.next(true), Step::Play(1));
        assert_eq!(current_id(&q), Some(10));
        // The block is consumed: a new one lands after the current track again.
        q.add_to_queue(track(11));
        assert_eq!(ids(&q), [1, 10, 11, 2]);
    }

    #[test]
    fn remove_keeps_the_current_track_and_refuses_to_remove_it() {
        let mut q = queue(&[1, 2, 3, 4], 2);
        assert!(!q.remove(2), "the current track stays");
        assert!(q.remove(0));
        assert_eq!(ids(&q), [2, 3, 4]);
        assert_eq!(current_id(&q), Some(3));
        assert!(q.remove(2));
        assert_eq!(ids(&q), [2, 3]);
        assert!(!q.remove(9));
    }

    #[test]
    fn removing_an_up_next_item_shrinks_the_block() {
        let mut q = queue(&[1, 2], 0);
        q.add_to_queue(track(10));
        q.add_to_queue(track(11));
        assert!(q.remove(1));
        q.add_to_queue(track(12));
        assert_eq!(ids(&q), [1, 11, 12, 2]);
    }

    #[test]
    fn move_follows_the_current_track() {
        let mut q = queue(&[1, 2, 3, 4], 1);
        assert!(q.move_item(1, 3), "the current track itself");
        assert_eq!(ids(&q), [1, 3, 4, 2]);
        assert_eq!(current_id(&q), Some(2));

        let mut q = queue(&[1, 2, 3, 4], 1);
        assert!(q.move_item(0, 3), "from before to after");
        assert_eq!(ids(&q), [2, 3, 4, 1]);
        assert_eq!(current_id(&q), Some(2));

        let mut q = queue(&[1, 2, 3, 4], 1);
        assert!(q.move_item(3, 0), "from after to before");
        assert_eq!(ids(&q), [4, 1, 2, 3]);
        assert_eq!(current_id(&q), Some(2));

        assert!(!q.move_item(1, 1));
        assert!(!q.move_item(0, 9));
    }

    #[test]
    fn play_index_jumps() {
        let mut q = queue(&[1, 2, 3], 0);
        assert!(q.play_index(2));
        assert_eq!(current_id(&q), Some(3));
        assert!(!q.play_index(3));
    }

    #[test]
    fn shuffle_keeps_the_current_track_and_turning_it_off_restores_the_order() {
        let all: Vec<u64> = (1..=12).collect();
        let mut q = queue(&all, 3);
        q.set_shuffle(true);
        assert_eq!(current_id(&q), Some(4));
        assert_eq!(q.snapshot().current, Some(3), "tracks before it stay put");
        assert_eq!(ids(&q)[..4], [1, 2, 3, 4]);
        let mut sorted = ids(&q);
        sorted.sort_unstable();
        assert_eq!(sorted, all);
        assert_ne!(ids(&q), all, "the rest was shuffled");

        q.set_shuffle(false);
        assert_eq!(ids(&q), all);
        assert_eq!(current_id(&q), Some(4));
        assert_eq!(q.snapshot().current, Some(3));
    }

    #[test]
    fn a_new_context_with_shuffle_on_starts_with_the_chosen_track() {
        let mut q = Queue::new(3);
        q.set_shuffle(true);
        q.set_context((1..=8).map(track).collect(), 5);
        assert_eq!(current_id(&q), Some(6));
        assert_eq!(q.snapshot().current, Some(0));
        q.set_shuffle(false);
        assert_eq!(ids(&q), (1..=8).collect::<Vec<_>>());
    }

    #[test]
    fn tracks_added_while_shuffled_come_back_last_when_unshuffled() {
        let mut q = queue(&[1, 2, 3, 4, 5], 0);
        q.set_shuffle(true);
        q.extend_context(vec![track(9)]);
        q.set_shuffle(false);
        assert_eq!(ids(&q), [1, 2, 3, 4, 5, 9]);
    }

    #[test]
    fn the_same_track_queued_twice_is_two_entries() {
        let mut q = queue(&[1, 2], 0);
        q.add_to_queue(track(1));
        q.set_shuffle(true);
        q.set_shuffle(false);
        assert_eq!(ids(&q), [1, 1, 2]);
    }

    #[test]
    fn extend_context_skips_tracks_already_queued() {
        let mut q = queue(&[1, 2], 1);
        assert_eq!(q.extend_context(vec![track(2), track(7), track(8)]), 2);
        assert_eq!(ids(&q), [1, 2, 7, 8]);
        assert_eq!(q.next(true), Step::Play(2));
    }

    #[test]
    fn an_empty_queue_has_nothing_to_play() {
        let mut q = Queue::new(1);
        assert_eq!(q.next(true), Step::End);
        assert_eq!(q.previous(), None);
        q.set_context(Vec::new(), 0);
        assert_eq!(q.snapshot().current, None);
    }

    #[test]
    fn restore_brings_back_the_saved_state() {
        let mut q = Queue::new(1);
        q.restore(vec![track(1), track(2)], Some(1), true, Repeat::All);
        let snapshot = q.snapshot();
        assert_eq!(snapshot.current, Some(1));
        assert!(snapshot.shuffle);
        assert_eq!(snapshot.repeat, Repeat::All);
        q.set_shuffle(false);
        assert_eq!(ids(&q), [1, 2]);
    }
}
