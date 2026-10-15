//! User settings, stored as TOML in the profile directory. Every field has
//! a default, so a file from an older or newer version still loads.

use std::fmt;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Loaded in the first window at startup when no URL is given.
    pub homepage: String,
    /// Loaded in every new tab and new window.
    pub new_tab_url: String,
    /// Reserved for session restore; stored but not acted on in Phase 1.
    pub restore_windows_on_launch: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            homepage: "https://servo.org/".to_string(),
            new_tab_url: "about:blank".to_string(),
            restore_windows_on_launch: false,
        }
    }
}

#[derive(Debug)]
pub enum SettingsError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::Io(e) => write!(f, "settings file: {e}"),
            SettingsError::Parse(e) => write!(f, "settings file is not valid TOML: {e}"),
            SettingsError::Serialize(e) => write!(f, "could not serialize settings: {e}"),
        }
    }
}

impl std::error::Error for SettingsError {}

impl From<io::Error> for SettingsError {
    fn from(e: io::Error) -> Self {
        SettingsError::Io(e)
    }
}

impl Settings {
    pub fn parse(text: &str) -> Result<Settings, SettingsError> {
        toml::from_str(text).map_err(SettingsError::Parse)
    }

    pub fn to_toml(&self) -> Result<String, SettingsError> {
        toml::to_string_pretty(self).map_err(SettingsError::Serialize)
    }

    /// Load from `path`. A missing file yields the defaults; a malformed
    /// file is an error so a typo never silently resets the user's settings.
    pub fn load(path: &Path) -> Result<Settings, SettingsError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Settings::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(SettingsError::Io(e)),
        }
    }

    /// Write to `path`, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml()?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let s = Settings {
            homepage: "https://a.example/".to_string(),
            new_tab_url: "https://b.example/".to_string(),
            restore_windows_on_launch: true,
        };
        let text = s.to_toml().unwrap();
        assert_eq!(Settings::parse(&text).unwrap(), s);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s = Settings::parse("homepage = \"https://a.example/\"\n").unwrap();
        assert_eq!(s.homepage, "https://a.example/");
        assert_eq!(s.new_tab_url, Settings::default().new_tab_url);
        assert_eq!(
            s.restore_windows_on_launch,
            Settings::default().restore_windows_on_launch
        );
    }

    #[test]
    fn empty_file_is_all_defaults() {
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
    }

    #[test]
    fn malformed_toml_is_an_error() {
        assert!(matches!(
            Settings::parse("homepage = "),
            Err(SettingsError::Parse(_))
        ));
    }

    #[test]
    fn load_of_missing_file_is_default_and_save_creates_parents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.toml");
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());

        let s = Settings {
            homepage: "https://saved.example/".to_string(),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
    }
}
