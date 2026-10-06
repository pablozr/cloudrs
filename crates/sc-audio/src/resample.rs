//! Converting decoded audio to the output device's rate and channel count.
//!
//! Linear interpolation is enough for the M0 spike; a band-limited resampler
//! (`rubato`) replaces it when the output path is tuned (M5).

/// Streaming linear resampler with channel mapping. Keeps state between
/// chunks so there are no clicks at chunk boundaries.
#[derive(Debug)]
pub struct Resampler {
    in_rate: u32,
    out_rate: u32,
    in_channels: usize,
    out_channels: usize,
    /// Fractional read position into the next input frame.
    phase: f64,
    /// Last input frame of the previous chunk (already channel-mapped).
    previous: Vec<f32>,
}

impl Resampler {
    pub fn new(in_rate: u32, in_channels: usize, out_rate: u32, out_channels: usize) -> Self {
        Self {
            in_rate,
            out_rate,
            in_channels: in_channels.max(1),
            out_channels: out_channels.max(1),
            phase: 0.0,
            previous: Vec::new(),
        }
    }

    pub fn matches(&self, in_rate: u32, in_channels: usize) -> bool {
        self.in_rate == in_rate && self.in_channels == in_channels.max(1)
    }

    /// Appends the converted samples of `input` (interleaved) to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let frames: Vec<f32> = input
            .chunks_exact(self.in_channels)
            .flat_map(|frame| map_channels(frame, self.out_channels))
            .collect();
        if self.in_rate == self.out_rate {
            out.extend_from_slice(&frames);
            return;
        }
        let ch = self.out_channels;
        let step = f64::from(self.in_rate) / f64::from(self.out_rate);
        // Input frames available: the carried-over frame (index -1) plus this chunk.
        let count = frames.len() / ch;
        let frame_at = |i: isize, c: usize| -> f32 {
            if i < 0 {
                self.previous.get(c).copied().unwrap_or(0.0)
            } else {
                frames[i as usize * ch + c]
            }
        };
        // `phase` is measured from the carried-over frame when there is one.
        let offset: isize = if self.previous.is_empty() { 0 } else { -1 };
        let mut pos = self.phase;
        loop {
            let base = pos.floor() as isize + offset;
            if base + 1 >= count as isize {
                break;
            }
            let t = (pos - pos.floor()) as f32;
            for c in 0..ch {
                let a = frame_at(base, c);
                let b = frame_at(base + 1, c);
                out.push(a + (b - a) * t);
            }
            pos += step;
        }
        // Carry the last frame and the remaining phase into the next chunk.
        let consumed = (count as isize - offset - 1) as f64;
        self.phase = (pos - consumed).max(0.0);
        if count > 0 {
            self.previous = frames[(count - 1) * ch..count * ch].to_vec();
        }
    }
}

fn map_channels(frame: &[f32], out_channels: usize) -> impl Iterator<Item = f32> + '_ {
    (0..out_channels).map(move |c| match (frame.len(), out_channels) {
        (1, _) => frame[0],
        // Down-mix to mono.
        (n, 1) => frame.iter().sum::<f32>() / n as f32,
        // Extra output channels (e.g. 5.1 from stereo) stay silent.
        (n, _) => frame.get(c).copied().filter(|_| c < n).unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_when_formats_match() {
        let mut r = Resampler::new(48_000, 2, 48_000, 2);
        let mut out = Vec::new();
        r.process(&[0.1, 0.2, 0.3, 0.4], &mut out);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn upmixes_mono_and_downmixes_stereo() {
        let mut out = Vec::new();
        Resampler::new(8, 1, 8, 2).process(&[0.5], &mut out);
        assert_eq!(out, [0.5, 0.5]);
        out.clear();
        Resampler::new(8, 2, 8, 1).process(&[0.2, 0.4], &mut out);
        assert!((out[0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn keeps_the_rate_ratio_across_chunks() {
        let mut r = Resampler::new(44_100, 1, 48_000, 1);
        let mut out = Vec::new();
        let chunk: Vec<f32> = (0..441).map(|i| (i as f32 / 441.0).sin()).collect();
        for _ in 0..100 {
            r.process(&chunk, &mut out);
        }
        // 1 s of input becomes ~1 s of output (one frame of latency).
        let expected = 48_000.0;
        assert!((out.len() as f64 - expected).abs() < 4.0, "{}", out.len());
    }

    #[test]
    fn is_continuous_across_chunk_boundaries() {
        let mut r = Resampler::new(2, 1, 3, 1);
        let mut out = Vec::new();
        r.process(&[0.0, 1.0], &mut out);
        r.process(&[2.0, 3.0], &mut out);
        // A ramp stays a monotonic ramp.
        assert!(out.windows(2).all(|w| w[1] >= w[0]), "{out:?}");
    }
}
