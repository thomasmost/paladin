//! Servo-free model of the browser. Grows module by module through Phase 2;
//! Task 5 replaces this file with its final form.

pub mod profile;
pub mod settings;

pub use profile::{Profile, ProfileError};
pub use settings::{Settings, SettingsError};
