# Paladin Phase 1 Desktop Browser Implementation Plan

**Goal:** A desktop browser for macOS and Windows that renders pages with Servo under an egui chrome, with multiple windows and tabs, a settings window, a logo, installable packages, and CI on both platforms.

**Architecture:** Three-crate Cargo workspace plus an `xtask` helper. `paladin-core` holds the Servo-free model, named commands, keymap, settings, and profile paths. `paladin-engine` is the only crate that imports Servo and exposes `Engine`, `GlSurface`, and `TabHost`. The `paladin` binary runs the winit event loop and draws the chrome with egui, blitting Servo's offscreen framebuffer under the toolbar. The UI only emits `Command`s; the pure `dispatch` and `apply` functions in core turn commands and engine events into state changes plus `EngineAction`s.

**Tech Stack:** Rust 1.97.1 (edition 2024), servo 0.7.0, winit 0.30.13, egui / egui-winit / egui_glow 0.34.3, glow 0.17.0, surfman 0.14.0, keyboard-types 0.8.3, directories 6, toml 1.1, clap 4, resvg 0.48.1, icns 0.5, ico 0.6, cargo-packager 0.11.8, GitHub Actions on `macos-15` and `windows-2025`.

**Scope:** 6 phases from original design (this file is phase 2 of 6)

**Codebase verified:** 2026-10-14. Phase 1 leaves `crates/paladin-core/src/lib.rs` as a one-line placeholder and `crates/paladin-core/Cargo.toml` already declaring `directories`, `keyboard-types`, `serde`, `toml`, `url`, and the `tempfile` dev-dependency. Every file below was compiled and its tests run (48 tests, all passing) with the pinned toolchain, clippy clean.

---

## Conventions for every task in every phase

- Run every command from the repository root, `/Users/thomas/vibes/paladin`.
- Commit to the current branch (`main`) with Conventional Commits. No feature branches.
- "Create" means the file does not exist yet; write exactly the content shown. "Replace" means overwrite the whole file with the content shown. Code blocks are complete files unless a step says otherwise.
- Before every commit run `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`. All three must be clean.
- Tests are deliverables with the code: every module in this phase ships with its unit tests in a `#[cfg(test)] mod tests` block, and a task is done only when `cargo test -p paladin-core` reports the listed tests passing.

---

## Phase 2: Core model

**Goal:** The Servo-free model, commands, keymap, settings, and profile resolution, fully unit-tested.

**Done when:** Tests cover tab creation, activation, close with next-active selection, next/previous tab wraparound, every `EngineEvent` mutating state, keymap resolving default chords, settings round-trip with missing fields defaulting, and profile paths per OS; `cargo test -p paladin-core` passes locally and in CI.

