//! Demuxing and decoding with symphonia into interleaved `f32` samples.

use std::io::{ErrorKind, Read};
use std::time::Duration;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;

use crate::{Error, Result};

/// A demuxer plus decoder for the first audio track of a stream.
pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    sample_rate: u32,
    channels: usize,
    /// Audio still to drop before the first returned sample (after a seek).
    skip: Duration,
}

impl Decoder {
    /// Probes the stream and prepares the decoder. `extension` (`mp4`, `aac`,
    /// `mp3`) speeds up the probe; the container is still detected from the bytes.
    pub fn new(reader: Box<dyn Read + Send + Sync>, extension: Option<&str>) -> Result<Self> {
        let stream = MediaSourceStream::new(
            Box::new(ReadOnlySource::new(reader)),
            MediaSourceStreamOptions::default(),
        );
        let mut hint = Hint::new();
        if let Some(extension) = extension {
            hint.with_extension(extension);
        }
        let format = symphonia::default::get_probe().probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?;
        let track = format
            .first_track_known_codec(TrackType::Audio)
            .ok_or(Error::NoAudioTrack)?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or(Error::NoAudioTrack)?
            .clone();
        let track_id = track.id;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())?;
        Ok(Self {
            sample_rate: params.sample_rate.unwrap_or(44_100),
            channels: params.channels.as_ref().map_or(2, |c| c.count()),
            format,
            decoder,
            track_id,
            skip: Duration::ZERO,
        })
    }

    /// Drops the first `duration` of audio, to land exactly on a seek target.
    pub fn skip(&mut self, duration: Duration) {
        self.skip = duration;
    }

    /// Sample rate of the last decoded chunk, in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count of the last decoded chunk.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Decodes the next packet into `out` (cleared first, interleaved).
    /// Returns `false` at the end of the stream.
    pub fn next_chunk(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        out.clear();
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(false),
                Err(SymphoniaError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buffer) => {
                    if buffer.frames() == 0 {
                        continue;
                    }
                    self.sample_rate = buffer.spec().rate();
                    self.channels = buffer.spec().channels().count();
                    let frames = buffer.frames();
                    let to_skip = (self.skip.as_secs_f64() * f64::from(self.sample_rate)) as usize;
                    if to_skip >= frames {
                        self.skip = self
                            .skip
                            .saturating_sub(frames_to_duration(frames, self.sample_rate));
                        continue;
                    }
                    self.skip = Duration::ZERO;
                    buffer.copy_to_vec_interleaved(out);
                    out.drain(..to_skip * self.channels);
                    return Ok(true);
                }
                // A corrupt packet: skip it, like every player does.
                Err(SymphoniaError::DecodeError(reason)) => {
                    tracing::warn!(reason, "skipped an undecodable packet");
                }
                Err(SymphoniaError::ResetRequired) => self.decoder.reset(),
                Err(e) => return Err(e.into()),
            }
        }
    }
}

fn frames_to_duration(frames: usize, rate: u32) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(rate.max(1)))
}
