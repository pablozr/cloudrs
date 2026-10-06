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

/// Opens a source for decoding. Blocks until the first bytes are available.
pub fn open(source: &Source) -> Result<Opened> {
    match source.kind {
        SourceKind::Progressive => {
            let response = http()?.get(&source.url).send()?.error_for_status()?;
            Ok(Opened {
                extension: extension_of(&source.url),
                reader: Box::new(response),
            })
        }
        SourceKind::Hls => {
            let reader = HlsReader::open(&source.url)?;
            Ok(Opened {
                extension: reader.extension.clone(),
                reader: Box::new(reader),
            })
        }
    }
}

fn extension_of(url: &str) -> Option<String> {
    let path = Url::parse(url).ok()?.path().to_owned();
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "m4s" | "m4a" | "mp4" | "cmfa" => "mp4".to_owned(),
        _ => ext,
    })
}

/// Reads an HLS VOD playlist as one continuous byte stream: the init segment
/// (`#EXT-X-MAP`, for fMP4) followed by every media segment.
pub struct HlsReader {
    segments: flume::Receiver<io::Result<Vec<u8>>>,
    current: Vec<u8>,
    pos: usize,
    /// Media extension of the segments, for the probe.
    pub extension: Option<String>,
}

impl HlsReader {
    pub fn open(url: &str) -> Result<Self> {
        let client = http()?;
        let (base, media) = load_media_playlist(&client, url)?;
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
        let mut urls = Vec::with_capacity(media.segments.len() + 1);
        if let Some(map) = media.segments.first().and_then(|s| s.map.as_ref()) {
            urls.push(resolve(&map.uri)?);
        }
        for segment in &media.segments {
            urls.push(resolve(&segment.uri)?);
        }
        let extension = urls.last().and_then(|u| extension_of(u.as_str()));
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
                    // The reader was dropped: stop fetching.
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
            extension,
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
