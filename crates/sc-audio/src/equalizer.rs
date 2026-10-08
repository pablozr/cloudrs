//! A ten-band graphic equalizer on the engine thread: one RBJ peaking filter
//! per band, in cascade, with a preamp so boosts never clip.

use std::f64::consts::{PI, SQRT_2};

use crate::loudness::db_to_gain;

/// Bands in the equalizer, from 31 Hz to 16 kHz.
pub const EQ_BANDS: usize = 10;

const FREQUENCIES: [f64; EQ_BANDS] = [
    31.25, 62.5, 125., 250., 500., 1000., 2000., 4000., 8000., 16000.,
];
/// Bands above this fraction of the sample rate are skipped: the filter
/// would misbehave close to Nyquist.
const MAX_FRACTION: f64 = 0.45;
/// Below this magnitude a filter state is flushed to zero (denormals).
const TINY: f32 = 1e-20;

/// Normalized biquad coefficients (a0 = 1).
#[derive(Debug, Clone, Copy)]
struct Coefficients {
    band: usize,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// RBJ peakingEQ, computed in f64.
fn peaking(band: usize, rate: u32, gain_db: f32) -> Coefficients {
    let a = 10.0_f64.powf(f64::from(gain_db) / 40.0);
    let w0 = 2.0 * PI * FREQUENCIES[band] / f64::from(rate);
    let alpha = w0.sin() / (2.0 * SQRT_2);
    let cos = w0.cos();
    let a0 = 1.0 + alpha / a;
    Coefficients {
        band,
        b0: ((1.0 + alpha * a) / a0) as f32,
        b1: (-2.0 * cos / a0) as f32,
        b2: ((1.0 - alpha * a) / a0) as f32,
        a1: (-2.0 * cos / a0) as f32,
        a2: ((1.0 - alpha / a) / a0) as f32,
    }
}

#[derive(Debug)]
pub(crate) struct Equalizer {
    channels: usize,
    bands: Vec<Coefficients>,
    /// Two delays per band per channel, indexed by band so a change of gains
    /// keeps what each filter remembers.
    state: Vec<f32>,
    preamp: f32,
}

impl Equalizer {
    pub(crate) fn new(rate: u32, channels: usize, gains_db: [f32; EQ_BANDS]) -> Self {
        let channels = channels.max(1);
        let mut eq = Self {
            channels,
            bands: Vec::with_capacity(EQ_BANDS),
            state: vec![0.0; EQ_BANDS * 2 * channels],
            preamp: 1.0,
        };
        eq.set_gains(rate, gains_db);
        eq
    }

    /// Recomputes the filters, keeping their state.
    pub(crate) fn set_gains(&mut self, rate: u32, gains_db: [f32; EQ_BANDS]) {
        self.bands.clear();
        for (band, &gain) in gains_db.iter().enumerate() {
            let usable = FREQUENCIES[band] < MAX_FRACTION * f64::from(rate);
            if gain != 0.0 && gain.is_finite() && usable {
                self.bands.push(peaking(band, rate, gain));
            }
        }
        let boost = gains_db
            .iter()
            .copied()
            .filter(|g| g.is_finite())
            .fold(0.0_f32, f32::max);
        self.preamp = db_to_gain(-boost);
    }

    pub(crate) fn reset(&mut self) {
        self.state.fill(0.0);
    }

    /// Filters interleaved samples in place.
    pub(crate) fn process(&mut self, samples: &mut [f32]) {
        if self.bands.is_empty() {
            return;
        }
        let per_channel = EQ_BANDS * 2;
        for frame in samples.chunks_exact_mut(self.channels) {
            for (c, sample) in frame.iter_mut().enumerate() {
                let state = &mut self.state[c * per_channel..(c + 1) * per_channel];
                let mut x = *sample * self.preamp;
                for k in &self.bands {
                    let s = &mut state[k.band * 2..k.band * 2 + 2];
                    let y = k.b0 * x + s[0];
                    s[0] = k.b1 * x - k.a1 * y + s[1];
                    s[1] = k.b2 * x - k.a2 * y;
                    if s[0].abs() < TINY {
                        s[0] = 0.0;
                    }
                    if s[1].abs() < TINY {
                        s[1] = 0.0;
                    }
                    x = y;
                }
                *sample = x;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, channels: usize, hz: f64, seconds: usize) -> Vec<f32> {
        (0..rate as usize * seconds)
            .flat_map(|i| {
                let v = (2.0 * PI * hz * i as f64 / f64::from(rate)).sin() as f32 * 0.5;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    /// RMS of the second half, in dB relative to `reference`.
    fn gain_db(output: &[f32], reference: &[f32]) -> f32 {
        let rms = |s: &[f32]| {
            let half = &s[s.len() / 2..];
            (half.iter().map(|v| v * v).sum::<f32>() / half.len() as f32).sqrt()
        };
        20.0 * (rms(output) / rms(reference)).log10()
    }

    fn gains(band: usize, db: f32) -> [f32; EQ_BANDS] {
        let mut g = [0.0; EQ_BANDS];
        g[band] = db;
        g
    }

    #[test]
    fn a_boosted_band_rises_by_its_gain_plus_the_preamp() {
        let input = sine(48_000, 2, 1000.0, 2);
        let mut out = input.clone();
        Equalizer::new(48_000, 2, gains(5, 6.0)).process(&mut out);
        // +6 dB at 1 kHz and a -6 dB preamp.
        assert!(
            gain_db(&out, &input).abs() < 0.2,
            "{}",
            gain_db(&out, &input)
        );
    }

    #[test]
    fn a_distant_frequency_only_gets_the_preamp() {
        let input = sine(48_000, 2, 100.0, 2);
        let mut out = input.clone();
        Equalizer::new(48_000, 2, gains(5, 6.0)).process(&mut out);
        assert!(
            (gain_db(&out, &input) + 6.0).abs() < 0.5,
            "{}",
            gain_db(&out, &input)
        );
    }

    #[test]
    fn zero_gains_leave_the_signal_untouched() {
        let input = sine(44_100, 2, 440.0, 1);
        let mut out = input.clone();
        Equalizer::new(44_100, 2, [0.0; EQ_BANDS]).process(&mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn no_rate_makes_nan() {
        for rate in [22_050, 32_000, 44_100, 48_000, 96_000] {
            let mut out = sine(rate, 2, 1000.0, 1);
            Equalizer::new(rate, 2, [6.0; EQ_BANDS]).process(&mut out);
            assert!(out.iter().all(|v| v.is_finite()), "{rate}");
        }
    }

    #[test]
    fn bands_near_nyquist_are_skipped() {
        let eq = Equalizer::new(32_000, 2, [6.0; EQ_BANDS]);
        assert!(eq.bands.iter().all(|b| b.band != 9));
        assert_eq!(eq.bands.len(), EQ_BANDS - 1);
        let eq = Equalizer::new(48_000, 2, [6.0; EQ_BANDS]);
        assert_eq!(eq.bands.len(), EQ_BANDS);
    }

    #[test]
    fn reset_clears_the_memory() {
        let mut eq = Equalizer::new(48_000, 1, gains(2, 9.0));
        let mut out = sine(48_000, 1, 125.0, 1);
        eq.process(&mut out);
        eq.reset();
        assert!(eq.state.iter().all(|&v| v == 0.0));
    }
}
