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
async fn fetches_related_tracks() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/tracks/42/related"))
        .and(query_param("client_id", OLD_ID))
        .and(query_param("limit", "10"))
        .respond_with(search_fixture())
        .mount(&server)
        .await;

    let page = client(&server, Some(OLD_ID), None)
        .related(42, 10)
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

#[tokio::test]
async fn downloads_waveforms_and_files_without_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wave/abc_m.json"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"width":3,"height":140,"samples":[1,2,3]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/art/abc-t300x300.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0xFF, 0xD8, 0xFF]))
        .mount(&server)
        .await;

    // No client_id configured and no web app mounted: these calls must not need one.
    let sc = client(&server, None, None);
    let wave = sc
        .waveform(&format!("{}/wave/abc_m.json", server.uri()))
        .await
        .unwrap();
    assert_eq!(wave.samples, [1, 2, 3]);
    let bytes = sc
        .download(&format!("{}/art/abc-t300x300.jpg", server.uri()))
        .await
        .unwrap();
    assert_eq!(bytes, [0xFF, 0xD8, 0xFF]);
    let missing = sc.download(&format!("{}/art/none.jpg", server.uri())).await;
    assert!(matches!(missing, Err(Error::NotFound)));
}

fn json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json")
}

#[tokio::test]
async fn searches_people_playlists_and_albums() {
    let server = MockServer::start().await;
    for (endpoint, body) in [
        (
            "users",
            r#"{"collection":[{"id":1,"username":"Ana","followers_count":10,"verified":true}],"next_href":"https://x/next"}"#,
        ),
        (
            "playlists",
            r#"{"collection":[{"id":2,"title":"Set","track_count":3,"is_album":false}]}"#,
        ),
        (
            "albums",
            r#"{"collection":[{"id":3,"title":"LP","set_type":"album","is_album":true}]}"#,
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/search/{endpoint}")))
            .and(query_param("q", "ana"))
            .and(query_param("linked_partitioning", "1"))
            .respond_with(json(body))
            .mount(&server)
            .await;
    }
    let sc = client(&server, Some(OLD_ID), None);

    let users = sc.search_users("ana", 20).await.unwrap();
    assert_eq!(users.collection[0].username, "Ana");
    assert_eq!(users.collection[0].followers_count, Some(10));
    assert!(users.next_href.is_some());
    let playlists = sc.search_playlists("ana", 20).await.unwrap();
    assert_eq!(playlists.collection[0].title, "Set");
    let albums = sc.search_albums("ana", 20).await.unwrap();
    assert_eq!(albums.collection[0].is_album, Some(true));
}

#[tokio::test]
async fn fetches_a_profile_and_its_lists() {
    let server = MockServer::start().await;
    let routes = [
        (
            "users/7",
            r#"{"id":7,"username":"Ana","track_count":4,"city":"Lisbon"}"#,
        ),
        (
            "users/7/tracks",
            r#"{"collection":[{"id":1,"title":"A"}],"next_href":null}"#,
        ),
        (
            "users/7/playlists",
            r#"{"collection":[{"id":2,"title":"P"}]}"#,
        ),
        (
            "users/7/likes",
            r#"{"collection":[{"track":{"id":3,"title":"L"}},{"playlist":{"id":4}}],"next_href":"https://x/n"}"#,
        ),
    ];
    for (route, body) in routes {
        Mock::given(method("GET"))
            .and(path(format!("/api/{route}")))
            .respond_with(json(body))
            .mount(&server)
            .await;
    }
    let sc = client(&server, Some(OLD_ID), None);

    let user = sc.user(7).await.unwrap();
    assert_eq!((user.username.as_str(), user.track_count), ("Ana", Some(4)));
    assert_eq!(user.city.as_deref(), Some("Lisbon"));
    assert_eq!(
        sc.user_tracks(7, 10).await.unwrap().collection[0].title,
        "A"
    );
    assert_eq!(sc.user_playlists(7, 10).await.unwrap().collection[0].id, 2);
    let likes = sc.user_likes(7, 10).await.unwrap();
    assert_eq!(likes.collection.len(), 2);
    assert_eq!(likes.collection[0].track.as_ref().unwrap().title, "L");
    assert!(likes.collection[1].track.is_none());
    assert!(likes.next_href.is_some());
}

#[tokio::test]
async fn fetches_a_playlist_with_partial_tracks() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/playlists/5"))
        .respond_with(json(
            r#"{"id":5,"title":"Mix","is_album":false,"duration":9000,
                "user":{"id":7,"username":"Ana"},
                "tracks":[{"id":1,"title":"Full"},{"id":2,"kind":"track"}]}"#,
        ))
        .mount(&server)
        .await;

    let playlist = client(&server, Some(OLD_ID), None)
        .playlist(5)
        .await
        .unwrap();
    assert_eq!(playlist.title, "Mix");
    assert_eq!(playlist.user.unwrap().username, "Ana");
    assert_eq!(playlist.tracks.len(), 2);
    assert!(playlist.tracks[1].title.is_empty());
}
