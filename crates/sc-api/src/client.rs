//! The `api-v2` HTTP client.

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::sync::Semaphore;
use url::Url;

use crate::SoundCloudApi;
use crate::client_id::{find_client_id, script_urls};
use crate::error::{Error, Result};
use crate::models::{
    LibraryItem, Like, Page, Playlist, PlaylistEdit, Resource, Selection, StreamItem,
    SystemPlaylist, Track, User, Waveform,
};
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
    /// The signed-in user's token; replaced on sign-in and sign-out.
    oauth_token: Arc<RwLock<Option<String>>>,
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
            oauth_token: Arc::new(RwLock::new(config.oauth_token.clone())),
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

    fn token(&self) -> Option<String> {
        self.oauth_token.read().expect("token lock").clone()
    }

    /// GET `url` (relative to the API base, or absolute) and decode the JSON.
    async fn get_json<T: DeserializeOwned>(&self, url: &str, query: &[(&str, &str)]) -> Result<T> {
        let body = self
            .send(Method::GET, url, query, None)
            .await?
            .bytes()
            .await?;
        serde_json::from_slice(&body).map_err(Error::Decode)
    }

    /// Sends `body` as JSON and decodes the JSON answer.
    async fn send_json<T: DeserializeOwned>(
        &self,
        method: Method,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let answer = self
            .send(method, url, &[], Some(body))
            .await?
            .bytes()
            .await?;
        serde_json::from_slice(&answer).map_err(Error::Decode)
    }

    /// Sends a request to the API and returns the successful response.
    /// Retries once with a fresh `client_id` when SoundCloud refuses the old one.
    async fn send(
        &self,
        method: Method,
        url: &str,
        query: &[(&str, &str)],
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::Response> {
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
                    .request(method.clone(), url.clone())
                    .query(query)
                    .query(&[("client_id", client_id.as_str())]);
                if let Some(body) = body {
                    request = request.json(body);
                }
                if let Some(token) = self.token() {
                    request =
                        request.header(reqwest::header::AUTHORIZATION, format!("OAuth {token}"));
                }
                request.send().await?
            };
            match response.status() {
                status if status.is_success() => return Ok(response),
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
    /// First page of a collection, with `limit` capped at 200.
    async fn paged<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, &str)],
        limit: u32,
    ) -> Result<Page<T>> {
        let limit = limit.clamp(1, 200).to_string();
        let mut query = query.to_vec();
        query.push(("limit", &limit));
        query.push(("linked_partitioning", "1"));
        self.get_json(url, &query).await
    }

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

    async fn search_users(&self, query: &str, limit: u32) -> Result<Page<User>> {
        self.paged("search/users", &[("q", query)], limit).await
    }

    async fn search_playlists(&self, query: &str, limit: u32) -> Result<Page<Playlist>> {
        self.paged("search/playlists", &[("q", query)], limit).await
    }

    async fn search_albums(&self, query: &str, limit: u32) -> Result<Page<Playlist>> {
        self.paged("search/albums", &[("q", query)], limit).await
    }

    async fn user(&self, id: u64) -> Result<User> {
        self.get_json(&format!("users/{id}"), &[]).await
    }

    async fn user_tracks(&self, id: u64, limit: u32) -> Result<Page<Track>> {
        self.paged(&format!("users/{id}/tracks"), &[], limit).await
    }

    async fn user_playlists(&self, id: u64, limit: u32) -> Result<Page<Playlist>> {
        self.paged(&format!("users/{id}/playlists"), &[], limit)
            .await
    }

    async fn user_likes(&self, id: u64, limit: u32) -> Result<Page<Like>> {
        self.paged(&format!("users/{id}/likes"), &[], limit).await
    }

    async fn playlist(&self, id: u64) -> Result<Playlist> {
        self.get_json(&format!("playlists/{id}"), &[]).await
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

    fn set_oauth_token(&self, token: Option<String>) {
        *self.oauth_token.write().expect("token lock") = token;
    }

    async fn me(&self) -> Result<User> {
        self.get_json("me", &[]).await
    }

    async fn feed(&self, limit: u32) -> Result<Page<StreamItem>> {
        self.paged("stream", &[], limit).await
    }

    async fn library(&self, limit: u32) -> Result<Page<LibraryItem>> {
        self.paged("me/library/all", &[], limit).await
    }

    async fn followings(&self, user: u64, limit: u32) -> Result<Page<User>> {
        self.paged(&format!("users/{user}/followings"), &[], limit)
            .await
    }

    async fn liked_track_ids(&self) -> Result<Vec<u64>> {
        self.all_ids("me/track_likes/ids").await
    }

    async fn followed_user_ids(&self) -> Result<Vec<u64>> {
        self.all_ids("me/followings/ids").await
    }

    async fn set_track_like(&self, me: u64, track: u64, liked: bool) -> Result<()> {
        let method = if liked { Method::PUT } else { Method::DELETE };
        self.send(
            method,
            &format!("users/{me}/track_likes/{track}"),
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    async fn mixed_selections(&self) -> Result<Page<Selection>> {
        self.paged("mixed-selections", &[], 10).await
    }

    async fn chart_selections(&self) -> Result<Page<Selection>> {
        self.get_json("charts/selections", &[]).await
    }

    async fn system_playlist(&self, urn: &str) -> Result<SystemPlaylist> {
        self.get_json(&format!("system-playlists/{urn}"), &[]).await
    }

    async fn create_playlist(&self, title: &str, public: bool, tracks: &[u64]) -> Result<Playlist> {
        let body = serde_json::json!({ "playlist": {
            "title": title,
            "sharing": crate::models::sharing(public),
            "tracks": tracks,
        }});
        self.send_json(Method::POST, "playlists", &body).await
    }

    async fn edit_playlist(&self, id: u64, edit: &PlaylistEdit) -> Result<Playlist> {
        let body = serde_json::json!({ "playlist": edit });
        self.send_json(Method::PUT, &format!("playlists/{id}"), &body)
            .await
    }

    async fn delete_playlist(&self, id: u64) -> Result<()> {
        self.send(Method::DELETE, &format!("playlists/{id}"), &[], None)
            .await?;
        Ok(())
    }

    async fn set_following(&self, user: u64, following: bool) -> Result<()> {
        let method = if following {
            Method::POST
        } else {
            Method::DELETE
        };
        self.send(method, &format!("me/followings/{user}"), &[], None)
            .await?;
        Ok(())
    }
}

impl ScClient {
    /// Every id of an `/ids` collection, following its pages.
    async fn all_ids(&self, url: &str) -> Result<Vec<u64>> {
        let mut page: Page<u64> = self.paged(url, &[], 200).await?;
        let mut ids = std::mem::take(&mut page.collection);
        while let Some(mut next) = self.next_page(&page).await? {
            ids.append(&mut next.collection);
            page = next;
        }
        Ok(ids)
    }
}
