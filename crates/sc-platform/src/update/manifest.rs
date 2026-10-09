//! The release manifest (`latest.json`) and the rules that decide whether
//! and what to download. Pure: no I/O.
//!
//! The format matches the one `cargo-packager-updater` reads, so the file
//! stays useful if the updater ever changes. `notes` and `pub_date` are in
//! the file for people and are ignored here.

use std::collections::HashMap;
use std::net::IpAddr;

use semver::Version;
use serde::Deserialize;
use url::{Host, Url};

use super::Install;

#[derive(Debug, Deserialize)]
pub(crate) struct Manifest {
    pub version: String,
    pub platforms: HashMap<String, Entry>,
}

/// One platform's package.
#[derive(Debug, Deserialize)]
pub(crate) struct Entry {
    pub url: String,
    /// The `.sig` file's text, which is itself base64.
    pub signature: String,
    /// `nsis`, `appimage` or `app`.
    pub format: String,
}

/// Reads a manifest; the version must be SemVer.
pub(crate) fn parse(bytes: &[u8]) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    parse_version(&manifest.version)?;
    Ok(manifest)
}

fn parse_version(text: &str) -> Result<Version, String> {
    let text = text.trim();
    Version::parse(text.strip_prefix('v').unwrap_or(text)).map_err(|e| e.to_string())
}

/// The manifest's version when it is strictly newer than `current`; an equal
/// or older one (a rollback) gives `None`.
pub(crate) fn newer(manifest: &Manifest, current: &Version) -> Option<Version> {
    parse_version(&manifest.version)
        .ok()
        .filter(|version| version > current)
}

/// `https`, or `http` only to this machine (the tests' local server).
pub(crate) fn allowed_url(url: &Url) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => match url.host() {
            Some(Host::Domain(name)) => name == "localhost",
            Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
            Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
            None => false,
        },
        _ => false,
    }
}

/// The file name in a download URL, if it is plain enough to be a file name
/// in our own folder.
pub(crate) fn file_name(url: &Url) -> Option<String> {
    let name = url.path_segments()?.next_back()?;
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    plain.then(|| name.to_owned())
}

/// Whether a signature really belongs to this download: its trusted comment
/// (`timestamp:N<TAB>file:<name>`, written by cargo-packager) names the file
/// being downloaded, and that file is for this version. A genuine signature
/// of an older build cannot be replayed as the new one.
pub(crate) fn signature_matches(trusted_comment: &str, file_name: &str, version: &Version) -> bool {
    let signed = trusted_comment
        .split('\t')
        .find_map(|part| part.strip_prefix("file:"));
    signed == Some(file_name) && file_name.contains(&format!("_{version}_"))
}

/// Whether the manifest's package format is the one this install uses.
pub(crate) fn fits(format: &str, install: &Install) -> bool {
    matches!(
        (format, install),
        ("nsis", Install::Nsis) | ("appimage", Install::AppImage(_)) | ("app", Install::MacApp(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str) -> Manifest {
        parse(format!(r#"{{"version":"{version}","platforms":{{}}}}"#).as_bytes()).unwrap()
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn the_version_may_start_with_v() {
        assert_eq!(
            newer(&manifest("v0.1.0-beta.2"), &version("0.1.0-beta.1")),
            Some(version("0.1.0-beta.2"))
        );
        assert_eq!(
            newer(&manifest("0.1.0-beta.2"), &version("0.1.0-beta.1")),
            Some(version("0.1.0-beta.2"))
        );
    }

    #[test]
    fn newer_follows_semver_order() {
        for (latest, current) in [
            ("0.1.0-beta.2", "0.1.0-beta.1"),
            ("0.1.0", "0.1.0-beta.9"),
            ("0.1.0-beta.10", "0.1.0-beta.9"),
        ] {
            assert!(newer(&manifest(latest), &version(current)).is_some());
        }
    }

    #[test]
    fn an_equal_or_lower_version_is_not_newer() {
        assert_eq!(newer(&manifest("0.1.0"), &version("0.1.0")), None);
        assert_eq!(newer(&manifest("0.1.0-beta.1"), &version("0.1.0")), None);
        assert_eq!(newer(&manifest("0.0.9"), &version("0.1.0-beta.1")), None);
    }

    #[test]
    fn broken_manifests_are_errors() {
        assert!(parse(b"not json").is_err());
        assert!(parse(br#"{"version":"1.0.0"}"#).is_err());
        assert!(parse(br#"{"version":"soon","platforms":{}}"#).is_err());
    }

    #[test]
    fn a_platform_entry_is_read() {
        let json = br#"{"version":"1.0.0","notes":"x","pub_date":"2026-10-09T12:00:00Z",
            "platforms":{"windows-x86_64":{"url":"https://h/a.exe","signature":"c2ln","format":"nsis"}}}"#;
        let manifest = parse(json).unwrap();
        let entry = &manifest.platforms["windows-x86_64"];
        assert_eq!(
            (entry.url.as_str(), entry.format.as_str()),
            ("https://h/a.exe", "nsis")
        );
    }

    #[test]
    fn only_https_or_local_http_is_allowed() {
        let allowed = |u: &str| allowed_url(&Url::parse(u).unwrap());
        assert!(allowed("https://github.com/x"));
        assert!(!allowed("http://example.com/x"));
        assert!(!allowed("ftp://example.com/x"));
        assert!(allowed("http://127.0.0.1:8080/x"));
        assert!(allowed("http://[::1]:8080/x"));
        assert!(allowed("http://localhost/x"));
    }

    #[test]
    fn file_names_are_plain() {
        let name = |u: &str| file_name(&Url::parse(u).unwrap());
        assert_eq!(
            name("https://h/d/cloudrs_1.0.0_x64-setup.exe").as_deref(),
            Some("cloudrs_1.0.0_x64-setup.exe")
        );
        assert_eq!(name("https://h/d/"), None);
        assert_eq!(name("https://h/d/..%5Cx.exe"), None);
        assert_eq!(name("https://h/d/.hidden"), None);
    }

    #[test]
    fn a_signature_must_name_this_file_and_version() {
        let comment = "timestamp:1791510624\tfile:cloudrs_1.2.3_x64-setup.exe";
        let v = version("1.2.3");
        assert!(signature_matches(
            comment,
            "cloudrs_1.2.3_x64-setup.exe",
            &v
        ));
        assert!(!signature_matches(comment, "cloudrs_1.2.3_other.exe", &v));
        assert!(!signature_matches(
            comment,
            "cloudrs_1.2.3_x64-setup.exe",
            &version("1.2.4")
        ));
        let old = "timestamp:1\tfile:cloudrs_1.2.2_x64-setup.exe";
        assert!(!signature_matches(old, "cloudrs_1.2.2_x64-setup.exe", &v));
        assert!(!signature_matches("timestamp:1", "cloudrs_1.2.3_x.exe", &v));
    }

    #[test]
    fn the_format_must_fit_the_install() {
        assert!(fits("nsis", &Install::Nsis));
        assert!(fits("appimage", &Install::AppImage("a".into())));
        assert!(fits("app", &Install::MacApp("a".into())));
        assert!(!fits("nsis", &Install::Manual));
        assert!(!fits("app", &Install::Nsis));
    }
}
