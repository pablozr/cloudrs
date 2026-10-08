//! The sound card side: a cpal stream fed from a lock-free ring buffer.
//!
//! The callback never allocates, locks or blocks. Everything it needs to know
//! from the engine travels through atomics in [`Shared`].

use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{Error, Result};

/// What the stream's error callback told the engine. Ordered by severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Fault {
    None = 0,
    /// The stream stopped working, for instance because the default device changed.
    Invalidated = 1,
    /// The device is gone.
    Lost = 2,
}

impl Fault {
    pub(crate) fn from_kind(kind: cpal::ErrorKind) -> Self {
        match kind {
            cpal::ErrorKind::DeviceNotAvailable => Self::Lost,
            cpal::ErrorKind::StreamInvalidated => Self::Invalidated,
            _ => Self::None,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            2 => Self::Lost,
            1 => Self::Invalidated,
            _ => Self::None,
        }
    }
}

/// State shared between the engine thread and the audio callback.
#[derive(Debug)]
pub struct Shared {
    volume: AtomicU32,
    pub paused: AtomicBool,
    /// Set by the engine to drop everything buffered; cleared by the callback.
    pub flush: AtomicBool,
    /// Frames sent to the device since the last flush.
    pub frames_played: AtomicU64,
    /// Worst [`Fault`] reported since the engine last looked.
    fault: AtomicU8,
}

impl Shared {
    fn new() -> Self {
        Self {
            volume: AtomicU32::new(1.0_f32.to_bits()),
            paused: AtomicBool::new(true),
            flush: AtomicBool::new(false),
            frames_played: AtomicU64::new(0),
            fault: AtomicU8::new(Fault::None as u8),
        }
    }

    /// Called from the stream's error callback.
    pub(crate) fn report(&self, fault: Fault) {
        self.fault.fetch_max(fault as u8, Ordering::Relaxed);
    }

    /// The worst fault since the last call, and resets it.
    pub(crate) fn take_fault(&self) -> Fault {
        Fault::from_u8(self.fault.swap(0, Ordering::Relaxed))
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn volume(&self) -> f32 {
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
    /// The device's cpal id, captured at open: a stream on the default device
    /// keeps following it, so asking later would name the new default.
    pub device_id: Option<String>,
}

/// Whether a device with this cpal id is currently plugged in and active.
pub(crate) fn is_present(id: &str) -> bool {
    cpal::DeviceId::from_str(id)
        .ok()
        .is_some_and(|id| cpal::default_host().device_by_id(&id).is_some())
}

/// An output device the person can choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    /// The cpal id, stable across runs (`host:id`).
    pub id: String,
    pub name: String,
}

/// The system's output devices. Slow on some systems (WASAPI opens each
/// device): call it off the UI thread. A host error gives an empty list.
pub fn output_devices() -> Vec<OutputDevice> {
    let devices = match cpal::default_host().output_devices() {
        Ok(devices) => devices,
        Err(error) => {
            tracing::warn!(%error, "could not list output devices");
            return Vec::new();
        }
    };
    // A device can vanish between listing and asking, so each ask may fail.
    // `Device`'s `Display` would panic then; read the description instead.
    devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            let name = device.description().ok()?.name().to_owned();
            Some(OutputDevice { id, name })
        })
        .collect()
}

/// Opens the wanted output device, or the system default when `wanted` is
/// `None`. The flag is true when the wanted device could not be opened and the
/// default plays instead. The ring buffer holds about half a second.
pub fn open(wanted: Option<&str>) -> Result<(Output, bool)> {
    let host = cpal::default_host();
    if let Some(id) = wanted {
        let chosen = cpal::DeviceId::from_str(id)
            .ok()
            .and_then(|id| host.device_by_id(&id));
        match chosen.map(build) {
            Some(Ok(output)) => return Ok((output, false)),
            Some(Err(error)) => tracing::warn!(%error, id, "chosen output device failed"),
            None => tracing::warn!(id, "chosen output device not found"),
        }
    }
    let device = host.default_output_device().ok_or(Error::NoOutputDevice)?;
    Ok((build(device)?, wanted.is_some()))
}

fn build(device: cpal::Device) -> Result<Output> {
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
    let error_shared = Arc::clone(&shared);
    let device_id = device.id().ok().map(|id| id.to_string());

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
            move |error| {
                error_shared.report(Fault::from_kind(error.kind()));
                tracing::warn!(%error, "audio output error");
            },
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
        device_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kinds_map_to_faults() {
        assert_eq!(
            Fault::from_kind(cpal::ErrorKind::DeviceNotAvailable),
            Fault::Lost
        );
        assert_eq!(
            Fault::from_kind(cpal::ErrorKind::StreamInvalidated),
            Fault::Invalidated
        );
        assert_eq!(Fault::from_kind(cpal::ErrorKind::Xrun), Fault::None);
        assert_eq!(
            Fault::from_kind(cpal::ErrorKind::DeviceChanged),
            Fault::None
        );
    }

    #[test]
    fn the_callback_volume_never_exceeds_one() {
        let shared = Shared::new();
        shared.set_volume(1.5);
        assert_eq!(shared.volume(), 1.0);
        shared.set_volume(-1.0);
        assert_eq!(shared.volume(), 0.0);
    }

    #[test]
    fn the_worst_fault_wins_and_is_taken_once() {
        let shared = Shared::new();
        shared.report(Fault::Invalidated);
        shared.report(Fault::Lost);
        shared.report(Fault::Invalidated);
        assert_eq!(shared.take_fault(), Fault::Lost);
        assert_eq!(shared.take_fault(), Fault::None);
    }
}
