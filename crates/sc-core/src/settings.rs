//! The person's settings (ADR 0017): typed, saved in the core's database and
//! read before the window opens, so the first frame already has the right theme.

use std::path::Path;

use crate::store;

/// Which theme the person wants. `System` follows the operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Dark,
    Light,
}

/// The interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
}

impl Language {
    /// Every language the interface has texts for.
    pub const ALL: [Language; 1] = [Language::English];

    /// The stable code saved in the database.
    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
        }
    }

    /// The language for a saved code; an unknown one gives the default.
    pub fn from_tag(tag: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|language| language.tag() == tag)
            .unwrap_or_default()
    }
}

/// Everything the person can change in Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub language: Language,
    /// Show the playing track on Discord (ADR 0015).
    pub discord: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            language: Language::default(),
            discord: true,
        }
    }
}

/// The saved settings, or the defaults when there are none or the database
/// cannot be read. Runs on the calling thread and costs one SQLite open, so
/// the app calls it once before the window opens. A damaged or newer database
/// is left for the core to report and reset.
pub fn read_settings(data_dir: &Path) -> Settings {
    let read = || -> Result<Option<Settings>, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        let conn = store::open(&data_dir.join(store::FILE_NAME)).map_err(|e| e.to_string())?;
        store::load_settings(&conn).map_err(|e| e.to_string())
    };
    match read() {
        Ok(saved) => saved.unwrap_or_default(),
        Err(error) => {
            tracing::warn!(%error, "could not read the settings; using the defaults");
            Settings::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags_round_trip() {
        for language in Language::ALL {
            assert_eq!(Language::from_tag(language.tag()), language);
        }
        assert_eq!(Language::from_tag("xx"), Language::English);
    }

    #[test]
    fn discord_is_on_by_default() {
        assert!(Settings::default().discord);
        assert_eq!(Settings::default().theme, ThemeChoice::System);
    }
}
