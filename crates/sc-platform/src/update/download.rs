//! Fetching the manifest and the package, and the signature check. A package
//! is never reported ready unless the minisign signature passed.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use semver::Version;
use url::Url;

use super::manifest::{self, Entry, Manifest};
use super::{Config, Install, Prepared, Progress, install};

/// The most a manifest may weigh.
const MANIFEST_LIMIT: u64 = 64 * 1024;
/// The most a package may weigh.
const PACKAGE_LIMIT: u64 = 300 * 1024 * 1024;
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(30);
const PACKAGE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const BUFFER: usize = 64 * 1024;

/// The one client of a check. It only follows redirects to allowed URLs:
/// GitHub sends downloads to its own https host, and nothing may drop to plain
/// http. Each request sets its own timeout.
pub(super) fn client(config: &Config) -> Result<Client, String> {
    Client::builder()
        .user_agent(&config.user_agent)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() < 5 && manifest::allowed_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirect to a URL that is not allowed")
            }
        }))
        .build()
        .map_err(|e| e.to_string())
}

fn get(
    client: &Client,
    url: &Url,
    timeout: Duration,
) -> Result<reqwest::blocking::Response, String> {
    if !manifest::allowed_url(url) {
        return Err(format!("{url} is not an allowed address"));
    }
    client
        .get(url.clone())
        .timeout(timeout)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| e.to_string())
}

/// Reads and parses the manifest, with a size cap.
pub(super) fn fetch_manifest(client: &Client, config: &Config) -> Result<Manifest, String> {
    let response = get(client, &config.manifest_url, MANIFEST_TIMEOUT)?;
    let mut body = Vec::new();
    response
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() as u64 > MANIFEST_LIMIT {
        return Err("the manifest is too large".into());
    }
    manifest::parse(&body)
}

/// The key and the signature, from their base64 text.
fn decode(public_key: &str, signature: &str) -> Result<(PublicKey, Signature), String> {
    let text = |encoded: &str| -> Result<String, String> {
        let bytes = STANDARD.decode(encoded.trim()).map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    };
    let key = PublicKey::decode(&text(public_key)?).map_err(|e| e.to_string())?;
    let signature = Signature::decode(&text(signature)?).map_err(|e| e.to_string())?;
    Ok((key, signature))
}

/// Checks a file on disk against its signature. Used again right before an
/// install, since the file waited in a folder the person can write to.
pub(super) fn verify_file(file: &Path, public_key: &str, signature: &str) -> Result<(), String> {
    let (key, signature) = decode(public_key, signature)?;
    let mut verifier = key.verify_stream(&signature).map_err(|e| e.to_string())?;
    let mut reader = File::open(file).map_err(|e| e.to_string())?;
    let mut buffer = vec![0; BUFFER];
    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        verifier.update(&buffer[..read]);
    }
    verifier.finalize().map_err(|e| e.to_string())
}

/// Gets the package ready to install, verifying it while it streams to disk.
/// A package kept from an earlier check that still verifies is reused. The
/// rest of the download folder is cleared before a new download.
pub(super) fn fetch(
    client: &Client,
    config: &Config,
    version: &Version,
    entry: &Entry,
    install: &Install,
    report: &flume::Sender<Progress>,
) -> Result<Prepared, String> {
    let url = Url::parse(&entry.url).map_err(|e| e.to_string())?;
    let name = manifest::file_name(&url).ok_or("the package has no usable file name")?;
    let (key, signature) = decode(&config.public_key, &entry.signature)?;
    // Before spending the bandwidth: is this signature for this file?
    if !manifest::signature_matches(signature.trusted_comment(), &name, version) {
        return Err("the signature is for another file or version".into());
    }
    let prepared = |file| Prepared {
        version: version.to_string(),
        file,
        signature: entry.signature.clone(),
        public_key: config.public_key.clone(),
        install: install.clone(),
    };

    let kept = config.download_dir.join(&name);
    if kept.is_file() && verify_file(&kept, &config.public_key, &entry.signature).is_ok() {
        return Ok(prepared(kept));
    }
    // Whatever an earlier run left (a partial file, an old package) goes first.
    match fs::remove_dir_all(&config.download_dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(error.to_string());
        }
        _ => {}
    }
    fs::create_dir_all(&config.download_dir).map_err(|e| e.to_string())?;

    let (part, target) = install::staging(install, &config.download_dir, &name);
    let file = File::create(&part).map_err(|e| e.to_string())?;
    let streamed = stream(client, &url, version, &key, &signature, file, report);
    if let Err(error) = streamed.and_then(|()| install::finish(install, &part, &target)) {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    Ok(prepared(target))
}

/// Writes the body to `file`, hashing it, and fails unless the signature holds.
fn stream(
    client: &Client,
    url: &Url,
    version: &Version,
    key: &PublicKey,
    signature: &Signature,
    mut file: File,
    report: &flume::Sender<Progress>,
) -> Result<(), String> {
    let mut response = get(client, url, PACKAGE_TIMEOUT)?;
    let total = response.content_length();
    if total.is_some_and(|total| total > PACKAGE_LIMIT) {
        return Err("the package is too large".into());
    }
    let mut verifier = key.verify_stream(signature).map_err(|e| e.to_string())?;
    let mut buffer = vec![0; BUFFER];
    let (mut done, mut sent) = (0u64, None);
    loop {
        // Unknown length shows no percentage. Steps of 5 keep the window from
        // redrawing a hundred times.
        let percent = total.map(|total| (done * 100 / total.max(1)).min(100) as u8 / 5 * 5);
        if let Some(percent) = percent
            && Some(percent) != sent
        {
            sent = Some(percent);
            let _ = report.send(Progress::Downloading {
                version: version.to_string(),
                percent,
            });
        }
        let read = response.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        done += read as u64;
        if done > PACKAGE_LIMIT {
            return Err("the package is too large".into());
        }
        verifier.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    verifier
        .finalize()
        .map_err(|e| format!("verification failed: {e}"))
}
