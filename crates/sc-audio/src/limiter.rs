//! A look-ahead peak limiter for the volume boost. A boost above 100% turns
//! peaks into samples over full scale, and clipping them distorts; instead the
//! gain is lowered smoothly *before* each peak arrives, then released slowly.

use std::collections::VecDeque;

/// -1 dBFS: no output sample goes above this.
const CEILING: f32 = 0.891_251;
/// How far ahead of a peak the gain starts to fall.
const LOOKAHEAD_SECONDS: f64 = 0.005;
/// Time constant of the release.
const RELEASE_SECONDS: f64 = 0.15;

pub(crate) struct Limiter {
    channels: usize,
    /// Look-ahead in frames; also the delay the limiter adds.
    len: usize,
    /// Frames waiting for their gain.
    delay: VecDeque<f32>,
    /// Sliding minimum of the wanted gain over the last `len + 1` frames
    /// (monotonic deque of frame index and gain).
    mins: VecDeque<(u64, f64)>,
    /// The last `len + 1` values of the envelope, and their sum.
    boxes: VecDeque<f64>,
    box_sum: f64,
    env: f64,
    release: f64,
    index: u64,
}

impl Limiter {
    /// Allocates everything the limiter will ever need.
    pub(crate) fn new(rate: u32, channels: usize) -> Self {
        let channels = channels.max(1);
        let len = ((LOOKAHEAD_SECONDS * f64::from(rate)).round() as usize).max(1);
        let mut limiter = Self {
            channels,
            len,
            delay: VecDeque::with_capacity((len + 1) * channels),
            mins: VecDeque::with_capacity(len + 2),
            boxes: VecDeque::with_capacity(len + 1),
            box_sum: 0.0,
            env: 1.0,
            release: 1.0 - (-1.0 / (RELEASE_SECONDS * f64::from(rate))).exp(),
            index: 0,
        };
        limiter.reset();
        limiter
    }

    /// Forgets everything held and goes back to unity gain.
    pub(crate) fn reset(&mut self) {
        self.delay.clear();
        self.mins.clear();
        self.boxes.clear();
        self.boxes.resize(self.len + 1, 1.0);
        self.box_sum = (self.len + 1) as f64;
        self.env = 1.0;
        self.index = 0;
    }

    /// The gain for the next frame, whose loudest sample is `peak`.
    fn gain(&mut self, peak: f32) -> f32 {
        let wanted = if peak > CEILING {
            f64::from(CEILING / peak)
        } else {
            1.0
        };
        let n = self.index;
        self.index += 1;
        while self.mins.back().is_some_and(|&(_, v)| v >= wanted) {
            self.mins.pop_back();
        }
        self.mins.push_back((n, wanted));
        let window = self.len as u64 + 1;
        while self.mins.front().is_some_and(|&(i, _)| i + window <= n) {
            self.mins.pop_front();
        }
        let lowest = self.mins.front().map_or(1.0, |&(_, v)| v);
        self.env = lowest.min(self.env + (1.0 - self.env) * self.release);
        if self.boxes.len() > self.len
            && let Some(old) = self.boxes.pop_front()
        {
            self.box_sum -= old;
        }
        self.boxes.push_back(self.env);
        self.box_sum += self.env;
        let full = (self.len + 1) as f64;
        if self.box_sum >= full - 1e-9 {
            1.0
        } else {
            (self.box_sum / full) as f32
        }
    }

    /// Limits `buf` (interleaved) in place. The output lags the input by the
    /// look-ahead, so right after a reset it holds fewer frames than it was
    /// given; the rest come out later, or from [`Self::drain`].
    pub(crate) fn process(&mut self, buf: &mut Vec<f32>) {
        let ch = self.channels;
        let frames = buf.len() / ch;
        let mut written = 0;
        for i in 0..frames {
            let frame = &buf[i * ch..(i + 1) * ch];
            let peak = frame.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
            let gain = self.gain(peak);
            self.delay.extend(frame.iter().copied());
            if self.delay.len() > self.len * ch {
                for c in 0..ch {
                    let held = self.delay.pop_front().unwrap_or(0.0);
                    buf[written * ch + c] = (held * gain).clamp(-CEILING, CEILING);
                }
                written += 1;
            }
        }
        buf.truncate(written * ch);
    }

