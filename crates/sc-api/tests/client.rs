//! `ScClient` against a local mock of soundcloud.com and api-v2.

use sc_api::models::{PlaylistEdit, Resource, SelectionItem};
use sc_api::{ClientConfig, Error, ScClient, SoundCloudApi, StreamProtocol};
use url::Url;
use wiremock::matchers::{body_json, header, method, path, query_param};
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

#[tokio::test]
async fn swaps_the_token_at_runtime() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/me"))
        .and(header("authorization", "OAuth new-token"))
        .respond_with(json(r#"{"id":9,"username":"Me"}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/me"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    mount_web_app(&server, OLD_ID).await;
    let sc = client(&server, Some(OLD_ID), None);

    assert!(matches!(sc.me().await, Err(Error::Unauthorized)));
    sc.set_oauth_token(Some("new-token".into()));
    assert_eq!(sc.me().await.unwrap().username, "Me");
    sc.set_oauth_token(None);
    assert!(matches!(sc.me().await, Err(Error::Unauthorized)));
}

#[tokio::test]
async fn fetches_the_account_lists() {
    let server = MockServer::start().await;
    let routes = [
        (
            "stream",
            r#"{"collection":[{"type":"track","track":{"id":1}},{"type":"playlist-repost","playlist":{"id":2}}]}"#,
        ),
        (
            "me/library/all",
            r#"{"collection":[{"type":"playlist-like","playlist":{"id":3}}]}"#,
        ),
        (
            "users/9/followings",
            r#"{"collection":[{"id":4,"username":"Bo"}]}"#,
        ),
    ];
    for (route, body) in routes {
        Mock::given(method("GET"))
            .and(path(format!("/api/{route}")))
            .and(header("authorization", "OAuth t"))
            .respond_with(json(body))
            .mount(&server)
            .await;
    }
    let sc = client(&server, Some(OLD_ID), Some("t"));

    let feed = sc.feed(20).await.unwrap();
    assert_eq!(feed.collection[0].track.as_ref().unwrap().id, 1);
    assert_eq!(feed.collection[1].kind, "playlist-repost");
    let library = sc.library(20).await.unwrap();
    assert_eq!(library.collection[0].playlist.as_ref().unwrap().id, 3);
    assert_eq!(
        sc.followings(9, 20).await.unwrap().collection[0].username,
        "Bo"
    );
}

#[tokio::test]
async fn collects_every_page_of_ids() {
    let server = MockServer::start().await;
    let next = format!("{}/api/me/track_likes/ids?cursor=2", server.uri());
    Mock::given(method("GET"))
        .and(path("/api/me/track_likes/ids"))
        .and(query_param("cursor", "2"))
        .respond_with(json(r#"{"collection":[3],"next_href":null}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/me/track_likes/ids"))
        .respond_with(json(&format!(
            r#"{{"collection":[1,2],"next_href":"{next}"}}"#
        )))
        .mount(&server)
        .await;

    let ids = client(&server, Some(OLD_ID), Some("t"))
        .liked_track_ids()
        .await
        .unwrap();
    assert_eq!(ids, [1, 2, 3]);
}

#[tokio::test]
async fn likes_and_follows() {
    let server = MockServer::start().await;
    for (verb, route) in [
        ("PUT", "users/9/track_likes/5"),
        ("DELETE", "users/9/track_likes/5"),
        ("POST", "me/followings/4"),
        ("DELETE", "me/followings/4"),
    ] {
        Mock::given(method(verb))
            .and(path(format!("/api/{route}")))
            .and(header("authorization", "OAuth t"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
    }
    let sc = client(&server, Some(OLD_ID), Some("t"));

    sc.set_track_like(9, 5, true).await.unwrap();
    sc.set_track_like(9, 5, false).await.unwrap();
    sc.set_following(4, true).await.unwrap();
    sc.set_following(4, false).await.unwrap();
}

#[tokio::test]
async fn reads_soundcloud_home_rows() {
    let server = MockServer::start().await;
    for (route, body) in [
        (
            "mixed-selections",
            include_str!("fixtures/mixed_selections.json"),
        ),
        (
            "charts/selections",
            include_str!("fixtures/chart_selections.json"),
        ),
        (
            "system-playlists/soundcloud:system-playlists:trending-by-genre:trap",
            include_str!("fixtures/system_playlist.json"),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/{route}")))
            .respond_with(json(body))
            .mount(&server)
            .await;
    }
    let sc = client(&server, Some(OLD_ID), None);

    let home = sc.mixed_selections().await.unwrap();
    assert_eq!(home.collection[0].title, "Artists to watch out for");
    let first = &home.collection[0].items.collection[0];
    assert!(matches!(first, SelectionItem::Playlist(p) if p.title == "Buzzing Mexico"));

    let genre: SelectionItem = serde_json::from_str(
        r#"{"kind":"system-playlist","urn":"soundcloud:system-playlists:trending-by-genre:house","title":"House"}"#,
    )
    .unwrap();
    assert!(matches!(genre, SelectionItem::SystemPlaylist(p) if p.title == "House"));
    let other: SelectionItem = serde_json::from_str(r#"{"kind":"user","id":1}"#).unwrap();
    assert!(matches!(other, SelectionItem::Other));

    let charts = sc.chart_selections().await.unwrap();
    assert_eq!(charts.collection[0].title, "Music Charts US");

    let trap = sc
        .system_playlist("soundcloud:system-playlists:trending-by-genre:trap")
        .await
        .unwrap();
    assert_eq!(trap.short_title.as_deref(), Some("Trap"));
    assert!(trap.tracks.iter().all(|t| t.id != 0 && t.title.is_empty()));
    assert!(trap.calculated_artwork_url.is_some());
}

#[tokio::test]
async fn creates_edits_and_deletes_playlists() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/playlists"))
        .and(header("authorization", "OAuth t"))
        .and(body_json(serde_json::json!({
            "playlist": {
                "title": "Night", "description": "late", "sharing": "private",
                "genre": "House", "tag_list": "deep \"after hours\"", "tracks": [5, 6]
            }
        })))
        .respond_with(json(r#"{"id":40,"title":"Night","sharing":"private"}"#))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/playlists/40"))
        .and(body_json(serde_json::json!({
            "playlist": { "title": "Late night", "tracks": [6, 5, 7] }
        })))
        .respond_with(json(r#"{"id":40,"title":"Late night"}"#))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/playlists/40"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let sc = client(&server, Some(OLD_ID), Some("t"));

    let new = PlaylistEdit {
        title: Some("Night".into()),
        description: Some("late".into()),
        sharing: Some("private".into()),
        genre: Some("House".into()),
        tag_list: Some("deep \"after hours\"".into()),
        tracks: Some(vec![5, 6]),
    };
    let created = sc.create_playlist(&new).await.unwrap();
    assert_eq!(created.id, 40);
    assert_eq!(created.sharing.as_deref(), Some("private"));
    let edit = PlaylistEdit {
        title: Some("Late night".into()),
        tracks: Some(vec![6, 5, 7]),
        ..PlaylistEdit::default()
    };
    assert_eq!(
        sc.edit_playlist(40, &edit).await.unwrap().title,
        "Late night"
    );
    sc.delete_playlist(40).await.unwrap();
}

#[tokio::test]
async fn uploads_a_playlist_cover() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/playlists/soundcloud:playlists:40/artwork"))
        .and(body_json(serde_json::json!({ "image_data": "/9j/" })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let sc = client(&server, Some(OLD_ID), Some("t"));
    // The first bytes of a JPEG.
    sc.set_playlist_artwork(40, &[0xff, 0xd8, 0xff])
        .await
        .unwrap();
}
