//! `ScClient` against a local mock of soundcloud.com and api-v2.

use sc_api::models::Resource;
use sc_api::{ClientConfig, Error, ScClient, SoundCloudApi, StreamProtocol};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OLD_ID: &str = "0ldC1ient1dXXXXXXXXXXXXXXXXXXXXX";
const NEW_ID: &str = "NewC1ient1dYYYYYYYYYYYYYYYYYYYYY";

fn client(server: &MockServer, client_id: Option<&str>, token: Option<&str>) -> ScClient {
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    ScClient::new(ClientConfig {
        api_base: base.join("api/").unwrap(),
        web_base: base,
        client_id: client_id.map(str::to_owned),
        oauth_token: token.map(str::to_owned),
        ..ClientConfig::default()
    })
    .unwrap()
}

async fn mount_web_app(server: &MockServer, id: &str) {
    let html = format!(
        r#"<html><script crossorigin src="{0}/assets/0-app.js"></script><script crossorigin src="{0}/assets/49-vendor.js"></script></html>"#,
        server.uri()
    );
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/assets/49-vendor.js"))
        .respond_with(ResponseTemplate::new(200).set_body_string("var a=1;"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/assets/0-app.js"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(format!(r#"x={{client_id:"{id}"}}"#)),
        )
        .mount(server)
        .await;
}

fn search_fixture() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(
        include_str!("fixtures/search_tracks.json"),
        "application/json",
    )
}

#[tokio::test]
async fn extracts_client_id_from_the_web_app() {
    let server = MockServer::start().await;
    mount_web_app(&server, NEW_ID).await;
    Mock::given(method("GET"))
        .and(path("/api/search/tracks"))
        .and(query_param("client_id", NEW_ID))
        .and(query_param("q", "lights out"))
        .respond_with(search_fixture())
        .mount(&server)
        .await;

    let page = client(&server, None, None)
        .search_tracks("lights out", 20)
        .await
        .unwrap();
    assert_eq!(page.collection.len(), 2);
}

#[tokio::test]
async fn refreshes_a_stale_client_id_once() {
    let server = MockServer::start().await;
    mount_web_app(&server, NEW_ID).await;
    Mock::given(method("GET"))
        .and(path("/api/tracks/1"))
        .and(query_param("client_id", OLD_ID))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tracks/1"))
        .and(query_param("client_id", NEW_ID))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"kind":"track","id":1,"title":"One"}"#,
            "application/json",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let sc = client(&server, Some(OLD_ID), None);
    assert_eq!(sc.track(1).await.unwrap().title, "One");
    assert_eq!(sc.client_id().await.unwrap(), NEW_ID);
}

#[tokio::test]
async fn gives_up_after_one_refresh() {
    let server = MockServer::start().await;
    mount_web_app(&server, NEW_ID).await;
    Mock::given(method("GET"))
        .and(path("/api/tracks/1"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let error = client(&server, Some(OLD_ID), None)
        .track(1)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Unauthorized), "{error:?}");
}

#[tokio::test]
async fn reports_rate_limits_with_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/tracks/1"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "7"))
        .mount(&server)
        .await;

    let error = client(&server, Some(OLD_ID), None)
        .track(1)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        Error::RateLimited { retry_after: Some(d) } if d.as_secs() == 7
    ));
}

#[tokio::test]
async fn sends_the_user_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/resolve"))
        .and(header("authorization", "OAuth secret-token"))
        .and(query_param("url", "https://soundcloud.com/someone"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"kind":"user","id":9,"username":"someone"}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let resource = client(&server, Some(OLD_ID), Some("secret-token"))
        .resolve("https://soundcloud.com/someone")
        .await
        .unwrap();
    assert!(matches!(resource, Resource::User(user) if user.username == "someone"));
}

#[tokio::test]
async fn follows_next_href() {
    let server = MockServer::start().await;
    let next = format!("{}/api/search/tracks?offset=2&q=x", server.uri());
    Mock::given(method("GET"))
        .and(path("/api/search/tracks"))
        .and(query_param("offset", "2"))
        .and(query_param("client_id", OLD_ID))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(r#"{"collection":[{"id":3}]}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let sc = client(&server, Some(OLD_ID), None);
    let first = sc_api::models::Page::<sc_api::models::Track> {
        collection: vec![],
        next_href: Some(next),
        total_results: None,
    };
    let second = sc.next_page(&first).await.unwrap().unwrap();
    assert_eq!(second.collection[0].id, 3);
    assert!(sc.next_page(&second).await.unwrap().is_none());
}

#[tokio::test]
async fn resolves_the_best_stream() {
    let server = MockServer::start().await;
    let fixture = include_str!("fixtures/search_tracks.json").replace(
        "https://api-v2.soundcloud.com/",
        &format!("{}/api/", server.uri()),
    );
    let page: sc_api::models::Page<sc_api::models::Track> = serde_json::from_str(&fixture).unwrap();

    Mock::given(method("GET"))
        .and(path(
            "/api/media/soundcloud:tracks:1844203521/aaa/stream/hls",
        ))
        .and(query_param(
            "track_authorization",
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.fixture",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"url":"https://cf-hls-media.sndcdn.com/playlist/abc.m3u8"}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let sc = client(&server, Some(OLD_ID), None);
    let stream = sc.stream_url(&page.collection[0]).await.unwrap();
    assert_eq!(stream.protocol, StreamProtocol::Hls);
    assert_eq!(
        stream.url,
        "https://cf-hls-media.sndcdn.com/playlist/abc.m3u8"
    );

    let preview = sc.stream_url(&page.collection[1]).await.unwrap_err();
    assert!(matches!(preview, Error::NoPlayableStream(_)));
}
