//! The sound card side: a cpal stream fed from a lock-free ring buffer.
//!
//! The callback never allocates, locks or blocks. Everything it needs to know
//! from the engine travels through atomics in [`Shared`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{Error, Result};

/// State shared between the engine thread and the audio callback.
#[derive(Debug)]
pub struct Shared {
    volume: AtomicU32,
    pub paused: AtomicBool,
    /// Set by the engine to drop everything buffered; cleared by the callback.
    pub flush: AtomicBool,
    /// Frames sent to the device since the last flush.
    pub frames_played: AtomicU64,
}

impl Shared {
    fn new() -> Self {
        Self {
            volume: AtomicU32::new(1.0_f32.to_bits()),
            paused: AtomicBool::new(true),
            flush: AtomicBool::new(false),
            frames_played: AtomicU64::new(0),
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
}

/// An open output stream. Not `Send` on every platform: it lives and dies on
/// the engine thread.
pub struct Output {
    _stream: cpal::Stream,
    pub sample_rate: u32,
    pub channels: usize,
    pub producer: rtrb::Producer<f32>,
    pub capacity: usize,
    pub shared: Arc<Shared>,
}

/// Opens the default output device. The ring buffer holds about half a second.
pub fn open() -> Result<Output> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(Error::NoOutputDevice)?;
    let config = device
        .default_output_config()
        .map_err(|e| Error::Output(e.to_string()))?
        .config();
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate;
    let capacity = (sample_rate as usize / 2) * channels;
    let (producer, mut consumer) = rtrb::RingBuffer::<f32>::new(capacity);
    let shared = Arc::new(Shared::new());
    let callback_shared = Arc::clone(&shared);

    let stream = device
        .build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _| {
                let shared = &callback_shared;
                if shared.flush.load(Ordering::Acquire) {
                    while consumer.pop().is_ok() {}
                    shared.frames_played.store(0, Ordering::Relaxed);
                    shared.flush.store(false, Ordering::Release);
                }
                if shared.paused.load(Ordering::Relaxed) {
                    data.fill(0.0);
                    return;
                }
                let volume = shared.volume();
                let available = consumer.slots().min(data.len());
                // Whole frames only, so channels never swap.
                let available = available - available % channels;
                if let Ok(chunk) = consumer.read_chunk(available) {
                    let (first, second) = chunk.as_slices();
                    let (head, tail) = data.split_at_mut(first.len());
                    for (out, sample) in head.iter_mut().zip(first) {
                        *out = sample * volume;
                    }
                    for (out, sample) in tail.iter_mut().zip(second) {
                        *out = sample * volume;
                    }
                    chunk.commit_all();
                }
                data[available..].fill(0.0);
                shared
                    .frames_played
                    .fetch_add((available / channels) as u64, Ordering::Relaxed);
            },
            |error| tracing::warn!(%error, "audio output error"),
            None,
        )
        .map_err(|e| Error::Output(e.to_string()))?;
    stream.play().map_err(|e| Error::Output(e.to_string()))?;

    Ok(Output {
        _stream: stream,
        sample_rate,
        channels,
        producer,
        capacity,
        shared,
    })
}
