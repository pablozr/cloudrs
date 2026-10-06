//! Choosing which rendition of a track to play.

use crate::models::Transcoding;

/// How the audio engine must fetch a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamProtocol {
    /// An HLS media playlist (`.m3u8`) of short segments.
    Hls,
    /// One plain file served over HTTP.
    Progressive,
}

/// A resolved, short-lived stream URL. It expires after a few minutes, so
/// resolve it right before playing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSource {
    pub url: String,
    pub protocol: StreamProtocol,
    pub mime_type: String,
}

/// The best rendition cloudrs can decode, or `None` when every rendition is
/// encrypted, a preview, or a codec we do not support (Opus).
///
/// Preference: AAC over HLS (160k before 96k), then progressive MP3, then
/// MP3 over HLS.
pub fn pick_transcoding(transcodings: &[Transcoding]) -> Option<&Transcoding> {
    transcodings
        .iter()
        .filter(|t| !t.snipped && !t.url.is_empty())
        .filter_map(|t| score(t).map(|s| (s, t)))
        .max_by_key(|(s, _)| *s)
        .map(|(_, t)| t)
}

pub(crate) fn protocol(transcoding: &Transcoding) -> Option<StreamProtocol> {
    match transcoding.format.protocol.as_str() {
        "hls" => Some(StreamProtocol::Hls),
        "progressive" => Some(StreamProtocol::Progressive),
        // `encrypted-hls`, `ctr-encrypted-hls`, `cbc-encrypted-hls`: DRM, never played.
        _ => None,
    }
}

fn score(transcoding: &Transcoding) -> Option<u32> {
    let protocol = protocol(transcoding)?;
    let mime = transcoding.format.mime_type.as_str();
    let high_bitrate =
        transcoding.preset.contains("160") || transcoding.quality.as_deref() == Some("hq");
    let base = if mime.contains("mp4a") || mime.starts_with("audio/mp4") || mime.contains("aac") {
        40
    } else if mime.starts_with("audio/mpeg") {
        match protocol {
            StreamProtocol::Progressive => 30,
            StreamProtocol::Hls => 20,
        }
    } else {
        // Opus/Ogg and anything unknown.
        return None;
    };
    Some(base + u32::from(high_bitrate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Format;

    fn t(protocol: &str, mime: &str, preset: &str, snipped: bool) -> Transcoding {
        Transcoding {
            url: format!("https://api-v2.soundcloud.com/media/{preset}/{protocol}"),
            preset: preset.into(),
            snipped,
            format: Format {
                protocol: protocol.into(),
                mime_type: mime.into(),
            },
            ..Transcoding::default()
        }
    }

    const AAC: &str = r#"audio/mp4; codecs="mp4a.40.2""#;

    #[test]
    fn prefers_aac_160_over_everything() {
        let list = [
            t("hls", "audio/mpeg", "mp3_1_0", false),
            t("progressive", "audio/mpeg", "mp3_1_0", false),
            t("hls", AAC, "aac_96k", false),
            t("hls", AAC, "aac_160k", false),
            t("hls", r#"audio/ogg; codecs="opus""#, "opus_0_0", false),
        ];
        assert_eq!(pick_transcoding(&list).unwrap().preset, "aac_160k");
    }

    #[test]
    fn falls_back_to_progressive_mp3() {
        let list = [
            t("hls", "audio/mpeg", "mp3_0_0", false),
            t("progressive", "audio/mpeg", "mp3_0_0", false),
        ];
        assert_eq!(
            pick_transcoding(&list).unwrap().format.protocol,
            "progressive"
        );
    }

    #[test]
    fn never_picks_encrypted_previews_or_opus() {
        let list = [
            t("ctr-encrypted-hls", AAC, "aac_160k", false),
            t("hls", AAC, "aac_160k", true),
            t("hls", r#"audio/ogg; codecs="opus""#, "opus_0_0", false),
        ];
        assert!(pick_transcoding(&list).is_none());
    }
}
