//! In-app updates (ADR 0026): look for a newer version on GitHub Releases,
//! download it in the background, check its minisign signature and hand the
//! app a [`Prepared`] update to install when the person restarts or quits.
//!
//! A thread owns the whole check. It reports through a channel and the app
//! decides what to show and when to install; nothing here touches the core or
//! the UI. Without a public key (or in a debug build) the updater is off, as
//! Discord is without an application id.

mod download;
mod install;
mod manifest;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use semver::Version;
use url::Url;

const REPO: &str = "https://github.com/pablozr/cloudrs";

/// Where a build that cannot update itself sends the person.
pub fn releases_page() -> String {
    format!("{REPO}/releases/latest")
}

/// The public half of the update signing key: the base64 line of
/// `update.key.pub`, made by `cargo packager signer generate`. Empty keeps the
/// updater off until the maintainer adds it (ADR 0026, Releasing).
const PUBLIC_KEY: &str = "";

/// How long after start the first check waits, so it never competes with the
/// first frame or the first track.
pub const START_DELAY: Duration = Duration::from_secs(10);
/// How long to wait between two checks.
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the app asks [`due`]; a laptop that slept catches up within it.
pub const TICK: Duration = Duration::from_secs(60 * 60);

/// How a downloaded update gets installed on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// Run the NSIS installer that installed this copy.
    Nsis,
    /// Replace this AppImage file.
    AppImage(PathBuf),
    /// Replace this `.app` bundle.
    MacApp(PathBuf),
    /// Nothing to install here (a `.deb`, a read-only folder): only a notice.
    Manual,
}

/// Everything a check needs; the fields are public so tests can build one.
#[derive(Debug, Clone)]
pub struct Config {
    /// The running version.
    pub current: Version,
    pub manifest_url: Url,
    /// The base64 line of the public key.
    pub public_key: String,
    pub user_agent: String,
    /// Where a downloaded package waits for the restart. Cleared when a new
    /// one is downloaded.
    pub download_dir: PathBuf,
    /// `None` detects it on the update thread; tests set it.
    pub install: Option<Install>,
}

impl Config {
    /// The configuration of this app, or `None` when it must not update: a
    /// debug build, no public key yet, or a version that is not SemVer. No I/O.
    pub fn for_this_app(current: &str, download_dir: PathBuf) -> Option<Self> {
        if cfg!(debug_assertions) || PUBLIC_KEY.is_empty() {
            return None;
        }
        Some(Self {
            current: Version::parse(current).ok()?,
            manifest_url: Url::parse(&format!("{REPO}/releases/latest/download/latest.json"))
                .ok()?,
            public_key: PUBLIC_KEY.to_owned(),
            user_agent: format!("cloudrs/{current} (+{REPO})"),
            download_dir,
            install: None,
        })
    }
}

/// What a check reports, in order; the last one is `UpToDate`, `Available`,
/// `Ready` or `Failed`.
#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    UpToDate,
    /// A newer version exists but cannot be installed here.
    Available {
        version: String,
    },
    Downloading {
        version: String,
        percent: u8,
    },
    /// Downloaded and verified, waiting for [`Prepared::apply`].
    Ready(Prepared),
    /// The check or the download failed; the reason is logged, not shown.
    Failed,
}

/// A downloaded update whose signature passed. Applying it checks again.
#[derive(Debug, PartialEq, Eq)]
pub struct Prepared {
    pub version: String,
    file: PathBuf,
    signature: String,
    public_key: String,
    install: Install,
}

/// Looks for an update on a thread of its own. The receiver gets the reports
/// and ends after the last one, so the app needs no channel crate of its own.
pub fn check(config: Config) -> flume::Receiver<Progress> {
    let (report, reports) = flume::unbounded();
    let on_thread = report.clone();
    let spawned = std::thread::Builder::new()
        .name("cloudrs-update".into())
        .spawn(move || {
            // A panic still ends with a report, or the app would wait on a
            // check that never finishes and refuse every later one.
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&config, &on_thread)));
            let last = match outcome {
                Ok(Ok(progress)) => progress,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "the update check failed");
                    Progress::Failed
                }
                Err(_) => {
                    tracing::error!("the update check panicked");
                    Progress::Failed
                }
            };
            let _ = on_thread.send(last);
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "the update thread could not start");
        let _ = report.send(Progress::Failed);
    }
    reports
}

fn run(config: &Config, report: &flume::Sender<Progress>) -> Result<Progress, String> {
    let client = download::client(config)?;
    let manifest = download::fetch_manifest(&client, config)?;
    let Some(version) = manifest::newer(&manifest, &config.current) else {
        return Ok(Progress::UpToDate);
    };
    let install = config.install.clone().unwrap_or_else(install::detect);
    let entry = manifest
        .platforms
        .get(&platform_key())
        .filter(|entry| manifest::fits(&entry.format, &install));
    let Some(entry) = entry else {
        return Ok(Progress::Available {
            version: version.to_string(),
        });
    };
    download::fetch(&client, config, &version, entry, &install, report).map(Progress::Ready)
}

/// Whether a check is due: never checked, the clock went back, or at least
/// [`INTERVAL`] has passed.
pub fn due(last_check: Option<SystemTime>, now: SystemTime) -> bool {
    last_check.is_none_or(|last| now.duration_since(last).map_or(true, |age| age >= INTERVAL))
}

/// This machine's key in the manifest's `platforms`: `windows-x86_64`,
/// `linux-x86_64`, `macos-aarch64` or `macos-x86_64`.
pub fn platform_key() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn a_check_is_due_when_never_made_or_a_day_old() {
        let now = at(1_000_000);
        assert!(due(None, now));
        assert!(!due(Some(now - Duration::from_secs(3600)), now));
        assert!(due(Some(now - Duration::from_secs(25 * 3600)), now));
    }

    #[test]
    fn a_clock_that_went_back_makes_a_check_due() {
        let now = at(1_000_000);
        assert!(due(Some(now + Duration::from_secs(60)), now));
    }

    #[test]
    fn the_platform_key_names_the_os_and_the_architecture() {
        let key = platform_key();
        assert!(key.starts_with(std::env::consts::OS), "{key}");
        assert!(key.ends_with(std::env::consts::ARCH), "{key}");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_debug_build_does_not_update() {
        assert!(Config::for_this_app("0.1.0", PathBuf::new()).is_none());
    }
}
