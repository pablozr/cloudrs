use std::time::Duration;

/// Errors from talking to SoundCloud.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The `client_id` or the user token was refused, even after refreshing the `client_id`.
    #[error("SoundCloud refused the request (unauthorized)")]
    Unauthorized,
    /// Too many requests. Retry after the given delay, when SoundCloud sent one.
    #[error("rate limited by SoundCloud")]
    RateLimited { retry_after: Option<Duration> },
    /// The resource does not exist or is private.
    #[error("not found")]
    NotFound,
    /// The resource exists but is not available in this country.
    #[error("not available in this region")]
    GeoBlocked,
    /// No `client_id` could be found on soundcloud.com.
    #[error("could not find a client_id on soundcloud.com")]
    ClientIdNotFound,
    /// The track has no stream cloudrs can play (encrypted, preview only or blocked).
    #[error("no playable stream: {0}")]
    NoPlayableStream(&'static str),
    /// Any other unexpected HTTP status.
    #[error("unexpected HTTP status {0}")]
    Status(u16),
    /// The network failed (DNS, TLS, timeout, connection reset).
    #[error("network error: {0}")]
    Network(#[source] reqwest::Error),
    /// The response did not have the expected shape.
    #[error("unexpected response: {0}")]
    Decode(#[source] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error)
    }
}
