//! The updater against a local server: the real thread, the real download and
//! the real signature check, with a key made only for these tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sc_platform::update::{self, Config, Failure, Install, Progress};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PACKAGE: &str = "cloudrs_9.9.9_test.bin";

fn fixture(name: &str) -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/update");
    std::fs::read(dir.join(name)).unwrap()
}

fn text(name: &str) -> String {
    String::from_utf8(fixture(name)).unwrap().trim().to_owned()
}

fn temp_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "cloudrs-update-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A server with the manifest for this platform and, optionally, the package.
async fn server(package: Option<Vec<u8>>) -> MockServer {
    let server = MockServer::start().await;
    let manifest = serde_json::json!({
        "version": "v9.9.9",
        "platforms": { update::platform_key().unwrap(): {
            "url": format!("{}/{PACKAGE}", server.uri()),
            "signature": text(&format!("{PACKAGE}.sig")),
            "format": "nsis",
        }},
    });
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(manifest))
        .mount(&server)
        .await;
    if let Some(bytes) = package {
        Mock::given(method("GET"))
            .and(path(format!("/{PACKAGE}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&server)
            .await;
    }
    server
}

fn config(server: &MockServer, current: &str, install: Install, dir: &Path) -> Config {
    Config {
        current: current.parse().unwrap(),
        manifest_url: format!("{}/latest.json", server.uri()).parse().unwrap(),
        public_key: text("test.key.pub"),
        user_agent: "cloudrs-test".into(),
        download_dir: dir.to_owned(),
        install: Some(install),
    }
}

/// Everything the check reports, up to and including its last message.
async fn run(config: Config) -> Vec<Progress> {
    let rx = update::check(config);
    let mut seen = Vec::new();
    loop {
        let next = tokio::time::timeout(Duration::from_secs(20), rx.recv_async())
            .await
            .expect("the check reports in time")
            .expect("the check ends with a last message");
        let last = !matches!(next, Progress::Downloading { .. });
        seen.push(next);
        if last {
            return seen;
        }
    }
}

fn files_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

async fn requests_for(server: &MockServer, name: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with(name))
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_newer_version_is_downloaded_and_verified() {
    let server = server(Some(fixture(PACKAGE))).await;
    let dir = temp_dir();
    let seen = run(config(&server, "0.1.0", Install::Nsis, &dir)).await;

    assert!(matches!(seen.last(), Some(Progress::Ready(p)) if p.version == "9.9.9"));
    assert!(seen[..seen.len() - 1].iter().all(
        |p| matches!(p, Progress::Downloading { version, percent } if version == "9.9.9" && *percent <= 100)
    ));
    assert_eq!(files_in(&dir), [PACKAGE]);
    assert_eq!(std::fs::read(dir.join(PACKAGE)).unwrap(), fixture(PACKAGE));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_current_version_is_up_to_date() {
    let server = server(Some(fixture(PACKAGE))).await;
    let dir = temp_dir();
    let seen = run(config(&server, "9.9.9", Install::Nsis, &dir)).await;
    assert_eq!(seen, [Progress::UpToDate]);
    assert_eq!(requests_for(&server, PACKAGE).await, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_newer_install_never_goes_back() {
    let server = server(Some(fixture(PACKAGE))).await;
    let dir = temp_dir();
    let seen = run(config(&server, "10.0.0", Install::Nsis, &dir)).await;
    assert_eq!(seen, [Progress::UpToDate]);
    assert_eq!(requests_for(&server, PACKAGE).await, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn tampered_bytes_fail_and_leave_nothing() {
    let mut bytes = fixture(PACKAGE);
    bytes[0] ^= 0xff;
    let server = server(Some(bytes)).await;
    let dir = temp_dir();
    let seen = run(config(&server, "0.1.0", Install::Nsis, &dir)).await;
    assert_eq!(seen.last(), Some(&Progress::Failed(Failure::Download)));
    assert!(!seen.iter().any(|p| matches!(p, Progress::Ready(_))));
    assert!(files_in(&dir).is_empty(), "left {:?}", files_in(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_manifest_is_a_failed_check() {
    let server = MockServer::start().await;
    let dir = temp_dir();
    let seen = run(config(&server, "0.1.0", Install::Nsis, &dir)).await;
    assert_eq!(seen, [Progress::Failed(Failure::Check)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_install_only_hears_about_it() {
    let server = server(Some(fixture(PACKAGE))).await;
    let dir = temp_dir();
    let seen = run(config(&server, "0.1.0", Install::Manual, &dir)).await;
    assert_eq!(
        seen,
        [Progress::Available {
            version: "9.9.9".into()
        }]
    );
    assert_eq!(requests_for(&server, PACKAGE).await, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn old_downloads_are_cleared_before_a_check() {
    let server = server(None).await;
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("stale.part"), b"x").unwrap();
    let seen = run(config(&server, "9.9.9", Install::Nsis, &dir)).await;
    assert_eq!(seen, [Progress::UpToDate]);
    assert!(files_in(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn an_appimage_is_swapped_in_place_keeping_its_permissions() {
    use std::os::unix::fs::PermissionsExt;

    // The manifest of this test names an AppImage package.
    let server = MockServer::start().await;
    let name = PACKAGE;
    let manifest = serde_json::json!({
        "version": "9.9.9",
        "platforms": { update::platform_key().unwrap(): {
            "url": format!("{}/{name}", server.uri()),
            "signature": text(&format!("{name}.sig")),
            "format": "appimage",
        }},
    });
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(manifest))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture(name)))
        .mount(&server)
        .await;

    let dir = temp_dir();
    let app = dir.join("apps");
    std::fs::create_dir_all(&app).unwrap();
    let image = app.join("cloudrs.AppImage");
    std::fs::write(&image, b"old").unwrap();
    std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o750)).unwrap();

    let downloads = dir.join("downloads");
    let seen = run(config(
        &server,
        "0.1.0",
        Install::AppImage(image.clone()),
        &downloads,
    ))
    .await;
    assert!(matches!(seen.last(), Some(Progress::Ready(_))), "{seen:?}");
    assert_eq!(std::fs::read(&image).unwrap(), fixture(name));
    let mode = std::fs::metadata(&image).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o750);
    assert_eq!(files_in(&app), ["cloudrs.AppImage"]);
    let _ = std::fs::remove_dir_all(&dir);
}
