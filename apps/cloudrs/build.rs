//! Embeds the app icon in the Windows executable.
//!
//! The icon is resource id 1: Explorer shows the lowest id on the `.exe`, and
//! GPUI's Windows backend loads id 1 for the window class (title bar and
//! taskbar). Other targets skip everything, so Linux and macOS builds are
//! unaffected.

use std::path::Path;

const ICON: &str = "../../assets/brand/app-icon.ico";

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if let Err(message) = embed_icon() {
        // The app still works without the icon, so do not fail the build.
        println!("cargo:warning=app icon not embedded: {message}");
    }
}

fn embed_icon() -> Result<(), String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?;
    let out_dir = std::env::var("OUT_DIR").map_err(|e| e.to_string())?;
    let icon = Path::new(&manifest_dir)
        .join(ICON)
        .canonicalize()
        .map_err(|e| format!("{ICON}: {e}"))?;

    // Generated with an absolute path: resource compilers disagree on what a
    // relative path in a `.rc` is relative to. Forward slashes need no escaping.
    let icon = icon.to_string_lossy().replace('\\', "/");
    let icon = icon.strip_prefix("//?/").unwrap_or(&icon);
    let rc = Path::new(&out_dir).join("app.rc");
    std::fs::write(&rc, format!("1 ICON \"{icon}\"\n")).map_err(|e| e.to_string())?;

    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .map_err(|e| e.to_string())
}
