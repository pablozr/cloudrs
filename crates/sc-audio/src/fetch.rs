//! Turning a [`Source`] into a byte stream.
//!
//! HLS segments are fetched on a background thread into a bounded channel, so
//! the decoder always has about 30 seconds ready and never waits on the
//! network between segments.

use std::io::{self, Read};
use std::thread;
use std::time::Duration;

use m3u8_rs::Playlist;
use url::Url;

use crate::{Error, Result, Source, SourceKind};

/// Segments kept ready ahead of the decoder (SoundCloud segments are ~5 s).
const READ_AHEAD_SEGMENTS: usize = 6;
const TIMEOUT: Duration = Duration::from_secs(20);

/// An opened source: the bytes plus a hint about the container.
pub struct Opened {
    pub reader: Box<dyn Read + Send + Sync>,
    /// File extension of the media, used as a probe hint (`mp4`, `aac`, `mp3`).
    pub extension: Option<String>,
}

fn http() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("cloudrs/", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .build()?)
}

fn extension_of(url: &str) -> Option<String> {
    let path = Url::parse(url).ok()?.path().to_owned();
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "m4s" | "m4a" | "mp4" | "cmfa" => "mp4".to_owned(),
        _ => ext,
    })
}

/// A source opened once (the HLS playlist is loaded here), then read from
/// the start or from any position.
pub struct Stream {
    client: reqwest::blocking::Client,
    kind: StreamKind,
}

enum StreamKind {
    Progressive(String),
    Hls(HlsPlaylist),
}

impl Stream {
    pub fn open(source: &Source) -> Result<Self> {
        let client = http()?;
        let kind = match source.kind {
            SourceKind::Progressive => StreamKind::Progressive(source.url.clone()),
            SourceKind::Hls => StreamKind::Hls(HlsPlaylist::load(&client, &source.url)?),
        };
        Ok(Self { client, kind })
    }

    /// Bytes starting as close to `at` as the format allows, plus how much
    /// audio the decoder must still drop to land exactly on `at`.
    pub fn read_from(&self, at: Duration) -> Result<(Opened, Duration)> {
        match &self.kind {
            // A progressive file restarts from the beginning.
            StreamKind::Progressive(url) => {
                let response = self.client.get(url).send()?.error_for_status()?;
                let opened = Opened {
                    extension: extension_of(url),
                    reader: Box::new(response),
                };
                Ok((opened, at))
            }
            StreamKind::Hls(playlist) => {
                let (index, start) = playlist.segment_at(at);
                let reader = HlsReader::start(self.client.clone(), playlist, index)?;
                let opened = Opened {
                    extension: playlist.extension.clone(),
                    reader: Box::new(reader),
                };
                Ok((opened, at.saturating_sub(start)))
            }
        }
    }
}

/// The parts of a VOD media playlist the player needs.
#[derive(Debug, Clone)]
pub struct HlsPlaylist {
    /// `#EXT-X-MAP` init segment (fMP4), sent before any media segment.
    init: Option<Url>,
    segments: Vec<(Url, Duration)>,
    extension: Option<String>,
}

