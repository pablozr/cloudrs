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
    /// Brazilian Portuguese (ADR 0025).
    PtBr,
}

impl Language {
    /// Every language the interface has texts for.
    pub const ALL: [Language; 2] = [Language::English, Language::PtBr];

    /// The stable code saved in the database.
    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::PtBr => "pt-BR",
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

/// The equalizer presets (ADR 0022). There are no sliders yet; the gains
/// are proposals, to be tuned by ear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EqPreset {
    #[default]
    Off,
    Bass,
    Treble,
    Vocal,
    Electronic,
}

impl EqPreset {
    /// Every preset, in the order Settings lists them.
    pub const ALL: [EqPreset; 5] = [
        Self::Off,
        Self::Bass,
        Self::Treble,
        Self::Vocal,
        Self::Electronic,
    ];

    /// The stable code saved in the database.
    pub fn code(self) -> i64 {
        match self {
            Self::Off => 0,
            Self::Bass => 1,
            Self::Treble => 2,
            Self::Vocal => 3,
            Self::Electronic => 4,
        }
    }

    /// The preset for a saved code; an unknown one gives `Off`.
    pub fn from_code(code: i64) -> Self {
        Self::ALL
            .into_iter()
            .find(|preset| preset.code() == code)
            .unwrap_or_default()
    }

    /// The gain in dB for each band, 31 Hz to 16 kHz; `None` for `Off`.
    pub fn gains(self) -> Option<[f32; sc_audio::EQ_BANDS]> {
        match self {
            Self::Off => None,
            Self::Bass => Some([6., 5., 4., 2., 0., 0., 0., 0., 0., 0.]),
            Self::Treble => Some([0., 0., 0., 0., 0., 0., 1.5, 3., 4.5, 6.]),
            Self::Vocal => Some([-2., -2., -1., 0., 2., 3., 3., 2., 0., -1.]),
            Self::Electronic => Some([5., 4., 1., 0., -2., 0., 1., 2., 4., 5.]),
        }
    }
}

/// The highest volume the player accepts: 200% with the volume boost on,
/// 100% without it.
pub fn max_volume(volume_boost: bool) -> f32 {
    if volume_boost { 2.0 } else { 1.0 }
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
    /// Even out loud and quiet tracks (ADR 0022).
    pub normalize: bool,
    pub equalizer: EqPreset,
    /// Let the volume go up to 200%, behind a limiter (ADR 0022).
    pub volume_boost: bool,
    /// Check GitHub for a new version and install it on restart (ADR 0026).
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            language: Language::default(),
            discord: true,
            output_device: None,
            normalize: true,
            equalizer: EqPreset::Off,
            volume_boost: false,
            auto_update: true,
        }
    }
}

/// The saved settings, or the defaults when there are none or the database
/// cannot be read. See [`read_saved_settings`].
pub fn read_settings(data_dir: &Path) -> Settings {
    read_saved_settings(data_dir).unwrap_or_default()
}

/// The settings saved earlier, or `None` when nothing is saved or the
/// database cannot be read. It tells a first run from a saved choice, so the
/// app can pick a default (the system language) only for the first. Runs on
/// the calling thread and costs one SQLite open, so the app calls it once
/// before the window opens. A damaged or newer database is left for the core
/// to report and reset.
pub fn read_saved_settings(data_dir: &Path) -> Option<Settings> {
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
        Ok(saved) => saved,
        Err(error) => {
            tracing::warn!(%error, "could not read the settings; using the defaults");
            None
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
        assert_eq!(Language::from_tag("pt-BR"), Language::PtBr);
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
    fn an_empty_directory_has_no_saved_settings() {
        let dir = std::env::temp_dir().join(format!("cloudrs-unsaved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read_saved_settings(&dir), None);
        assert_eq!(read_settings(&dir), Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_migrated_discord_flag_counts_as_saved() {
        let dir = std::env::temp_dir().join(format!("cloudrs-saved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LEGACY_DISCORD_OFF), b"").unwrap();
        let saved = read_saved_settings(&dir).expect("migrated settings are saved");
        assert!(!saved.discord);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn equalizer_codes_round_trip() {
        for preset in EqPreset::ALL {
            assert_eq!(EqPreset::from_code(preset.code()), preset);
        }
        assert_eq!(EqPreset::from_code(77), EqPreset::Off);
        assert_eq!(EqPreset::Off.gains(), None);
        assert!(EqPreset::ALL[1..].iter().all(|p| p.gains().is_some()));
    }

    #[test]
    fn the_sound_settings_default_to_a_plain_sound() {
        let settings = Settings::default();
        assert!(settings.normalize);
        assert_eq!(settings.equalizer, EqPreset::Off);
        assert!(!settings.volume_boost);
    }

    #[test]
    fn the_boost_allows_up_to_200_percent() {
        assert_eq!(max_volume(false), 1.0);
        assert_eq!(max_volume(true), 2.0);
    }

    #[test]
    fn discord_is_on_by_default() {
        assert!(Settings::default().discord);
        assert_eq!(Settings::default().theme, ThemeChoice::System);
    }
}
