//! Embedded images: the Lucide icons (ISC, see `assets/icons/`) and the logo.
//!
//! Compiled into the binary like the fonts. Register [`Assets`] on the
//! `Application` (`with_assets`) so `svg().path(..)` and `img(..)` find them.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

macro_rules! embedded {
    ($($path:literal),+ $(,)?) => {
        &[$(($path, include_bytes!(concat!("../../../assets/", $path)))),+]
    };
}

const FILES: &[(&str, &[u8])] = embedded![
    "brand/logo.svg",
    "icons/chevron-left.svg",
    "icons/chevron-right.svg",
    "icons/circle-alert.svg",
    "icons/circle-user.svg",
    "icons/copy.svg",
    "icons/heart-filled.svg",
    "icons/heart.svg",
    "icons/history.svg",
    "icons/house.svg",
    "icons/library.svg",
    "icons/list-music.svg",
    "icons/list-plus.svg",
    "icons/list-start.svg",
    "icons/log-in.svg",
    "icons/log-out.svg",
    "icons/minus.svg",
    "icons/radio.svg",
    "icons/moon.svg",
    "icons/repeat-1.svg",
    "icons/repeat.svg",
    "icons/rss.svg",
    "icons/search.svg",
    "icons/shuffle.svg",
    "icons/skip-back.svg",
    "icons/skip-forward.svg",
    "icons/square.svg",
    "icons/sun.svg",
    "icons/users.svg",
    "icons/volume-2.svg",
    "icons/volume-x.svg",
    "icons/x.svg",
];

/// Serves the embedded files by their path under `assets/`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::new_static(name))
            .collect())
    }
}

/// Path of the full-colour logo, for `img(..)`.
pub const LOGO: &str = "brand/logo.svg";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_is_served_and_unknown_paths_are_not() {
        for (name, bytes) in FILES {
            let loaded = Assets.load(name).unwrap().unwrap();
            assert_eq!(loaded.as_ref(), *bytes);
            assert!(
                bytes.starts_with(b"<") || bytes.starts_with(b"\xEF"),
                "{name} is SVG"
            );
        }
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
    }
}
