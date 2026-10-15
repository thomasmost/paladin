//! Servo-free model of the browser. Grows module by module through Phase 2;
//! Task 5 replaces this file with its final form.

pub mod command;
pub mod event;
pub mod model;
pub mod profile;
pub mod settings;

pub use command::{Command, EngineAction, dispatch};
pub use event::{EngineEvent, PendingTab, apply};
pub use model::{Browser, Favicon, LoadState, Tab, TabId, Window, WindowId};
pub use profile::{Profile, ProfileError};
pub use settings::{Settings, SettingsError};
