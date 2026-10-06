//! Embedded fonts (SIL Open Font License 1.1; licenses in `assets/fonts/`).
//!
//! Compiled into the binary and registered with the GPUI text system, so the
//! app looks the same on every OS whatever is installed.

use std::borrow::Cow;

use gpui::{App, Global, SharedString};

const GEIST: &[u8] = include_bytes!("../../../assets/fonts/Geist-Variable.ttf");
const GEIST_MONO: &[u8] = include_bytes!("../../../assets/fonts/GeistMono-Variable.ttf");
const BRICOLAGE: &[u8] = include_bytes!("../../../assets/fonts/BricolageGrotesque-Variable.ttf");

/// Family names as the platform registered them.
///
/// Platforms name variable fonts differently (DirectWrite groups instances by
/// optical size, for example), so each role tries a few candidates.
#[derive(Debug, Clone)]
pub struct FontFamilies {
    pub display: SharedString,
    pub interface: SharedString,
    pub mono: SharedString,
}

impl Global for FontFamilies {}

impl Default for FontFamilies {
    fn default() -> Self {
        Self {
            display: DISPLAY[0].into(),
            interface: INTERFACE[0].into(),
            mono: MONO[0].into(),
        }
    }
}

const DISPLAY: &[&str] = &["Bricolage Grotesque", "Bricolage Grotesque 14pt"];
const INTERFACE: &[&str] = &["Geist", "Geist Variable"];
const MONO: &[&str] = &["Geist Mono", "Geist Mono Variable"];

fn pick(available: &[String], candidates: &[&str]) -> SharedString {
    candidates
        .iter()
        .find(|name| available.iter().any(|family| family == *name))
        .copied()
        .unwrap_or(candidates[0])
        .into()
}

/// Registers the embedded fonts and stores the resolved families as a global.
pub fn register(cx: &mut App) {
    let fonts = vec![
        Cow::Borrowed(GEIST),
        Cow::Borrowed(GEIST_MONO),
        Cow::Borrowed(BRICOLAGE),
    ];
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        tracing::warn!(%error, "failed to register the embedded fonts");
    }
    let available = cx.text_system().all_font_names();
    let families = FontFamilies {
        display: pick(&available, DISPLAY),
        interface: pick(&available, INTERFACE),
        mono: pick(&available, MONO),
    };
    tracing::debug!(?families, "resolved font families");
    cx.set_global(families);
}
