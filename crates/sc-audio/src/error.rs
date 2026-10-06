/// Errors from the audio engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("invalid HLS playlist: {0}")]
    Playlist(String),
    #[error("encrypted streams are not supported")]
    Encrypted,
    #[error("could not decode the stream: {0}")]
    Decode(#[from] symphonia::core::errors::Error),
    #[error("the stream has no audio track")]
    NoAudioTrack,
    #[error("no audio output device")]
    NoOutputDevice,
    #[error("audio output error: {0}")]
    Output(String),
    #[error("the audio engine stopped")]
    EngineStopped,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