impl HlsPlaylist {
    fn load(client: &reqwest::blocking::Client, url: &str) -> Result<Self> {
        let (base, media) = load_media_playlist(client, url)?;
        if media.segments.iter().any(|s| {
            s.key
                .as_ref()
                .is_some_and(|k| k.method != m3u8_rs::KeyMethod::None)
        }) {
            return Err(Error::Encrypted);
        }
        let resolve = |uri: &str| {
            base.join(uri)
                .map_err(|e| Error::Playlist(format!("bad segment URI {uri}: {e}")))
        };
        let init = match media.segments.first().and_then(|s| s.map.as_ref()) {
            Some(map) => Some(resolve(&map.uri)?),
            None => None,
        };
        let segments = media
            .segments
            .iter()
            .map(|s| {
                Ok((
                    resolve(&s.uri)?,
                    Duration::from_secs_f32(s.duration.max(0.0)),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let extension = segments.last().and_then(|(u, _)| extension_of(u.as_str()));
        Ok(Self {
            init,
            segments,
            extension,
        })
    }

    /// Index of the segment that contains `at`, and the time it starts at.
    fn segment_at(&self, at: Duration) -> (usize, Duration) {
        let mut start = Duration::ZERO;
        for (index, (_, length)) in self.segments.iter().enumerate() {
            if at < start + *length || index + 1 == self.segments.len() {
                return (index, start);
            }
            start += *length;
        }
        (0, Duration::ZERO)
    }
}

/// Reads HLS segments as one continuous byte stream (init segment first),
/// fetched ahead on a background thread.
pub struct HlsReader {
    segments: flume::Receiver<io::Result<Vec<u8>>>,
    current: Vec<u8>,
    pos: usize,
}

impl HlsReader {
    fn start(
        client: reqwest::blocking::Client,
        playlist: &HlsPlaylist,
        first: usize,
    ) -> Result<Self> {
        let urls: Vec<Url> = playlist
            .init
            .iter()
            .cloned()
            .chain(
                playlist.segments[first..]
                    .iter()
                    .map(|(url, _)| url.clone()),
            )
            .collect();
        let (tx, rx) = flume::bounded(READ_AHEAD_SEGMENTS);
        thread::Builder::new()
            .name("cloudrs-hls-fetch".into())
            .spawn(move || {
                for url in urls {
                    let bytes = client
                        .get(url)
                        .send()
                        .and_then(|r| r.error_for_status())
                        .and_then(|r| r.bytes())
                        .map(|b| b.to_vec())
                        .map_err(io::Error::other);
                    let failed = bytes.is_err();
                    // The reader was dropped (stop or seek): stop fetching.
                    if tx.send(bytes).is_err() || failed {
                        return;
                    }
                }
            })
            .map_err(|e| Error::Playlist(e.to_string()))?;
        Ok(Self {
            segments: rx,
            current: Vec::new(),
            pos: 0,
        })
    }
}

fn load_media_playlist(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<(Url, m3u8_rs::MediaPlaylist)> {
    let mut url = Url::parse(url).map_err(|e| Error::Playlist(e.to_string()))?;
    // A master playlist points to media playlists; follow the best one (once).
    for _ in 0..2 {
        let body = client
            .get(url.clone())
            .send()?
            .error_for_status()?
            .bytes()?;
        match m3u8_rs::parse_playlist_res(&body) {
            Ok(Playlist::MediaPlaylist(media)) => return Ok((url, media)),
            Ok(Playlist::MasterPlaylist(master)) => {
                let best = master
                    .variants
                    .iter()
                    .filter(|v| !v.is_i_frame)
                    .max_by_key(|v| v.bandwidth)
                    .ok_or_else(|| Error::Playlist("master playlist without variants".into()))?;
                url = url
                    .join(&best.uri)
                    .map_err(|e| Error::Playlist(e.to_string()))?;
            }
            Err(e) => return Err(Error::Playlist(e.to_string())),
        }
    }
    Err(Error::Playlist("too many nested playlists".into()))
}

impl Read for HlsReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.current.len() {
            match self.segments.recv() {
                Ok(segment) => {
                    self.current = segment?;
                    self.pos = 0;
                }
                // All segments delivered.
                Err(flume::RecvError::Disconnected) => return Ok(0),
            }
        }
        let n = buf.len().min(self.current.len() - self.pos);
        buf[..n].copy_from_slice(&self.current[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(lengths: &[f32]) -> HlsPlaylist {
        HlsPlaylist {
            init: None,
            segments: lengths
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let url = Url::parse(&format!("https://x/seg{i}.m4s")).unwrap();
                    (url, Duration::from_secs_f32(*l))
                })
                .collect(),
            extension: Some("mp4".into()),
        }
    }

    #[test]
    fn finds_the_segment_for_a_position() {
        let p = playlist(&[5.0, 5.0, 2.0]);
        let secs = Duration::from_secs_f32;
        assert_eq!(p.segment_at(secs(0.0)), (0, secs(0.0)));
        assert_eq!(p.segment_at(secs(4.9)), (0, secs(0.0)));
        assert_eq!(p.segment_at(secs(5.0)), (1, secs(5.0)));
        assert_eq!(p.segment_at(secs(11.0)), (2, secs(10.0)));
        // Past the end: the last segment.
        assert_eq!(p.segment_at(secs(99.0)), (2, secs(10.0)));
    }

    #[test]
    fn maps_segment_extensions_to_containers() {
        assert_eq!(
            extension_of("https://x/a/seg1.m4s?sig=1").as_deref(),
            Some("mp4")
        );
        assert_eq!(extension_of("https://x/a/seg1.aac").as_deref(), Some("aac"));
        assert_eq!(
            extension_of("https://x/a/stream.mp3?x=y").as_deref(),
            Some("mp3")
        );
        assert_eq!(extension_of("https://x/a/noext"), None);
    }
}