Design decisions carried into the code (deviations from the design's sketch, each deliberate):

- `apply` returns `Vec<EngineAction>` instead of nothing. Two engine events need follow-up: a page opening a new tab (`NewTabRequested`) must be adopted into a window, and a page closing itself (`TabClosed`) may close a window. Returning actions keeps `apply` pure, like `dispatch`.
- `FaviconChanged` carries decoded RGBA (`Favicon { width, height, rgba }`) rather than PNG bytes. Servo hands the engine decoded pixels and egui consumes RGBA directly, so a PNG round trip would add two codecs for nothing.
- `EngineEvent::EngineError(String)` is added for Servo's engine-level `notify_error`; the design says it is logged and shown in the status area, which needs a place in the model (`Browser::engine_error`).
- `Keymap` maps a chord to a `CommandKind` (a `Command` without its window or tab argument); the app fills in the focused window and active tab. Chords use physical key codes (`keyboard_types::Code`) so `Cmd+Shift+]` is layout-independent.
- Two `EngineAction`s exist beyond the design's list: `OpenWindow` and `AdoptTab`, plus app-level `FocusAddressBar`, `OpenSettings`, and `Quit`. They are documented on the enum.
- "Every command reachable by its default chord" applies to `CommandKind`, the chord-bindable kinds. `ActivateTab`, `Navigate`, and `DismissEngineError` carry data only the UI has (a specific tab, a URL typed in, a message to dismiss) and have no chord; the test `every_command_kind_has_a_default_chord_on_both_platforms` checks the bindable set.
- One `Command` exists beyond the design's list: `DismissEngineError`, which clears the engine error from the status area (the design shows the error there, so the user needs a way to dismiss it).
- The default keymap leaves bare Escape and Cmd+Arrow unbound: pages need Escape to close menus and dialogs, and Cmd+Left/Right move the caret in text fields. Stop is Cmd+. (Ctrl+. on Windows), the macOS "cancel" convention; Back and Forward are Cmd+[ and Cmd+] on macOS and Alt+Left and Alt+Right on Windows.

<!-- START_TASK_1 -->
### Task 1: Settings with TOML round trip

**Files:**
- Create: `crates/paladin-core/src/settings.rs`
- Replace: `crates/paladin-core/src/lib.rs`

**Step 1: Create the module with its tests**

`crates/paladin-core/src/settings.rs`:

```rust
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
```

**Step 2: Declare it in lib.rs**

Replace `crates/paladin-core/src/lib.rs` with:

```rust
//! Servo-free model of the browser. Grows module by module through Phase 2;
//! Task 5 replaces this file with its final form.

pub mod settings;

pub use settings::{Settings, SettingsError};
```

**Step 3: Run the tests**

Run: `cargo test -p paladin-core`
Expected: 5 tests pass: `round_trips_through_toml`, `missing_fields_take_defaults`, `empty_file_is_all_defaults`, `malformed_toml_is_an_error`, `load_of_missing_file_is_default_and_save_creates_parents`.

**Step 4: Commit**

```bash
git add crates/paladin-core
git commit -m "feat(core): settings with TOML load and save"
```
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: Profile directories per OS

**Files:**
- Create: `crates/paladin-core/src/profile.rs`
- Modify: `crates/paladin-core/src/lib.rs`

**Step 1: Create the module with its tests**

`crates/paladin-core/src/profile.rs`:

```rust
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
```

**Step 2: Declare it in lib.rs**

In `crates/paladin-core/src/lib.rs`, change the module and re-export lines at the bottom to:

```rust
pub mod profile;
pub mod settings;

pub use profile::{Profile, ProfileError};
pub use settings::{Settings, SettingsError};
```

**Step 3: Run the tests**

Run: `cargo test -p paladin-core`
Expected: 9 tests pass; the four new ones are `resolves_under_app_and_profiles_directories`, `rejects_names_that_are_not_a_single_path_component`, `resolve_uses_the_platform_config_directory` (on this Mac it checks the path contains `/Library/Application Support/Paladin/`), and `ensure_dirs_creates_profile_and_servo_directories`.

**Step 4: Commit**

```bash
git add crates/paladin-core
git commit -m "feat(core): profile directory resolution"
```
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: Browser, window, and tab model

**Files:**
- Create: `crates/paladin-core/src/model.rs`
- Modify: `crates/paladin-core/src/lib.rs`

**Step 1: Create the module with its tests**

`crates/paladin-core/src/model.rs`:

```rust
//! Windows, tabs, and the identifiers that cross crate boundaries.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::profile::Profile;
use crate::settings::Settings;

/// Identifies a browser window. Allocated by [`Browser`]; Servo ids never
/// leave the engine crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WindowId(pub u64);

/// Identifies a tab. Allocated by [`Browser`]; Servo ids never leave the
/// engine crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TabId(pub u64);

/// Where a tab is in its page load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadState {
    #[default]
    Idle,
    Loading,
    Complete,
}

/// A decoded favicon, straight RGBA8, row-major, top-left origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Favicon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One page shown as a tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub url: Url,
    pub title: String,
    pub favicon: Option<Favicon>,
    pub load: LoadState,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub status: String,
    /// `Some(reason)` after the engine reported this tab's content crashed.
    /// The tab stays in the model so its URL is not lost; the UI shows an
    /// error panel with a reload button instead of page content.
    pub crashed: Option<String>,
}

impl Tab {
    pub fn new(id: TabId, url: Url) -> Tab {
        Tab {
            id,
            url,
            title: String::new(),
            favicon: None,
            load: LoadState::Idle,
            can_go_back: false,
            can_go_forward: false,
            status: String::new(),
            crashed: None,
        }
    }

    /// The text to show in the tab strip: the page title, or the URL until
    /// a title arrives.
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            self.url.as_str()
        } else {
            &self.title
        }
    }
}

/// A browser window. Invariant: a window always has at least one tab and
/// `active` names one of them. Closing the last tab closes the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: WindowId,
    pub tabs: Vec<Tab>,
    pub active: TabId,
}

impl Window {
    pub fn index_of(&self, tab: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == tab)
    }

    pub fn tab(&self, tab: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == tab)
    }

    pub fn tab_mut(&mut self, tab: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == tab)
    }

    /// Index of the active tab. Panics only if the window invariant is broken.
    pub fn active_index(&self) -> usize {
        self.index_of(self.active)
            .expect("window invariant: active tab is one of the window's tabs")
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_index()]
    }
}

/// The whole browser state for one process (one profile).
#[derive(Debug)]
pub struct Browser {
    pub profile: Profile,
    pub settings: Settings,
    pub windows: Vec<Window>,
    /// Last engine-level error reported by Servo, shown in the status area.
    pub engine_error: Option<String>,
    next_window_id: u64,
    next_tab_id: u64,
}

impl Browser {
    pub fn new(profile: Profile, settings: Settings) -> Browser {
        Browser {
            profile,
            settings,
            windows: Vec::new(),
            engine_error: None,
            next_window_id: 1,
            next_tab_id: 1,
        }
    }

    pub fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn window_of_tab(&self, tab: TabId) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|w| w.index_of(tab).is_some())
            .map(|w| w.id)
    }

    pub fn tab(&self, tab: TabId) -> Option<&Tab> {
        self.windows.iter().find_map(|w| w.tab(tab))
    }

    pub fn tab_mut(&mut self, tab: TabId) -> Option<&mut Tab> {
        self.windows.iter_mut().find_map(|w| w.tab_mut(tab))
    }

    /// The URL new tabs open with, from settings, falling back to
    /// `about:blank` if the setting does not parse.
    pub fn new_tab_url(&self) -> Url {
        Url::parse(&self.settings.new_tab_url)
            .unwrap_or_else(|_| Url::parse("about:blank").expect("about:blank parses"))
    }

    /// The homepage from settings, falling back to the new-tab URL.
    pub fn homepage(&self) -> Url {
        Url::parse(&self.settings.homepage).unwrap_or_else(|_| self.new_tab_url())
    }

    pub(crate) fn allocate_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    pub(crate) fn allocate_tab_id(&mut self) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        id
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    use super::*;

    /// A browser with no windows, a throwaway profile, and default settings.
    pub(crate) fn browser() -> Browser {
        let profile = Profile {
            name: "test".to_string(),
            dir: PathBuf::from("/nonexistent/paladin-test-profile"),
        };
        Browser::new(profile, Settings::default())
    }

    pub(crate) fn url(s: &str) -> Url {
        Url::parse(s).expect("test URL parses")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{browser, url};
    use super::*;

    #[test]
    fn display_title_falls_back_to_url() {
        let mut tab = Tab::new(TabId(1), url("https://example.com/"));
        assert_eq!(tab.display_title(), "https://example.com/");
        tab.title = "Example".to_string();
        assert_eq!(tab.display_title(), "Example");
    }

    #[test]
    fn new_tab_url_falls_back_to_about_blank_when_setting_is_invalid() {
        let mut b = browser();
        b.settings.new_tab_url = "not a url".to_string();
        assert_eq!(b.new_tab_url().as_str(), "about:blank");
    }

    #[test]
    fn ids_are_allocated_sequentially() {
        let mut b = browser();
        assert_eq!(b.allocate_tab_id(), TabId(1));
        assert_eq!(b.allocate_tab_id(), TabId(2));
        assert_eq!(b.allocate_window_id(), WindowId(1));
        assert_eq!(b.allocate_window_id(), WindowId(2));
    }
}
```

**Step 2: Declare it in lib.rs**

In `crates/paladin-core/src/lib.rs`, change the module and re-export lines to:

```rust
pub mod model;
pub mod profile;
pub mod settings;

pub use model::{Browser, Favicon, LoadState, Tab, TabId, Window, WindowId};
pub use profile::{Profile, ProfileError};
pub use settings::{Settings, SettingsError};
```

**Step 3: Run the tests**

Run: `cargo test -p paladin-core`
Expected: 12 tests pass; the three new ones are `display_title_falls_back_to_url`, `new_tab_url_falls_back_to_about_blank_when_setting_is_invalid`, and `ids_are_allocated_sequentially`.

**Step 4: Commit**

```bash
git add crates/paladin-core
git commit -m "feat(core): browser, window, and tab model"
```
<!-- END_TASK_3 -->

<!-- START_SUBCOMPONENT_A (tasks 4-4) -->
<!-- START_TASK_4 -->
### Task 4: Commands, engine events, dispatch, and apply

`command.rs` and `event.rs` reference each other (`dispatch` and `apply` share the close-tab logic, and `EngineAction::AdoptTab` carries an event type), so they are created together.

**Files:**
- Create: `crates/paladin-core/src/command.rs`
- Create: `crates/paladin-core/src/event.rs`
- Modify: `crates/paladin-core/src/lib.rs`

**Step 1: Create the command module with its tests**

`crates/paladin-core/src/command.rs`:

```rust
//! Named user actions and the pure `dispatch` that applies them.

use url::Url;

use crate::event::PendingTab;
use crate::model::{Browser, LoadState, Tab, TabId, Window, WindowId};

/// Every user action in Paladin is one of these. The chrome, the keymap,
/// and (in Phase 2) remappable hotkeys all produce `Command`s; nothing else
/// mutates the model from the UI side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    NewTab {
        window: WindowId,
        url: Option<Url>,
    },
    CloseTab(TabId),
    ActivateTab(TabId),
    NextTab(WindowId),
    PreviousTab(WindowId),
    Navigate {
        tab: TabId,
        url: Url,
    },
    Back(TabId),
    Forward(TabId),
    Reload(TabId),
    Stop(TabId),
    FocusAddressBar(WindowId),
    NewWindow,
    CloseWindow(WindowId),
    OpenSettings,
    /// Clear the engine error shown in the status area.
    DismissEngineError,
    Quit,
}

/// What the application must do after [`dispatch`] (or [`crate::apply`])
/// has updated the model. Most variants are forwarded to the engine's tab
/// host; `OpenWindow`, `CloseWindow`, `FocusAddressBar`, `OpenSettings`, and
/// `Quit` are handled by the app itself. Returning these instead of calling
/// the engine directly keeps `dispatch` pure and testable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineAction {
    /// Create and show a new engine tab for `tab` in `window`, loading `url`.
    CreateTab {
        window: WindowId,
        tab: TabId,
        url: Url,
    },
    /// Adopt a tab the engine already created (a `window.open` request) as
    /// `tab` in `window`, and show it.
    AdoptTab {
        window: WindowId,
        tab: TabId,
        pending: PendingTab,
    },
    Navigate {
        tab: TabId,
        url: Url,
    },
    Back(TabId),
    Forward(TabId),
    Reload(TabId),
    Stop(TabId),
    Show(TabId),
    Hide(TabId),
    CloseTab(TabId),
    /// Open a native window for `window` whose first tab is `tab` at `url`.
    OpenWindow {
        window: WindowId,
        tab: TabId,
        url: Url,
    },
    CloseWindow(WindowId),
    FocusAddressBar(WindowId),
    OpenSettings,
    Quit,
}

/// Apply a command to the model and return what the app must do next.
pub fn dispatch(browser: &mut Browser, cmd: &Command) -> Vec<EngineAction> {
    match cmd {
        Command::NewTab { window, url } => new_tab(browser, *window, url.clone()),
        Command::CloseTab(tab) => close_tab(browser, *tab),
        Command::ActivateTab(tab) => activate_tab(browser, *tab),
        Command::NextTab(window) => step_tab(browser, *window, 1),
        Command::PreviousTab(window) => step_tab(browser, *window, -1),
        Command::Navigate { tab, url } => match browser.tab_mut(*tab) {
            Some(t) => {
                t.url = url.clone();
                t.load = LoadState::Loading;
                t.crashed = None;
                vec![EngineAction::Navigate {
                    tab: *tab,
                    url: url.clone(),
                }]
            }
            None => Vec::new(),
        },
        Command::Back(tab) => pass_through(browser, *tab, EngineAction::Back(*tab)),
        Command::Forward(tab) => pass_through(browser, *tab, EngineAction::Forward(*tab)),
        Command::Reload(tab) => match browser.tab_mut(*tab) {
            Some(t) => {
                t.load = LoadState::Loading;
                t.crashed = None;
                vec![EngineAction::Reload(*tab)]
            }
            None => Vec::new(),
        },
        Command::Stop(tab) => pass_through(browser, *tab, EngineAction::Stop(*tab)),
        Command::FocusAddressBar(window) => {
            if browser.window(*window).is_some() {
                vec![EngineAction::FocusAddressBar(*window)]
            } else {
                Vec::new()
            }
        }
        Command::NewWindow => {
            let url = browser.new_tab_url();
            open_window(browser, url)
        }
        Command::CloseWindow(window) => close_window(browser, *window),
        Command::OpenSettings => vec![EngineAction::OpenSettings],
        Command::DismissEngineError => {
            browser.engine_error = None;
            Vec::new()
        }
        Command::Quit => vec![EngineAction::Quit],
    }
}

/// Add a window to the model whose first tab shows `url`. Used by
/// `Command::NewWindow` and by `main` for the initial window, which opens
/// the homepage or a URL given on the command line.
pub fn open_window(browser: &mut Browser, url: Url) -> Vec<EngineAction> {
    let window = browser.allocate_window_id();
    let tab = browser.allocate_tab_id();
    browser.windows.push(Window {
        id: window,
        tabs: vec![Tab::new(tab, url.clone())],
        active: tab,
    });
    vec![EngineAction::OpenWindow { window, tab, url }]
}

fn pass_through(browser: &Browser, tab: TabId, action: EngineAction) -> Vec<EngineAction> {
    if browser.tab(tab).is_some() {
        vec![action]
    } else {
        Vec::new()
    }
}

fn new_tab(browser: &mut Browser, window: WindowId, url: Option<Url>) -> Vec<EngineAction> {
    if browser.window(window).is_none() {
        return Vec::new();
    }
    let url = url.unwrap_or_else(|| browser.new_tab_url());
    let tab = browser.allocate_tab_id();
    let w = browser
        .window_mut(window)
        .expect("window existence checked above");
    let previous = w.active;
    w.tabs.push(Tab::new(tab, url.clone()));
    w.active = tab;
    vec![
        EngineAction::Hide(previous),
        EngineAction::CreateTab { window, tab, url },
    ]
}

/// Remove `tab`. If it was active, the tab to its right becomes active, or
/// the one to its left if it was last. Closing the last tab closes the
/// window, and closing the last window quits.
pub(crate) fn close_tab(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    let mut actions = vec![EngineAction::CloseTab(tab)];
    let new_active = {
        let w = browser
            .window_mut(window_id)
            .expect("window_of_tab returned an existing window");
        let index = w.index_of(tab).expect("tab is in this window");
        w.tabs.remove(index);
        if w.tabs.is_empty() {
            None
        } else if w.active == tab {
            let next = index.min(w.tabs.len() - 1);
            w.active = w.tabs[next].id;
            Some(w.active)
        } else {
            return actions;
        }
    };
    match new_active {
        Some(next) => actions.push(EngineAction::Show(next)),
        None => {
            browser.windows.retain(|w| w.id != window_id);
            actions.push(EngineAction::CloseWindow(window_id));
            if browser.windows.is_empty() {
                actions.push(EngineAction::Quit);
            }
        }
    }
    actions
}

fn activate_tab(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    let w = browser
        .window_mut(window_id)
        .expect("window_of_tab returned an existing window");
    if w.active == tab {
        return Vec::new();
    }
    let previous = w.active;
    w.active = tab;
    vec![EngineAction::Hide(previous), EngineAction::Show(tab)]
}

fn step_tab(browser: &mut Browser, window: WindowId, delta: isize) -> Vec<EngineAction> {
    let Some(w) = browser.window(window) else {
        return Vec::new();
    };
    let len = w.tabs.len() as isize;
    let next = (w.active_index() as isize + delta).rem_euclid(len) as usize;
    let target = w.tabs[next].id;
    activate_tab(browser, target)
}

fn close_window(browser: &mut Browser, window: WindowId) -> Vec<EngineAction> {
    let Some(position) = browser.windows.iter().position(|w| w.id == window) else {
        return Vec::new();
    };
    let removed = browser.windows.remove(position);
    let mut actions: Vec<EngineAction> = removed
        .tabs
        .iter()
        .map(|t| EngineAction::CloseTab(t.id))
        .collect();
    actions.push(EngineAction::CloseWindow(window));
    if browser.windows.is_empty() {
        actions.push(EngineAction::Quit);
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_support::{browser, url};

    /// A browser with one window holding one tab. Returns the ids.
    fn one_window() -> (Browser, WindowId, TabId) {
        let mut b = browser();
        let actions = open_window(&mut b, url("https://a.example/"));
        let EngineAction::OpenWindow { window, tab, .. } = actions[0].clone() else {
            panic!("open_window returns OpenWindow");
        };
        (b, window, tab)
    }

    #[test]
    fn open_window_creates_window_with_one_active_tab() {
        let (b, window, tab) = one_window();
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 1);
        assert_eq!(w.active, tab);
        assert_eq!(w.active_tab().url.as_str(), "https://a.example/");
    }

    #[test]
    fn new_tab_uses_new_tab_url_and_becomes_active() {
        let (mut b, window, first) = one_window();
        b.settings.new_tab_url = "https://new.example/".to_string();
        let actions = dispatch(&mut b, &Command::NewTab { window, url: None });
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 2);
        let second = w.tabs[1].id;
        assert_eq!(w.active, second);
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(first),
                EngineAction::CreateTab {
                    window,
                    tab: second,
                    url: url("https://new.example/"),
                },
            ]
        );
    }

    #[test]
    fn new_tab_with_explicit_url_uses_it() {
        let (mut b, window, _) = one_window();
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://b.example/")),
            },
        );
        assert_eq!(
            b.window(window).unwrap().active_tab().url.as_str(),
            "https://b.example/"
        );
    }

    #[test]
    fn new_tab_in_unknown_window_does_nothing() {
        let (mut b, _, _) = one_window();
        let actions = dispatch(
            &mut b,
            &Command::NewTab {
                window: WindowId(99),
                url: None,
            },
        );
        assert!(actions.is_empty());
        assert_eq!(b.windows.len(), 1);
    }

    #[test]
    fn activate_tab_hides_previous_and_shows_target() {
        let (mut b, window, first) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let second = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::ActivateTab(first));
        assert_eq!(b.window(window).unwrap().active, first);
        assert_eq!(
            actions,
            vec![EngineAction::Hide(second), EngineAction::Show(first)]
        );
        assert!(dispatch(&mut b, &Command::ActivateTab(first)).is_empty());
    }

    #[test]
    fn close_active_tab_activates_tab_to_the_right_then_left() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t3 = b.window(window).unwrap().active;

        dispatch(&mut b, &Command::ActivateTab(t2));
        let actions = dispatch(&mut b, &Command::CloseTab(t2));
        assert_eq!(b.window(window).unwrap().active, t3, "right neighbour wins");
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t2), EngineAction::Show(t3)]
        );

        let actions = dispatch(&mut b, &Command::CloseTab(t3));
        assert_eq!(
            b.window(window).unwrap().active,
            t1,
            "last tab falls back left"
        );
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t3), EngineAction::Show(t1)]
        );
    }

    #[test]
    fn close_inactive_tab_keeps_active_and_emits_no_show() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::CloseTab(t1));
        assert_eq!(b.window(window).unwrap().active, t2);
        assert_eq!(actions, vec![EngineAction::CloseTab(t1)]);
    }

    #[test]
    fn closing_last_tab_closes_window_and_last_window_quits() {
        let (mut b, w1, t1) = one_window();
        let w2_actions = dispatch(&mut b, &Command::NewWindow);
        let EngineAction::OpenWindow {
            window: w2,
            tab: t2,
            ..
        } = w2_actions[0].clone()
        else {
            panic!("NewWindow returns OpenWindow");
        };

        let actions = dispatch(&mut b, &Command::CloseTab(t1));
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t1), EngineAction::CloseWindow(w1)]
        );
        assert!(b.window(w1).is_none());

        let actions = dispatch(&mut b, &Command::CloseTab(t2));
        assert_eq!(
            actions,
            vec![
                EngineAction::CloseTab(t2),
                EngineAction::CloseWindow(w2),
                EngineAction::Quit,
            ]
        );
        assert!(b.windows.is_empty());
    }

    #[test]
    fn next_and_previous_tab_wrap_around() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t3 = b.window(window).unwrap().active;

        dispatch(&mut b, &Command::NextTab(window));
        assert_eq!(
            b.window(window).unwrap().active,
            t1,
            "wraps from last to first"
        );
        dispatch(&mut b, &Command::PreviousTab(window));
        assert_eq!(
            b.window(window).unwrap().active,
            t3,
            "wraps from first to last"
        );
        dispatch(&mut b, &Command::PreviousTab(window));
        assert_eq!(b.window(window).unwrap().active, t2);
    }

    #[test]
    fn next_tab_with_single_tab_is_a_no_op() {
        let (mut b, window, t1) = one_window();
        assert!(dispatch(&mut b, &Command::NextTab(window)).is_empty());
        assert_eq!(b.window(window).unwrap().active, t1);
    }

    #[test]
    fn navigate_updates_url_marks_loading_and_clears_crash() {
        let (mut b, _, tab) = one_window();
        b.tab_mut(tab).unwrap().crashed = Some("boom".to_string());
        let actions = dispatch(
            &mut b,
            &Command::Navigate {
                tab,
                url: url("https://b.example/"),
            },
        );
        let t = b.tab(tab).unwrap();
        assert_eq!(t.url.as_str(), "https://b.example/");
        assert_eq!(t.load, LoadState::Loading);
        assert_eq!(t.crashed, None);
        assert_eq!(
            actions,
            vec![EngineAction::Navigate {
                tab,
                url: url("https://b.example/"),
            }]
        );
    }

    #[test]
    fn back_forward_reload_stop_pass_through_for_known_tabs_only() {
        let (mut b, _, tab) = one_window();
        assert_eq!(
            dispatch(&mut b, &Command::Back(tab)),
            vec![EngineAction::Back(tab)]
        );
        assert_eq!(
            dispatch(&mut b, &Command::Forward(tab)),
            vec![EngineAction::Forward(tab)]
        );
        assert_eq!(
            dispatch(&mut b, &Command::Reload(tab)),
            vec![EngineAction::Reload(tab)]
        );
        assert_eq!(b.tab(tab).unwrap().load, LoadState::Loading);
        assert_eq!(
            dispatch(&mut b, &Command::Stop(tab)),
            vec![EngineAction::Stop(tab)]
        );
        assert!(dispatch(&mut b, &Command::Back(TabId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::Reload(TabId(99))).is_empty());
    }

    #[test]
    fn new_window_opens_with_new_tab_url() {
        let (mut b, _, _) = one_window();
        b.settings.new_tab_url = "https://new.example/".to_string();
        let actions = dispatch(&mut b, &Command::NewWindow);
        assert_eq!(b.windows.len(), 2);
        let w = &b.windows[1];
        assert_eq!(
            actions,
            vec![EngineAction::OpenWindow {
                window: w.id,
                tab: w.active,
                url: url("https://new.example/"),
            }]
        );
    }

    #[test]
    fn close_window_closes_all_its_tabs_and_quits_when_last() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::CloseWindow(window));
        assert_eq!(
            actions,
            vec![
                EngineAction::CloseTab(t1),
                EngineAction::CloseTab(t2),
                EngineAction::CloseWindow(window),
                EngineAction::Quit,
            ]
        );
        assert!(b.windows.is_empty());
        assert!(dispatch(&mut b, &Command::CloseWindow(window)).is_empty());
    }

    #[test]
    fn focus_settings_and_quit_are_forwarded_to_the_app() {
        let (mut b, window, _) = one_window();
        assert_eq!(
            dispatch(&mut b, &Command::FocusAddressBar(window)),
            vec![EngineAction::FocusAddressBar(window)]
        );
        assert!(dispatch(&mut b, &Command::FocusAddressBar(WindowId(99))).is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::OpenSettings),
            vec![EngineAction::OpenSettings]
        );
        assert_eq!(dispatch(&mut b, &Command::Quit), vec![EngineAction::Quit]);
    }

    #[test]
    fn dismiss_engine_error_clears_it() {
        let (mut b, _, _) = one_window();
        b.engine_error = Some("lost backend".to_string());
        assert!(dispatch(&mut b, &Command::DismissEngineError).is_empty());
        assert_eq!(b.engine_error, None);
    }
}
```

**Step 2: Create the event module with its tests**

`crates/paladin-core/src/event.rs`:

```rust
//! Notifications from the engine and the pure `apply` that folds them into
//! the model.

use url::Url;

use crate::command::{EngineAction, close_tab};
use crate::model::{Browser, Favicon, LoadState, Tab, TabId};

/// Handle to an engine tab that page content created (`window.open`) and
/// that the engine is holding until the model adopts it with
/// [`EngineAction::AdoptTab`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PendingTab(pub u64);

/// Something the engine reported. Produced by the engine crate from Servo
/// delegate callbacks, keyed by core ids.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    UrlChanged {
        tab: TabId,
        url: Url,
    },
    TitleChanged {
        tab: TabId,
        title: String,
    },
    LoadStateChanged {
        tab: TabId,
        state: LoadState,
    },
    HistoryChanged {
        tab: TabId,
        can_go_back: bool,
        can_go_forward: bool,
    },
    StatusChanged {
        tab: TabId,
        text: String,
    },
    FaviconChanged {
        tab: TabId,
        favicon: Favicon,
    },
    /// Page content in `opener` asked for a new tab. The engine already
    /// created it and holds it as `pending`.
    NewTabRequested {
        opener: TabId,
        pending: PendingTab,
    },
    /// Page content closed the tab (`window.close()`).
    TabClosed(TabId),
    TabCrashed {
        tab: TabId,
        reason: String,
    },
    /// The engine painted a new frame for this tab; the window showing it
    /// should redraw.
    FrameReady(TabId),
    /// An engine-level error not tied to a tab (for example, losing the
    /// connection to the engine backend).
    EngineError(String),
}

/// Fold an engine event into the model. Returns follow-up actions; only
/// `NewTabRequested` and `TabClosed` produce any.
pub fn apply(browser: &mut Browser, event: &EngineEvent) -> Vec<EngineAction> {
    match event {
        EngineEvent::UrlChanged { tab, url } => {
            with_tab(browser, *tab, |t| t.url = url.clone());
            Vec::new()
        }
        EngineEvent::TitleChanged { tab, title } => {
            with_tab(browser, *tab, |t| t.title = title.clone());
            Vec::new()
        }
        EngineEvent::LoadStateChanged { tab, state } => {
            with_tab(browser, *tab, |t| t.load = *state);
            Vec::new()
        }
        EngineEvent::HistoryChanged {
            tab,
            can_go_back,
            can_go_forward,
        } => {
            with_tab(browser, *tab, |t| {
                t.can_go_back = *can_go_back;
                t.can_go_forward = *can_go_forward;
            });
            Vec::new()
        }
        EngineEvent::StatusChanged { tab, text } => {
            with_tab(browser, *tab, |t| t.status = text.clone());
            Vec::new()
        }
        EngineEvent::FaviconChanged { tab, favicon } => {
            with_tab(browser, *tab, |t| t.favicon = Some(favicon.clone()));
            Vec::new()
        }
        EngineEvent::NewTabRequested { opener, pending } => {
            let Some(window) = browser.window_of_tab(*opener) else {
                return Vec::new();
            };
            let tab = browser.allocate_tab_id();
            let w = browser
                .window_mut(window)
                .expect("window_of_tab returned an existing window");
            let previous = w.active;
            w.tabs.push(Tab::new(
                tab,
                Url::parse("about:blank").expect("about:blank parses"),
            ));
            w.active = tab;
            vec![
                EngineAction::Hide(previous),
                EngineAction::AdoptTab {
                    window,
                    tab,
                    pending: *pending,
                },
            ]
        }
        EngineEvent::TabClosed(tab) => close_tab(browser, *tab),
        EngineEvent::TabCrashed { tab, reason } => {
            with_tab(browser, *tab, |t| {
                t.crashed = Some(reason.clone());
                t.load = LoadState::Idle;
            });
            Vec::new()
        }
        EngineEvent::FrameReady(_) => Vec::new(),
        EngineEvent::EngineError(message) => {
            browser.engine_error = Some(message.clone());
            Vec::new()
        }
    }
}

fn with_tab(browser: &mut Browser, tab: TabId, f: impl FnOnce(&mut Tab)) {
    if let Some(t) = browser.tab_mut(tab) {
        f(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, dispatch, open_window};
    use crate::model::WindowId;
    use crate::model::test_support::{browser, url};

    fn one_window() -> (Browser, WindowId, TabId) {
        let mut b = browser();
        let actions = open_window(&mut b, url("https://a.example/"));
        let EngineAction::OpenWindow { window, tab, .. } = actions[0].clone() else {
            panic!("open_window returns OpenWindow");
        };
        (b, window, tab)
    }

    #[test]
    fn url_changed_updates_tab_url() {
        let (mut b, _, tab) = one_window();
        let actions = apply(
            &mut b,
            &EngineEvent::UrlChanged {
                tab,
                url: url("https://b.example/"),
            },
        );
        assert!(actions.is_empty());
        assert_eq!(b.tab(tab).unwrap().url.as_str(), "https://b.example/");
    }

    #[test]
    fn title_changed_updates_title() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::TitleChanged {
                tab,
                title: "Hello".to_string(),
            },
        );
        assert_eq!(b.tab(tab).unwrap().title, "Hello");
    }

    #[test]
    fn load_state_changed_updates_load() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::LoadStateChanged {
                tab,
                state: LoadState::Complete,
            },
        );
        assert_eq!(b.tab(tab).unwrap().load, LoadState::Complete);
    }

    #[test]
    fn history_changed_updates_navigation_flags() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::HistoryChanged {
                tab,
                can_go_back: true,
                can_go_forward: false,
            },
        );
        let t = b.tab(tab).unwrap();
        assert!(t.can_go_back);
        assert!(!t.can_go_forward);
    }

    #[test]
    fn status_changed_updates_status() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::StatusChanged {
                tab,
                text: "https://link.example/".to_string(),
            },
        );
        assert_eq!(b.tab(tab).unwrap().status, "https://link.example/");
    }

    #[test]
    fn favicon_changed_stores_favicon() {
        let (mut b, _, tab) = one_window();
        let favicon = Favicon {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        };
        apply(
            &mut b,
            &EngineEvent::FaviconChanged {
                tab,
                favicon: favicon.clone(),
            },
        );
        assert_eq!(b.tab(tab).unwrap().favicon, Some(favicon));
    }

    #[test]
    fn new_tab_requested_adds_active_tab_and_adopts_pending() {
        let (mut b, window, opener) = one_window();
        let actions = apply(
            &mut b,
            &EngineEvent::NewTabRequested {
                opener,
                pending: PendingTab(7),
            },
        );
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 2);
        let tab = w.tabs[1].id;
        assert_eq!(w.active, tab);
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(opener),
                EngineAction::AdoptTab {
                    window,
                    tab,
                    pending: PendingTab(7),
                },
            ]
        );
    }

    #[test]
    fn new_tab_requested_for_unknown_opener_does_nothing() {
        let (mut b, _, _) = one_window();
        let actions = apply(
            &mut b,
            &EngineEvent::NewTabRequested {
                opener: TabId(99),
                pending: PendingTab(1),
            },
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn tab_closed_behaves_like_close_tab_command() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let actions = apply(&mut b, &EngineEvent::TabClosed(t2));
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t2), EngineAction::Show(t1)]
        );
        assert_eq!(b.window(window).unwrap().tabs.len(), 1);
    }

    #[test]
    fn tab_crashed_keeps_tab_and_records_reason() {
        let (mut b, _, tab) = one_window();
        apply(
            &mut b,
            &EngineEvent::LoadStateChanged {
                tab,
                state: LoadState::Loading,
            },
        );
        apply(
            &mut b,
            &EngineEvent::TabCrashed {
                tab,
                reason: "script panicked".to_string(),
            },
        );
        let t = b.tab(tab).unwrap();
        assert_eq!(t.crashed.as_deref(), Some("script panicked"));
        assert_eq!(t.load, LoadState::Idle);
        assert_eq!(t.url.as_str(), "https://a.example/");
    }

    #[test]
    fn frame_ready_changes_nothing() {
        let (mut b, _, tab) = one_window();
        let before = b.windows.clone();
        assert!(apply(&mut b, &EngineEvent::FrameReady(tab)).is_empty());
        assert_eq!(b.windows, before);
    }

    #[test]
    fn engine_error_is_recorded() {
        let (mut b, _, _) = one_window();
        apply(
            &mut b,
            &EngineEvent::EngineError("lost backend".to_string()),
        );
        assert_eq!(b.engine_error.as_deref(), Some("lost backend"));
    }

    #[test]
    fn events_for_unknown_tabs_are_ignored() {
        let (mut b, _, _) = one_window();
        let before = b.windows.clone();
        apply(
            &mut b,
            &EngineEvent::TitleChanged {
                tab: TabId(99),
                title: "x".to_string(),
            },
        );
        assert_eq!(b.windows, before);
    }
}
```

**Step 3: Declare both in lib.rs**

In `crates/paladin-core/src/lib.rs`, change the module and re-export lines to:

```rust
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
```

**Step 4: Run the tests**

Run: `cargo test -p paladin-core`
Expected: 41 tests pass. The 16 command tests include `close_active_tab_activates_tab_to_the_right_then_left`, `closing_last_tab_closes_window_and_last_window_quits`, `next_and_previous_tab_wrap_around`, and `dismiss_engine_error_clears_it`; the 13 event tests cover every `EngineEvent` variant, including `new_tab_requested_adds_active_tab_and_adopts_pending` and `tab_crashed_keeps_tab_and_records_reason`.

**Step 5: Commit**

```bash
git add crates/paladin-core
git commit -m "feat(core): commands, engine events, dispatch, and apply"
```
<!-- END_TASK_4 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_5 -->
### Task 5: Keymap with per-platform defaults

**Files:**
- Create: `crates/paladin-core/src/keymap.rs`
- Replace: `crates/paladin-core/src/lib.rs`

**Step 1: Create the module with its tests**

`crates/paladin-core/src/keymap.rs`:

```rust
//! Keyboard shortcuts: a key chord maps to a command kind. Phase 2 adds
//! persistence to `keymap.toml` and an editor; the data model is already
//! here so that is a data change, not a rewrite.

use keyboard_types::{Code, Modifiers};
use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::model::{TabId, WindowId};

/// A physical key plus modifiers. Chords use the physical key code, not
/// the produced character, so `Cmd+Shift+]` is the same chord on every
/// keyboard layout and does not depend on what `Shift+]` types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyChord {
    pub modifiers: Modifiers,
    pub code: Code,
}

impl KeyChord {
    /// Only these modifiers take part in matching; lock keys and the like
    /// are ignored.
    pub const RELEVANT_MODIFIERS: Modifiers = Modifiers::CONTROL
        .union(Modifiers::SHIFT)
        .union(Modifiers::ALT)
        .union(Modifiers::META);

    pub fn new(modifiers: Modifiers, code: Code) -> KeyChord {
        KeyChord {
            modifiers: modifiers & Self::RELEVANT_MODIFIERS,
            code,
        }
    }
}

/// A [`Command`] without its window or tab argument. The keymap resolves a
/// chord to one of these; the app fills in the focused window and its
/// active tab with [`CommandKind::into_command`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandKind {
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    Back,
    Forward,
    Reload,
    Stop,
    FocusAddressBar,
    NewWindow,
    CloseWindow,
    OpenSettings,
    Quit,
}

impl CommandKind {
    /// Every kind, in a fixed order. Used to check that the defaults bind
    /// each one and, in Phase 2, to list them in the editor.
    pub const ALL: [CommandKind; 13] = [
        CommandKind::NewTab,
        CommandKind::CloseTab,
        CommandKind::NextTab,
        CommandKind::PreviousTab,
        CommandKind::Back,
        CommandKind::Forward,
        CommandKind::Reload,
        CommandKind::Stop,
        CommandKind::FocusAddressBar,
        CommandKind::NewWindow,
        CommandKind::CloseWindow,
        CommandKind::OpenSettings,
        CommandKind::Quit,
    ];

    pub fn into_command(self, window: WindowId, tab: TabId) -> Command {
        match self {
            CommandKind::NewTab => Command::NewTab { window, url: None },
            CommandKind::CloseTab => Command::CloseTab(tab),
            CommandKind::NextTab => Command::NextTab(window),
            CommandKind::PreviousTab => Command::PreviousTab(window),
            CommandKind::Back => Command::Back(tab),
            CommandKind::Forward => Command::Forward(tab),
            CommandKind::Reload => Command::Reload(tab),
            CommandKind::Stop => Command::Stop(tab),
            CommandKind::FocusAddressBar => Command::FocusAddressBar(window),
            CommandKind::NewWindow => Command::NewWindow,
            CommandKind::CloseWindow => Command::CloseWindow(window),
            CommandKind::OpenSettings => Command::OpenSettings,
            CommandKind::Quit => Command::Quit,
        }
    }
}

/// Which platform's conventions the default bindings follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Windows
        }
    }

    /// The "primary" shortcut modifier: Command on macOS, Control elsewhere.
    pub fn primary(self) -> Modifiers {
        match self {
            Platform::MacOs => Modifiers::META,
            Platform::Windows => Modifiers::CONTROL,
        }
    }
}

/// Ordered chord-to-command bindings. Several chords may map to one kind;
/// one chord maps to at most one kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keymap {
    bindings: Vec<(KeyChord, CommandKind)>,
}

impl Keymap {
    /// The built-in bindings for a platform.
    pub fn defaults(platform: Platform) -> Keymap {
        let primary = platform.primary();
        let mut keymap = Keymap::default();
        let mut bind = |modifiers: Modifiers, code: Code, kind: CommandKind| {
            keymap.bind(KeyChord::new(modifiers, code), kind);
        };
        bind(primary, Code::KeyT, CommandKind::NewTab);
        bind(primary, Code::KeyW, CommandKind::CloseTab);
        bind(
            primary | Modifiers::SHIFT,
            Code::KeyW,
            CommandKind::CloseWindow,
        );
        bind(primary, Code::KeyN, CommandKind::NewWindow);
        bind(primary, Code::KeyL, CommandKind::FocusAddressBar);
        bind(primary, Code::KeyR, CommandKind::Reload);
        // Not bare Escape: pages need it to close menus and dialogs.
        bind(primary, Code::Period, CommandKind::Stop);
        bind(
            primary | Modifiers::SHIFT,
            Code::BracketRight,
            CommandKind::NextTab,
        );
        bind(
            primary | Modifiers::SHIFT,
            Code::BracketLeft,
            CommandKind::PreviousTab,
        );
        bind(Modifiers::CONTROL, Code::Tab, CommandKind::NextTab);
        bind(
            Modifiers::CONTROL | Modifiers::SHIFT,
            Code::Tab,
            CommandKind::PreviousTab,
        );
        bind(primary, Code::Comma, CommandKind::OpenSettings);
        bind(primary, Code::KeyQ, CommandKind::Quit);
        match platform {
            Platform::MacOs => {
                // Not Cmd+Arrow: those move the caret to line start and
                // end in text fields.
                bind(primary, Code::BracketLeft, CommandKind::Back);
                bind(primary, Code::BracketRight, CommandKind::Forward);
            }
            Platform::Windows => {
                bind(Modifiers::ALT, Code::ArrowLeft, CommandKind::Back);
                bind(Modifiers::ALT, Code::ArrowRight, CommandKind::Forward);
                bind(Modifiers::empty(), Code::F5, CommandKind::Reload);
            }
        }
        keymap
    }

    /// Bind `chord` to `kind`, replacing any previous binding of that chord.
    pub fn bind(&mut self, chord: KeyChord, kind: CommandKind) {
        self.bindings.retain(|(c, _)| *c != chord);
        self.bindings.push((chord, kind));
    }

    pub fn resolve(&self, chord: KeyChord) -> Option<CommandKind> {
        let chord = KeyChord::new(chord.modifiers, chord.code);
        self.bindings
            .iter()
            .find(|(c, _)| *c == chord)
            .map(|(_, kind)| *kind)
    }

    /// The first chord bound to `kind`, for showing hints in menus.
    pub fn chord_for(&self, kind: CommandKind) -> Option<KeyChord> {
        self.bindings
            .iter()
            .find(|(_, k)| *k == kind)
            .map(|(c, _)| *c)
    }

    pub fn bindings(&self) -> impl Iterator<Item = (KeyChord, CommandKind)> + '_ {
        self.bindings.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_defaults_use_command_key() {
        let k = Keymap::defaults(Platform::MacOs);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::META, Code::KeyT)),
            Some(CommandKind::NewTab)
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::CONTROL, Code::KeyT)),
            None
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::META, Code::BracketLeft)),
            Some(CommandKind::Back)
        );
        assert_eq!(
            k.resolve(KeyChord::new(
                Modifiers::META | Modifiers::SHIFT,
                Code::BracketLeft
            )),
            Some(CommandKind::PreviousTab)
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::META, Code::Period)),
            Some(CommandKind::Stop)
        );
    }

    #[test]
    fn keys_pages_need_are_left_unbound() {
        for platform in [Platform::MacOs, Platform::Windows] {
            let k = Keymap::defaults(platform);
            assert_eq!(
                k.resolve(KeyChord::new(Modifiers::empty(), Code::Escape)),
                None
            );
            assert_eq!(
                k.resolve(KeyChord::new(Modifiers::META, Code::ArrowLeft)),
                None
            );
            assert_eq!(
                k.resolve(KeyChord::new(Modifiers::META, Code::ArrowRight)),
                None
            );
        }
    }

    #[test]
    fn windows_defaults_use_control_key() {
        let k = Keymap::defaults(Platform::Windows);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::CONTROL, Code::KeyT)),
            Some(CommandKind::NewTab)
        );
        assert_eq!(k.resolve(KeyChord::new(Modifiers::META, Code::KeyT)), None);
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::ALT, Code::ArrowLeft)),
            Some(CommandKind::Back)
        );
        assert_eq!(
            k.resolve(KeyChord::new(Modifiers::empty(), Code::F5)),
            Some(CommandKind::Reload)
        );
    }

    #[test]
    fn every_command_kind_has_a_default_chord_on_both_platforms() {
        for platform in [Platform::MacOs, Platform::Windows] {
            let k = Keymap::defaults(platform);
            for kind in CommandKind::ALL {
                assert!(
                    k.chord_for(kind).is_some(),
                    "{kind:?} has no default on {platform:?}"
                );
            }
        }
    }

    #[test]
    fn irrelevant_modifiers_are_ignored_when_matching() {
        let k = Keymap::defaults(Platform::MacOs);
        let with_caps_lock = KeyChord {
            modifiers: Modifiers::META | Modifiers::CAPS_LOCK,
            code: Code::KeyT,
        };
        assert_eq!(k.resolve(with_caps_lock), Some(CommandKind::NewTab));
    }

    #[test]
    fn bind_replaces_an_existing_chord() {
        let mut k = Keymap::defaults(Platform::MacOs);
        let chord = KeyChord::new(Modifiers::META, Code::KeyT);
        k.bind(chord, CommandKind::Reload);
        assert_eq!(k.resolve(chord), Some(CommandKind::Reload));
        assert_eq!(k.bindings().filter(|(c, _)| *c == chord).count(), 1);
    }

    #[test]
    fn into_command_fills_in_window_and_tab() {
        let w = WindowId(3);
        let t = TabId(9);
        assert_eq!(
            CommandKind::NewTab.into_command(w, t),
            Command::NewTab {
                window: w,
                url: None
            }
        );
        assert_eq!(
            CommandKind::CloseTab.into_command(w, t),
            Command::CloseTab(t)
        );
        assert_eq!(CommandKind::NextTab.into_command(w, t), Command::NextTab(w));
        assert_eq!(CommandKind::Quit.into_command(w, t), Command::Quit);
    }
}
```

