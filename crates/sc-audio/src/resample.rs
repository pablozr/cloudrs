//! Converting decoded audio to the output device's rate and channel count.
//!
//! The channels are mapped first, then a band-limited sinc resampler (`rubato`,
//! ADR 0022) changes the rate. Equal rates pass straight through.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Indexing, Resampler as _, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};

/// Input frames handed to the sinc resampler at a time.
const CHUNK: usize = 1024;

/// Streaming resampler with channel mapping. Keeps state between chunks so
/// there are no clicks at chunk boundaries.
pub struct Resampler {
    in_rate: u32,
    in_channels: usize,
    out_channels: usize,
    /// `None` when the rates are equal.
    sinc: Option<Sinc>,
}

struct Sinc {
    resampler: Async<f32>,
    ratio: f64,
    /// Channel-mapped input frames waiting for a whole chunk.
    queue: Vec<f32>,
    /// One chunk's output, reused.
    scratch: Vec<f32>,
    /// Frames of start-up delay still to drop from the output.
    skip: usize,
    /// Input frames received and output frames passed on since the start.
    fed: u64,
    emitted: u64,
}

impl Resampler {
    pub fn new(in_rate: u32, in_channels: usize, out_rate: u32, out_channels: usize) -> Self {
        let out_channels = out_channels.max(1);
        let sinc = (in_rate != out_rate)
            .then(|| Sinc::new(in_rate, out_rate, out_channels))
            .flatten();
        Self {
            in_rate,
            in_channels: in_channels.max(1),
            out_channels,
            sinc,
        }
    }

    pub fn matches(&self, in_rate: u32, in_channels: usize) -> bool {
        self.in_rate == in_rate && self.in_channels == in_channels.max(1)
    }

    /// Appends the converted samples of `input` (interleaved) to `out`. The
    /// output lags the input by up to a chunk and the filter's delay; `flush`
    /// gives the rest.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let frames = input.chunks_exact(self.in_channels);
        let Some(sinc) = &mut self.sinc else {
            out.extend(frames.flat_map(|frame| map_channels(frame, self.out_channels)));
            return;
        };
        for frame in frames {
            sinc.queue.extend(map_channels(frame, self.out_channels));
            sinc.fed += 1;
        }
        while sinc.queue.len() >= CHUNK * self.out_channels {
            sinc.run(self.out_channels, None, out);
        }
    }

    /// Appends what is still inside the resampler, at the end of a stream, and
    /// starts over.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let channels = self.out_channels;
        let Some(sinc) = &mut self.sinc else {
            return;
        };
        let wanted = (sinc.fed as f64 * sinc.ratio).round() as u64;
        let held = sinc.queue.len() / channels;
        // The rest of the input (zero-padded), then a chunk of silence so the
        // filter rings out.
        sinc.queue.resize(CHUNK * channels, 0.0);
        for partial in [held, 0] {
            if sinc.emitted >= wanted {
                break;
            }
            sinc.run(channels, Some(partial), out);
            // The output past the end of the stream is padding.
            let extra = sinc.emitted.saturating_sub(wanted) as usize;
            out.truncate(out.len() - extra * channels);
            sinc.emitted -= extra as u64;
            sinc.queue.fill(0.0);
        }
        sinc.restart();
    }
}

impl Sinc {
    fn new(in_rate: u32, out_rate: u32, channels: usize) -> Option<Self> {
        let ratio = f64::from(out_rate) / f64::from(in_rate);
        let params = SincInterpolationParameters::new(128, WindowFunction::BlackmanHarris2)
            .oversampling_factor(128)
            .interpolation(SincInterpolationType::Linear);
        let resampler =
            match Async::<f32>::new_sinc(ratio, 1.1, &params, CHUNK, channels, FixedAsync::Input) {
                Ok(resampler) => resampler,
                Err(error) => {
                    tracing::warn!(%error, in_rate, out_rate, "could not build the resampler");
                    return None;
                }
            };
        let scratch = vec![0.0; resampler.output_frames_max() * channels];
        let skip = resampler.output_delay();
        Some(Self {
            resampler,
            ratio,
            queue: Vec::with_capacity(4 * CHUNK * channels),
            scratch,
            skip,
            fed: 0,
            emitted: 0,
        })
    }

