//! The `api-v2` HTTP client.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::sync::Semaphore;
use url::Url;

use crate::SoundCloudApi;
use crate::client_id::{find_client_id, script_urls};
use crate::error::{Error, Result};
use crate::models::{Page, Resource, Track, Waveform};
use crate::stream::{StreamSource, pick_transcoding, protocol};

/// Where to reach SoundCloud and which credentials to start with.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// `https://api-v2.soundcloud.com/`
    pub api_base: Url,
    /// `https://soundcloud.com/`, scraped for the `client_id`.
    pub web_base: Url,
    /// Skip the extraction and use this id (debugging, or a cached id).
    pub client_id: Option<String>,
    /// The signed-in user's token, sent as `Authorization: OAuth <token>`.
    pub oauth_token: Option<String>,
    /// Requests in flight at the same time.
    pub max_concurrency: usize,
    pub timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            api_base: Url::parse("https://api-v2.soundcloud.com/").expect("valid url"),
            web_base: Url::parse("https://soundcloud.com/").expect("valid url"),
            client_id: None,
            oauth_token: None,
            max_concurrency: 4,
            timeout: Duration::from_secs(15),
        }
    }
}

/// `api-v2` client. Cheap to clone; clones share the `client_id` and the
/// concurrency limit.
#[derive(Clone)]
pub struct ScClient {
    http: reqwest::Client,
    config: Arc<ClientConfig>,
    client_id: Arc<Mutex<Option<String>>>,
    /// Serializes `client_id` refreshes so a burst of 401s scrapes once.
    refreshing: Arc<tokio::sync::Mutex<()>>,
    permits: Arc<Semaphore>,
}

const USER_AGENT: &str = concat!("cloudrs/", env!("CARGO_PKG_VERSION"));

impl ScClient {
    pub fn new(config: ClientConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(config.timeout)
            .build()?;
        Ok(Self {
            http,
            client_id: Arc::new(Mutex::new(config.client_id.clone())),
            refreshing: Arc::new(tokio::sync::Mutex::new(())),
            permits: Arc::new(Semaphore::new(config.max_concurrency.max(1))),
            config: Arc::new(config),
        })
    }

    /// The `client_id` in use, extracting one first if needed.
    pub async fn client_id(&self) -> Result<String> {
        if let Some(id) = self.cached_client_id() {
            return Ok(id);
        }
        self.refresh_client_id(None).await
    }

    fn cached_client_id(&self) -> Option<String> {
        self.client_id.lock().expect("client_id lock").clone()
    }

    /// Scrapes a fresh `client_id`, unless another task already replaced `stale`.
    async fn refresh_client_id(&self, stale: Option<&str>) -> Result<String> {
        let _guard = self.refreshing.lock().await;
        if let Some(current) = self.cached_client_id()
            && Some(current.as_str()) != stale
        {
            return Ok(current);
        }
        let id = self.scrape_client_id().await?;
        tracing::debug!("extracted a new SoundCloud client_id");
        *self.client_id.lock().expect("client_id lock") = Some(id.clone());
        Ok(id)
    }

    async fn scrape_client_id(&self) -> Result<String> {
        let html = self
            .http
            .get(self.config.web_base.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        // The id usually sits in one of the last bundles.
        for script in script_urls(&html).iter().rev() {
            let Ok(url) = self.config.web_base.join(script) else {
                continue;
            };
            let response = self.http.get(url).send().await?;
            if !response.status().is_success() {
                continue;
            }
            if let Some(id) = find_client_id(&response.text().await?) {
                return Ok(id);
            }
        }
        Err(Error::ClientIdNotFound)
    }

    /// GET `url` (relative to the API base, or absolute) and decode the JSON.
    /// Retries once with a fresh `client_id` when SoundCloud refuses the old one.
    async fn get_json<T: DeserializeOwned>(&self, url: &str, query: &[(&str, &str)]) -> Result<T> {
        let url = self
            .config
            .api_base
            .join(url)
            .map_err(|_| Error::Status(400))?;
        let mut client_id = self.client_id().await?;
        let mut refreshed = false;
        loop {
            let response = {
                let _permit = self.permits.acquire().await.expect("semaphore open");
                let mut request = self
                    .http
                    .get(url.clone())
                    .query(query)
                    .query(&[("client_id", client_id.as_str())]);
                if let Some(token) = &self.config.oauth_token {
                    request =
                        request.header(reqwest::header::AUTHORIZATION, format!("OAuth {token}"));
                }
                request.send().await?
            };
            match response.status() {
                status if status.is_success() => {
                    let body = response.bytes().await?;
                    return serde_json::from_slice(&body).map_err(Error::Decode);
                }
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN if !refreshed => {
                    client_id = self.refresh_client_id(Some(&client_id)).await?;
                    refreshed = true;
                }
                StatusCode::UNAUTHORIZED => return Err(Error::Unauthorized),
                StatusCode::FORBIDDEN => return Err(Error::GeoBlocked),
                StatusCode::NOT_FOUND => return Err(Error::NotFound),
                StatusCode::TOO_MANY_REQUESTS => {
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse().ok())
                        .map(Duration::from_secs);
                    return Err(Error::RateLimited { retry_after });
                }
                status => return Err(Error::Status(status.as_u16())),
            }
        }
    }
}

