//! Installing a verified update, and finding out how this copy is installed.
//! Nothing here exits the process: the app decides when it ends.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::download::verify_file;
use super::{Install, Prepared};

/// The NSIS installer's switches: `/P` passive, `/S` silent, `/R` relaunch
/// the app when done, `/NS` leave the shortcuts alone. Either way it ends a
/// running `cloudrs.exe`.
pub(crate) fn nsis_args(relaunch: bool) -> &'static [&'static str] {
    if relaunch {
        &["/P", "/R", "/NS"]
    } else {
        &["/S", "/NS"]
    }
}

impl Prepared {
    /// Installs the update. With `relaunch` the new version starts afterwards;
    /// without it (a plain quit) nothing starts. The Windows installer and the
    /// macOS swap check the signature again first. Call it as the app quits.
    pub fn apply(&self, relaunch: bool) -> Result<(), String> {
        match &self.install {
            Install::Nsis => {
                verify_file(&self.file, &self.public_key, &self.signature)?;
                Command::new(&self.file)
                    .args(nsis_args(relaunch))
                    .spawn()
                    .map(drop)
                    .map_err(|e| e.to_string())
            }
            // Swapped when the download finished.
            Install::AppImage(path) => {
                if relaunch {
                    Command::new(path).spawn().map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            Install::MacApp(bundle) => {
                verify_file(&self.file, &self.public_key, &self.signature)?;
                swap_bundle(&self.file, bundle)?;
                if relaunch {
                    Command::new("open")
                        .arg("-n")
                        .arg(bundle)
                        .spawn()
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            Install::Manual => Ok(()),
        }
    }
}

/// Replaces `bundle` with the `cloudrs.app` inside the verified archive. The
/// old bundle is kept until the new one is in place, and put back on failure.
fn swap_bundle(archive: &Path, bundle: &Path) -> Result<(), String> {
    let parent = bundle.parent().ok_or("the bundle has no folder")?;
    let staging = parent.join(".cloudrs-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let swapped = unpack_and_replace(archive, bundle, &staging);
    let _ = std::fs::remove_dir_all(&staging);
    swapped
}

fn unpack_and_replace(archive: &Path, bundle: &Path, staging: &Path) -> Result<(), String> {
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(staging)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("tar failed: {status}"));
    }
    let fresh = staging.join("cloudrs.app");
    if !fresh.is_dir() {
        return Err("the archive has no cloudrs.app".into());
    }
    let mut old = bundle.as_os_str().to_owned();
    old.push(".old");
    let old = PathBuf::from(old);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(bundle, &old).map_err(|e| e.to_string())?;
    if let Err(error) = std::fs::rename(&fresh, bundle) {
        let _ = std::fs::rename(&old, bundle);
        return Err(error.to_string());
    }
    if let Err(error) = std::fs::remove_dir_all(&old) {
        tracing::warn!(%error, "could not remove the old app bundle");
    }
    Ok(())
}

/// How this copy was installed, by looking at the files around it. Runs on
/// the update thread, never at startup.
pub(super) fn detect() -> Install {
    let Ok(exe) = std::env::current_exe() else {
        return Install::Manual;
    };
    #[cfg(windows)]
    {
        // The NSIS installer writes its uninstaller beside the executable.
        if exe
            .parent()
            .is_some_and(|dir| dir.join("uninstall.exe").is_file())
        {
            Install::Nsis
        } else {
            Install::Manual
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = exe;
        match std::env::var_os("APPIMAGE").map(PathBuf::from) {
            Some(path) if path.is_file() => Install::AppImage(path),
            _ => Install::Manual,
        }
    }
    #[cfg(target_os = "macos")]
    {
        match app_bundle(&exe) {
            Some(bundle) if bundle.parent().is_some_and(is_writable) => Install::MacApp(bundle),
            _ => Install::Manual,
        }
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = exe;
        Install::Manual
    }
}

/// `X.app` for an executable at `X.app/Contents/MacOS/<name>`.
#[cfg(any(target_os = "macos", test))]
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    let inside =
        exe.parent()?.file_name()? == "MacOS" && exe.parent()?.parent()?.file_name()? == "Contents";
    (inside && bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

/// Whether a file can be created in `dir`, tried for real.
#[cfg(target_os = "macos")]
fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".cloudrs-write-test");
    let created = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if created {
        let _ = std::fs::remove_file(&probe);
    }
    created
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_installer_switches_follow_the_relaunch() {
        assert_eq!(nsis_args(true), ["/P", "/R", "/NS"]);
        assert_eq!(nsis_args(false), ["/S", "/NS"]);
    }

    #[test]
    fn a_bundle_is_found_from_its_executable() {
        assert_eq!(
            app_bundle(Path::new(
                "/Applications/cloudrs.app/Contents/MacOS/cloudrs"
            )),
            Some(PathBuf::from("/Applications/cloudrs.app"))
        );
        assert_eq!(app_bundle(Path::new("/usr/local/bin/cloudrs")), None);
        assert_eq!(
            app_bundle(Path::new("/x/cloudrs/Contents/MacOS/cloudrs")),
            None
        );
    }
}