    /// Resamples the first chunk of the queue (`partial` valid frames, if it is
    /// not full), appends the output after the start-up delay, and drops the
    /// chunk from the queue.
    fn run(&mut self, channels: usize, partial: Option<usize>, out: &mut Vec<f32>) {
        let capacity = self.scratch.len() / channels;
        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            active_channels_mask: None,
            partial_len: partial,
        };
        let result = InterleavedSlice::new(&self.queue[..CHUNK * channels], channels, CHUNK)
            .and_then(|input| {
                InterleavedSlice::new_mut(&mut self.scratch, channels, capacity)
                    .map(|output| (input, output))
            })
            .map_err(|e| e.to_string())
            .and_then(|(input, mut output)| {
                self.resampler
                    .process_into_buffer(&input, &mut output, Some(&indexing))
                    .map_err(|e| e.to_string())
            });
        let (used, produced) = match result {
            Ok(counts) => counts,
            Err(error) => {
                tracing::warn!(%error, "resampling failed; dropping a chunk");
                (CHUNK, 0)
            }
        };
        self.queue.drain(..(used * channels).min(self.queue.len()));
        let dropped = self.skip.min(produced);
        self.skip -= dropped;
        out.extend_from_slice(&self.scratch[dropped * channels..produced * channels]);
        self.emitted += (produced - dropped) as u64;
    }

    fn restart(&mut self) {
        self.resampler.reset();
        self.queue.clear();
        self.skip = self.resampler.output_delay();
        self.fed = 0;
        self.emitted = 0;
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

    fn sine(rate: u32, hz: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| {
                (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin()) as f32
            })
            .collect()
    }

    /// Resamples `input` in chunks of `size` frames, then flushes.
    fn run(r: &mut Resampler, input: &[f32], size: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for chunk in input.chunks(size) {
            r.process(chunk, &mut out);
        }
        r.flush(&mut out);
        out
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32).sqrt()
    }

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
        let chunk = sine(44_100, 440.0, 441);
        for _ in 0..100 {
            r.process(&chunk, &mut out);
        }
        // 1 s of input becomes ~1 s of output, minus what waits for a whole
        // chunk and the filter's delay.
        assert!(
            (out.len() as f64 - 48_000.0).abs() < 1200.0,
            "{}",
            out.len()
        );
    }

    #[test]
    fn flushing_gives_exactly_the_resampled_length() {
        let mut r = Resampler::new(44_100, 2, 48_000, 2);
        let input: Vec<f32> = sine(44_100, 440.0, 44_100)
            .into_iter()
            .flat_map(|v| [v, v])
            .collect();
        let out = run(&mut r, &input, 1000);
        assert_eq!(out.len(), 48_000 * 2);
        // It starts over afterwards.
        let again = run(&mut r, &input, 1000);
        assert_eq!(again.len(), out.len());
    }

    #[test]
    fn a_sine_keeps_its_level() {
        let input = sine(44_100, 1000.0, 44_100);
        let out = run(&mut Resampler::new(44_100, 1, 48_000, 1), &input, 1024);
        let before = rms(&input[4000..40_000]);
        let after = rms(&out[4000..44_000]);
        let db = 20.0 * (after / before).log10();
        assert!(db.abs() < 0.1, "{db} dB");
    }

    #[test]
    fn the_chunk_size_does_not_change_the_result() {
        let input = sine(44_100, 1000.0, 20_000);
        let whole = run(
            &mut Resampler::new(44_100, 1, 48_000, 1),
            &input,
            input.len(),
        );
        for size in [37, 4096] {
            let chunked = run(&mut Resampler::new(44_100, 1, 48_000, 1), &input, size);
            assert_eq!(chunked.len(), whole.len(), "{size}");
            assert!(
                whole
                    .iter()
                    .zip(&chunked)
                    .all(|(a, b)| (a - b).abs() < 1e-5),
                "{size}"
            );
        }
    }

    #[test]
    fn the_start_is_not_shifted_by_the_filter_delay() {
        // A burst at 1000 Hz after 0.5 s stays near 0.5 s in the output.
        let mut input = vec![0.0; 22_050];
        input.extend(sine(44_100, 1000.0, 4410));
        let out = run(&mut Resampler::new(44_100, 1, 48_000, 1), &input, 1024);
        let first = out.iter().position(|v| v.abs() > 0.25).unwrap();
        assert!((first as i64 - 24_000).abs() < 100, "{first}");
    }
}
