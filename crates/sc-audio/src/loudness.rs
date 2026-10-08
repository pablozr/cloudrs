//! Loudness normalization: measure a track while it decodes and steer a gain
//! toward a common level. SoundCloud gives no ReplayGain data, so the
//! measurement is the only source.

use ebur128::{EbuR128, Mode};

/// The level every track is steered toward (the streaming norm).
const TARGET_LUFS: f64 = -14.0;
/// Audio measured before the first estimate is trusted.
const WARMUP_SECONDS: usize = 3;
/// The gain never moves faster than this, so the change is not noticed.
const MAX_DB_PER_SEC: f32 = 2.0;
/// The most a loud track is turned down.
const MIN_GAIN_DB: f32 = -12.0;
/// The most a quiet track is turned up (only with the volume boost on).
const MAX_RAISE_DB: f32 = 12.0;

pub(crate) fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Integrated loudness of what has been added so far.
pub(crate) struct Meter {
    inner: EbuR128,
    frames: usize,
    warmup: usize,
    failed: bool,
}

impl Meter {
    /// `None` when the format is not one `ebur128` can measure.
    pub(crate) fn new(rate: u32, channels: usize) -> Option<Self> {
        let channels = u32::try_from(channels).ok()?;
        let inner = EbuR128::new(channels, rate, Mode::I | Mode::HISTOGRAM).ok()?;
        Some(Self {
            inner,
            frames: 0,
            warmup: rate as usize * WARMUP_SECONDS,
            failed: false,
        })
    }

    pub(crate) fn matches(&self, rate: u32, channels: usize) -> bool {
        self.inner.rate() == rate && self.inner.channels() as usize == channels
    }

    /// Adds interleaved source samples.
    pub(crate) fn add(&mut self, interleaved: &[f32]) {
        if self.failed {
            return;
        }
        match self.inner.add_frames_f32(interleaved) {
            Ok(()) => self.frames += interleaved.len() / self.inner.channels() as usize,
            Err(error) => {
                tracing::warn!(%error, "loudness measurement stopped");
                self.failed = true;
            }
        }
    }

    /// The integrated loudness, once there is enough audio to trust it.
    pub(crate) fn lufs(&self) -> Option<f64> {
        if self.failed || self.frames < self.warmup {
            return None;
        }
        self.inner.loudness_global().ok().filter(|l| l.is_finite())
    }
}

/// The gain in dB that brings `lufs` to the target: it turns loud tracks down
/// and, only when `raise` is set, quiet ones up.
pub(crate) fn target_gain_db(lufs: f64, raise: bool) -> f32 {
    if !lufs.is_finite() {
        return 0.0;
    }
    let ceiling = if raise { MAX_RAISE_DB } else { 0.0 };
    ((TARGET_LUFS - lufs) as f32).clamp(MIN_GAIN_DB, ceiling)
}

/// A boost earned on a quiet track is not carried into the next one, which
/// has not been measured yet; a cut is kept until it is.
pub(crate) fn gain_at_track_start(gain_db: f32) -> f32 {
    gain_db.min(0.0)
}

/// Moves `current` toward `target` by at most [`MAX_DB_PER_SEC`] over `seconds`.
pub(crate) fn step_toward(current: f32, target: f32, seconds: f32) -> f32 {
    let limit = MAX_DB_PER_SEC * seconds;
    current + (target - current).clamp(-limit, limit)
}

/// Multiplies interleaved samples by a gain that moves linearly from `from`
/// to `to` across the frames, ending exactly at `to`.
pub(crate) fn ramp_gain(samples: &mut [f32], channels: usize, from: f32, to: f32) {
    if from == 1.0 && to == 1.0 {
        return;
    }
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    for (i, frame) in samples.chunks_exact_mut(channels).enumerate() {
        let gain = if i + 1 == frames {
            to
        } else {
            from + (to - from) * ((i + 1) as f32 / frames as f32)
        };
        for sample in frame {
            *sample *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loud_tracks_are_turned_down_and_quiet_ones_left_alone() {
        assert_eq!(target_gain_db(-20.0, false), 0.0);
        assert_eq!(target_gain_db(-5.0, false), -9.0);
        assert_eq!(target_gain_db(-40.0, false), 0.0);
        assert_eq!(target_gain_db(f64::NEG_INFINITY, false), 0.0);
        assert_eq!(target_gain_db(f64::NAN, true), 0.0);
        assert_eq!(target_gain_db(-1.0, false), -12.0);
    }

    #[test]
    fn quiet_tracks_are_lifted_only_with_the_boost() {
        assert_eq!(target_gain_db(-20.0, true), 6.0);
        assert_eq!(target_gain_db(-30.0, true), 12.0);
        assert_eq!(target_gain_db(-40.0, true), 12.0);
        assert_eq!(target_gain_db(-5.0, true), -9.0);
    }

    #[test]
    fn a_positive_gain_is_not_inherited() {
        assert_eq!(gain_at_track_start(8.0), 0.0);
        assert_eq!(gain_at_track_start(-3.0), -3.0);
    }

    #[test]
    fn the_gain_moves_at_most_two_db_per_second() {
        assert_eq!(step_toward(0.0, -10.0, 0.5), -1.0);
        assert_eq!(step_toward(0.0, 10.0, 0.5), 1.0);
        assert_eq!(step_toward(-9.5, -10.0, 1.0), -10.0);
        assert_eq!(step_toward(-3.0, -3.0, 1.0), -3.0);
    }

    #[test]
    fn a_ramp_ends_exactly_at_its_target() {
        let mut samples = vec![1.0; 20];
        ramp_gain(&mut samples, 2, 1.0, 0.5);
        assert_eq!(&samples[18..], &[0.5, 0.5]);
        assert!(samples.windows(2).all(|w| w[1] <= w[0]));
        // Both channels of a frame get the same gain.
        assert!(samples.as_chunks::<2>().0.iter().all(|f| f[0] == f[1]));
    }

    #[test]
    fn unity_gain_changes_nothing() {
        let mut samples = vec![0.3, -0.7, 0.1, 0.9];
        let before = samples.clone();
        ramp_gain(&mut samples, 2, 1.0, 1.0);
        assert_eq!(samples, before);
    }

    fn sine(rate: u32, seconds: usize) -> Vec<f32> {
        // -20 dBFS peak.
        (0..rate as usize * seconds)
            .map(|i| {
                (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / f64::from(rate)).sin() as f32
                    * 0.1
            })
            .collect()
    }

    #[test]
    fn a_sine_is_measured_after_the_warmup() {
        let mut meter = Meter::new(48_000, 1).unwrap();
        meter.add(&sine(48_000, 1));
        assert_eq!(meter.lufs(), None);
        meter.add(&sine(48_000, 4));
        // A -20 dBFS peak is -23 dBFS RMS.
        let lufs = meter.lufs().unwrap();
        assert!((lufs + 23.0).abs() < 1.0, "{lufs}");
    }

    #[test]
    fn silence_has_no_loudness() {
        let mut meter = Meter::new(48_000, 1).unwrap();
        meter.add(&vec![0.0; 48_000 * 4]);
        assert_eq!(meter.lufs(), None);
    }
}