impl ScClient {
    /// GET an absolute URL outside the API (CDN files): no `client_id`, no token.
    async fn get_cdn(&self, url: &str) -> Result<reqwest::Response> {
        let _permit = self.permits.acquire().await.expect("semaphore open");
        let response = self.http.get(url).send().await?;
        match response.status() {
            status if status.is_success() => Ok(response),
            StatusCode::NOT_FOUND => Err(Error::NotFound),
            status => Err(Error::Status(status.as_u16())),
        }
    }
}

#[derive(Deserialize)]
struct StreamUrl {
    url: String,
}

impl SoundCloudApi for ScClient {
    async fn search_tracks(&self, query: &str, limit: u32) -> Result<Page<Track>> {
        let limit = limit.clamp(1, 200).to_string();
        self.get_json(
            "search/tracks",
            &[
                ("q", query),
                ("limit", &limit),
                ("linked_partitioning", "1"),
            ],
        )
        .await
    }

    async fn related(&self, id: u64, limit: u32) -> Result<Page<Track>> {
        let limit = limit.clamp(1, 200).to_string();
        self.get_json(
            &format!("tracks/{id}/related"),
            &[("limit", &limit), ("linked_partitioning", "1")],
        )
        .await
    }

    async fn next_page<T>(&self, page: &Page<T>) -> Result<Option<Page<T>>>
    where
        T: DeserializeOwned + Send + Sync,
    {
        match &page.next_href {
            Some(next) => self.get_json(next, &[]).await.map(Some),
            None => Ok(None),
        }
    }

    async fn resolve(&self, url: &str) -> Result<Resource> {
        self.get_json("resolve", &[("url", url)]).await
    }

    async fn track(&self, id: u64) -> Result<Track> {
        self.get_json(&format!("tracks/{id}"), &[]).await
    }

    async fn tracks(&self, ids: &[u64]) -> Result<Vec<Track>> {
        let mut all = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(50) {
            let ids = chunk
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let mut batch: Vec<Track> = self.get_json("tracks", &[("ids", &ids)]).await?;
            all.append(&mut batch);
        }
        Ok(all)
    }

    async fn stream_url(&self, track: &Track) -> Result<StreamSource> {
        if track.policy.as_deref() == Some("BLOCK") {
            return Err(Error::NoPlayableStream("blocked in this region"));
        }
        let Some(transcoding) = pick_transcoding(&track.media.transcodings) else {
            return Err(Error::NoPlayableStream(if track.is_preview_only() {
                "only a preview is available"
            } else {
                "no supported format"
            }));
        };
        let protocol = protocol(transcoding).expect("picked transcodings have a known protocol");
        let mut query = Vec::new();
        if let Some(auth) = &track.track_authorization {
            query.push(("track_authorization", auth.as_str()));
        }
        let resolved: StreamUrl = self.get_json(&transcoding.url, &query).await?;
        Ok(StreamSource {
            url: resolved.url,
            protocol,
            mime_type: transcoding.format.mime_type.clone(),
        })
    }

    async fn waveform(&self, url: &str) -> Result<Waveform> {
        let body = self.get_cdn(url).await?.bytes().await?;
        serde_json::from_slice(&body).map_err(Error::Decode)
    }

    async fn download(&self, url: &str) -> Result<Vec<u8>> {
        Ok(self.get_cdn(url).await?.bytes().await?.to_vec())
    }
}