**Step 2: Replace lib.rs with its final form**

`crates/paladin-core/src/lib.rs`:

```rust
//! Servo-free model of the browser.
//!
//! This crate knows nothing about Servo, egui, or winit. It holds the window
//! and tab model, the named commands the UI emits, the events the engine
//! reports, the keymap, settings, and profile paths. The binary crate turns
//! commands into engine calls; the engine crate turns Servo callbacks into
//! [`EngineEvent`]s. Both go through the pure functions [`dispatch`] and
//! [`apply`], which is what keeps this crate unit-testable without a browser
//! engine.

pub mod command;
pub mod event;
pub mod keymap;
pub mod model;
pub mod profile;
pub mod settings;

pub use command::{Command, EngineAction, dispatch};
pub use event::{EngineEvent, PendingTab, apply};
pub use keymap::{CommandKind, KeyChord, Keymap, Platform};
pub use model::{Browser, Favicon, LoadState, Tab, TabId, Window, WindowId};
pub use profile::{Profile, ProfileError};
pub use settings::{Settings, SettingsError};
```

**Step 3: Run the tests**

Run: `cargo test -p paladin-core`
Expected: `test result: ok. 48 passed`. The seven keymap tests include `every_command_kind_has_a_default_chord_on_both_platforms`, `keys_pages_need_are_left_unbound`, and `irrelevant_modifiers_are_ignored_when_matching`.

Run: `cargo doc -p paladin-core --no-deps`
Expected: `Finished` with no warnings (the intra-doc links in lib.rs resolve now that all modules exist).

**Step 4: Record the change and commit**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, add this bullet after the existing one:

```markdown
- Servo-free core model: windows and tabs, named commands with a pure
  `dispatch`, engine events with a pure `apply`, a keymap with macOS and
  Windows defaults, TOML settings, and per-OS profile directories.
```

```bash
git add crates/paladin-core CHANGELOG.md
git commit -m "feat(core): keymap with per-platform default chords"
```
<!-- END_TASK_5 -->

---

## Phase 2 verification

- `cargo test -p paladin-core` prints `48 passed; 0 failed`.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` are clean.
- `git push` and `gh run watch --exit-status` show CI green on both runners (the core tests run on Windows too; `resolve_uses_the_platform_config_directory` checks for `AppData/Roaming/Paladin` there).
