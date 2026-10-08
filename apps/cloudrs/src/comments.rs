//! A track's comments as the track page shows them (ADR 0021): the list, and
//! the pins under the waveform, at most one per bar. Plain data, no gpui.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use sc_core::{CommentSummary, WAVEFORM_BARS};

/// Comments a pin's popover lists; the rest read "+N more".
pub const POPOVER_COMMENTS: usize = 3;

/// The comments that share one bar of the waveform.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub bar: u16,
    /// The earliest of its comments: where a click goes.
    pub at: Duration,
    /// Indices into `CommentsView::items`, earliest first.
    pub comments: Box<[u32]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommentsView {
    /// As the core sent them (newest first).
    pub items: Vec<CommentSummary>,
    /// By bar, left to right.
    pub markers: Vec<Marker>,
    /// The bar of each marker, for the lane to draw.
    pub pins: Arc<[u16]>,
}

impl CommentsView {
    /// Buckets the timed comments into the bars of a waveform of `duration`.
    /// A zero duration (not known yet) gives no pins.
    pub fn new(items: Vec<CommentSummary>, duration: Duration) -> Self {
        let mut by_bar: BTreeMap<u16, Vec<u32>> = BTreeMap::new();
        for (ix, item) in items.iter().enumerate() {
            let bar = item.at.and_then(|at| bar_of(at, duration, WAVEFORM_BARS));
            if let Some(bar) = bar {
                by_bar.entry(bar).or_default().push(ix as u32);
            }
        }
        let markers: Vec<Marker> = by_bar
            .into_iter()
            .map(|(bar, mut comments)| {
                comments.sort_by_key(|ix| items[*ix as usize].at);
                Marker {
                    bar,
                    at: items[comments[0] as usize].at.unwrap_or_default(),
                    comments: comments.into(),
                }
            })
            .collect();
        let pins = markers.iter().map(|marker| marker.bar).collect();
        Self {
            items,
            markers,
            pins,
        }
    }
}

/// The bar `at` falls in, among `bars` bars spread over `duration`. A time
/// past the end lands on the last bar.
pub fn bar_of(at: Duration, duration: Duration, bars: usize) -> Option<u16> {
    if duration.is_zero() || bars == 0 {
        return None;
    }
    let bar = (at.as_secs_f64() / duration.as_secs_f64() * bars as f64).floor() as usize;
    Some(bar.min(bars - 1) as u16)
}

/// The marker nearest to the pointer at `fraction` (0..=1) of the strip, if
/// it is at most one bar away.
pub fn marker_near(markers: &[Marker], fraction: f32, bars: usize) -> Option<usize> {
    if bars == 0 {
        return None;
    }
    let pointer = ((fraction * bars as f32).floor() as i64).clamp(0, bars as i64 - 1);
    markers
        .iter()
        .enumerate()
        .map(|(ix, marker)| (ix, (i64::from(marker.bar) - pointer).abs()))
        .filter(|(_, distance)| *distance <= 1)
        .min_by_key(|(_, distance)| *distance)
        .map(|(ix, _)| ix)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DURATION: Duration = Duration::from_secs(160);

    fn comment(id: u64, at: Option<u64>) -> CommentSummary {
        CommentSummary {
            id,
            user: None,
            username: "Ana".into(),
            body: "hi".into(),
            at: at.map(Duration::from_millis),
        }
    }

    #[test]
    fn two_comments_in_the_same_bar_make_one_marker() {
        let view = CommentsView::new(
            vec![comment(1, Some(10_500)), comment(2, Some(10_000))],
            DURATION,
        );
        assert_eq!(view.markers.len(), 1);
        let marker = &view.markers[0];
        assert_eq!(marker.bar, 10);
        assert_eq!(marker.at, Duration::from_secs(10));
        assert_eq!(marker.comments.as_ref(), [1, 0], "earliest first");
        assert_eq!(view.pins.as_ref(), [10]);
    }

    #[test]
    fn an_untimed_comment_stays_in_the_list_without_a_pin() {
        let view = CommentsView::new(vec![comment(1, None), comment(2, Some(5_000))], DURATION);
        assert_eq!(view.items.len(), 2);
        assert_eq!(view.pins.as_ref(), [5]);
    }

    #[test]
    fn a_time_past_the_end_lands_on_the_last_bar() {
        assert_eq!(bar_of(Duration::from_secs(500), DURATION, 160), Some(159));
        assert_eq!(bar_of(DURATION, DURATION, 160), Some(159));
        assert_eq!(bar_of(Duration::ZERO, DURATION, 160), Some(0));
    }

    #[test]
    fn a_zero_duration_gives_no_pins() {
        let view = CommentsView::new(vec![comment(1, Some(5_000))], Duration::ZERO);
        assert!(view.markers.is_empty() && view.pins.is_empty());
        assert_eq!(view.items.len(), 1);
    }

    #[test]
    fn markers_come_left_to_right() {
        let view = CommentsView::new(
            vec![comment(1, Some(90_000)), comment(2, Some(3_000))],
            DURATION,
        );
        assert_eq!(view.pins.as_ref(), [3, 90]);
    }

    #[test]
    fn the_pointer_finds_the_exact_bar_and_one_either_side() {
        let view = CommentsView::new(vec![comment(1, Some(80_000))], DURATION);
        let near = |bar: f32| marker_near(&view.markers, (bar + 0.5) / 160.0, 160);
        assert_eq!(near(80.0), Some(0));
        assert_eq!(near(79.0), Some(0));
        assert_eq!(near(81.0), Some(0));
        assert_eq!(near(78.0), None);
        assert_eq!(near(82.0), None);
    }

    #[test]
    fn the_pointer_prefers_the_closer_marker() {
        let view = CommentsView::new(
            vec![comment(1, Some(80_000)), comment(2, Some(82_000))],
            DURATION,
        );
        // The exact bar wins over a neighbour.
        assert_eq!(marker_near(&view.markers, 82.5 / 160.0, 160), Some(1));
        assert_eq!(marker_near(&view.markers, 80.5 / 160.0, 160), Some(0));
    }
}
