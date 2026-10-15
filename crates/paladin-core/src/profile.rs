//! Per-user profile directories. All of a profile's state lives under one
//! directory so Phase 2 can run one process per profile.
//!
//! | OS      | Profile directory                                          |
//! |---------|------------------------------------------------------------|
//! | macOS   | `~/Library/Application Support/Paladin/profiles/<name>/`   |
//! | Windows | `%APPDATA%\Paladin\profiles\<name>\`                       |

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

pub const APP_DIR_NAME: &str = "Paladin";
pub const PROFILES_DIR_NAME: &str = "profiles";
pub const DEFAULT_PROFILE_NAME: &str = "default";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub dir: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// Names are one path component: letters, digits, `-`, `_`, `.`, and
    /// not empty, `.`, or `..`.
    InvalidName(String),
    /// The OS reported no home or config directory for this user.
    NoConfigDirectory,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::InvalidName(name) => write!(
                f,
                "invalid profile name {name:?}: use letters, digits, '-', '_', or '.'"
            ),
            ProfileError::NoConfigDirectory => {
                write!(f, "could not determine the user's configuration directory")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// Resolve `name` under the current user's per-OS config directory.
    pub fn resolve(name: &str) -> Result<Profile, ProfileError> {
        let base = BaseDirs::new().ok_or(ProfileError::NoConfigDirectory)?;
        Profile::resolve_under(base.config_dir(), name)
    }

    /// Resolve `name` under an explicit config root. `resolve` passes the
    /// OS config directory; tests pass a temporary directory.
    pub fn resolve_under(config_root: &Path, name: &str) -> Result<Profile, ProfileError> {
        if !is_valid_name(name) {
            return Err(ProfileError::InvalidName(name.to_string()));
        }
        Ok(Profile {
            name: name.to_string(),
            dir: config_root
                .join(APP_DIR_NAME)
                .join(PROFILES_DIR_NAME)
                .join(name),
        })
    }

    pub fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.toml")
    }

    /// Reserved for Phase 2.
    pub fn keymap_path(&self) -> PathBuf {
        self.dir.join("keymap.toml")
    }

    /// Servo's config directory: cookies, local storage, HTTP cache.
    pub fn servo_dir(&self) -> PathBuf {
        self.dir.join("servo")
    }

    /// Create the profile directory and Servo's subdirectory.
    pub fn ensure_dirs(&self) -> io::Result<()> {
        std::fs::create_dir_all(self.servo_dir())
    }
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_under_app_and_profiles_directories() {
        let p = Profile::resolve_under(Path::new("/cfg"), "work").unwrap();
        assert_eq!(p.name, "work");
        assert_eq!(p.dir, Path::new("/cfg/Paladin/profiles/work"));
        assert_eq!(
            p.settings_path(),
            Path::new("/cfg/Paladin/profiles/work/settings.toml")
        );
        assert_eq!(
            p.keymap_path(),
            Path::new("/cfg/Paladin/profiles/work/keymap.toml")
        );
        assert_eq!(p.servo_dir(), Path::new("/cfg/Paladin/profiles/work/servo"));
    }

    #[test]
    fn rejects_names_that_are_not_a_single_path_component() {
        for bad in ["", ".", "..", "a/b", "a\\b", "a b", "ä"] {
            assert_eq!(
                Profile::resolve_under(Path::new("/cfg"), bad),
                Err(ProfileError::InvalidName(bad.to_string())),
                "{bad:?} should be rejected"
            );
        }
        assert!(Profile::resolve_under(Path::new("/cfg"), "ok-name_1.2").is_ok());
    }

    #[test]
    fn resolve_uses_the_platform_config_directory() {
        let p = Profile::resolve(DEFAULT_PROFILE_NAME).unwrap();
        let dir = p.dir.to_string_lossy().replace('\\', "/");
        assert!(dir.ends_with("/Paladin/profiles/default"), "{dir}");
        if cfg!(target_os = "macos") {
            assert!(
                dir.contains("/Library/Application Support/Paladin/"),
                "{dir}"
            );
        }
        if cfg!(target_os = "windows") {
            assert!(dir.contains("/AppData/Roaming/Paladin/"), "{dir}");
        }
    }

    #[test]
    fn ensure_dirs_creates_profile_and_servo_directories() {
        let root = tempfile::tempdir().unwrap();
        let p = Profile::resolve_under(root.path(), "t").unwrap();
        p.ensure_dirs().unwrap();
        assert!(p.servo_dir().is_dir());
    }
}
