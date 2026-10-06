//! Turning SoundCloud's waveform (about 1800 columns) into the bars the
//! player bar draws.

/// Bars drawn in the player bar.
pub const BARS: usize = 160;

/// Groups `samples` into `bars` bars (peak of each group), scaled to 0..=1.
pub fn to_bars(samples: &[u32], height: u32, bars: usize) -> Vec<f32> {
    if samples.is_empty() || bars == 0 {
        return Vec::new();
    }
    let height = height.max(1) as f32;
    (0..bars)
        .map(|bar| {
            let start = bar * samples.len() / bars;
            let end = ((bar + 1) * samples.len() / bars).max(start + 1);
            let peak = samples[start..end.min(samples.len())]
                .iter()
                .copied()
                .max()
                .unwrap_or(0);
            (peak as f32 / height).clamp(0.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_peak_of_each_group() {
        assert_eq!(to_bars(&[0, 10, 5, 20], 20, 2), [0.5, 1.0]);
    }

    #[test]
    fn stretches_short_waveforms() {
        assert_eq!(to_bars(&[10, 20], 20, 4), [0.5, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn handles_empty_input() {
        assert!(to_bars(&[], 140, 10).is_empty());
    }
}