    /// Appends the frames still held, with the envelope carrying on as if
    /// silence followed.
    pub(crate) fn drain(&mut self, out: &mut Vec<f32>) {
        while self.delay.len() >= self.channels {
            let gain = self.gain(0.0);
            for _ in 0..self.channels {
                let held = self.delay.pop_front().unwrap_or(0.0);
                out.push((held * gain).clamp(-CEILING, CEILING));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn stereo_sine(amplitude: f32, seconds: f32) -> Vec<f32> {
        let frames = (RATE as f32 * seconds) as usize;
        (0..frames)
            .flat_map(|i| {
                let v = amplitude
                    * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / f64::from(RATE)).sin()
                        as f32;
                [v, v]
            })
            .collect()
    }

    /// Feeds `input` in chunks of odd sizes, then drains.
    fn run(limiter: &mut Limiter, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut at = 0;
        for size in [37, 1000, 1024].iter().cycle() {
            if at >= input.len() {
                break;
            }
            let end = (at + size * 2).min(input.len());
            let mut chunk = input[at..end].to_vec();
            limiter.process(&mut chunk);
            out.extend(chunk);
            at = end;
        }
        limiter.drain(&mut out);
        out
    }

    #[test]
    fn peaks_stay_under_the_ceiling_on_a_boosted_sine() {
        let mut limiter = Limiter::new(RATE, 2);
        let out = run(&mut limiter, &stereo_sine(4.0, 2.0));
        assert!(out.iter().all(|y| y.abs() <= CEILING));
        // It is limited, not muted.
        assert!(out.iter().any(|y| y.abs() > 0.5));
    }

    #[test]
    fn quiet_signals_pass_unchanged() {
        let input = stereo_sine(0.5, 1.0);
        let out = run(&mut Limiter::new(RATE, 2), &input);
        assert_eq!(out.len(), input.len());
        assert!(input.iter().zip(&out).all(|(x, y)| (x - y).abs() < 1e-6));
    }

    #[test]
    fn the_gain_returns_to_unity_after_a_peak() {
        let mut input = stereo_sine(4.0, 0.1);
        input.extend(stereo_sine(0.5, 3.0));
        let out = run(&mut Limiter::new(RATE, 2), &input);
        assert_eq!(out.len(), input.len());
        let last = input.len() - RATE as usize;
        assert!(
            input[last..]
                .iter()
                .zip(&out[last..])
                .all(|(x, y)| (x - y).abs() < 1e-6)
        );
    }

    #[test]
    fn both_channels_get_the_same_gain() {
        // The right channel is the left one at a quarter of the level.
        let input: Vec<f32> = stereo_sine(4.0, 0.5)
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|f| [f[0], f[1] * 0.25])
            .collect();
        let out = run(&mut Limiter::new(RATE, 2), &input);
        assert!(
            out.as_chunks::<2>()
                .0
                .iter()
                .all(|f| (f[0] * 0.25 - f[1]).abs() < 1e-5)
        );
    }

    #[test]
    fn reset_drops_what_was_held() {
        let mut limiter = Limiter::new(RATE, 2);
        let mut chunk = stereo_sine(0.5, 0.1);
        limiter.process(&mut chunk);
        limiter.reset();
        let mut out = Vec::new();
        limiter.drain(&mut out);
        assert!(out.is_empty());
        // It starts over: the first frames are held again.
        let mut chunk = stereo_sine(0.5, 0.001);
        let before = chunk.len();
        limiter.process(&mut chunk);
        assert!(chunk.len() < before);
    }

    #[test]
    fn mono_works_too() {
        let mut limiter = Limiter::new(RATE, 1);
        let mut out: Vec<f32> = stereo_sine(4.0, 0.5).iter().step_by(2).copied().collect();
        let frames = out.len();
        limiter.process(&mut out);
        limiter.drain(&mut out);
        assert_eq!(out.len(), frames);
        assert!(out.iter().all(|y| y.abs() <= CEILING));
    }
}
