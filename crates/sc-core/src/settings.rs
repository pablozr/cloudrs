//! The person's settings (ADR 0017): typed, saved in the core's database and
//! read before the window opens, so the first frame already has the right theme.

use std::path::Path;

use crate::store;

/// Where builds before ADR 0017 kept "Discord off": a file that exists when off.
const LEGACY_DISCORD_OFF: &str = "discord-off";

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
    /// A cpal device id (`host:id`); `None` follows the system default (ADR 0020).
    pub output_device: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            language: Language::default(),
            discord: true,
            output_device: None,
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
        let mut saved = store::load_settings(&conn).map_err(|e| e.to_string())?;
        // The old flag file wins once: move it into the database, then drop it.
        let legacy = data_dir.join(LEGACY_DISCORD_OFF);
        if legacy.exists() {
            let migrated = Settings {
                discord: false,
                ..saved.clone().unwrap_or_default()
            };
            store::save_settings(&conn, &migrated).map_err(|e| e.to_string())?;
            if let Err(error) = std::fs::remove_file(&legacy) {
                tracing::warn!(%error, "could not remove the old Discord flag");
            }
            saved = Some(migrated);
        }
        Ok(saved)
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
    fn the_discord_off_file_migrates_once() {
        let dir = std::env::temp_dir().join(format!("cloudrs-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LEGACY_DISCORD_OFF), b"").unwrap();

        assert!(!read_settings(&dir).discord);
        assert!(!dir.join(LEGACY_DISCORD_OFF).exists());
        assert!(!read_settings(&dir).discord, "kept in the database");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discord_is_on_by_default() {
        assert!(Settings::default().discord);
        assert_eq!(Settings::default().theme, ThemeChoice::System);
    }
}
