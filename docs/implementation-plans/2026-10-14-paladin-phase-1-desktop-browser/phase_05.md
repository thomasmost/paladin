# Paladin Phase 1 Desktop Browser Implementation Plan

**Goal:** A desktop browser for macOS and Windows that renders pages with Servo under an egui chrome, with multiple windows and tabs, a settings window, a logo, installable packages, and CI on both platforms.

**Architecture:** Three-crate Cargo workspace plus an `xtask` helper. `paladin-core` holds the Servo-free model, named commands, keymap, settings, and profile paths. `paladin-engine` is the only crate that imports Servo and exposes `Engine`, `GlSurface`, and `TabHost`. The `paladin` binary runs the winit event loop and draws the chrome with egui, blitting Servo's offscreen framebuffer under the toolbar. The UI only emits `Command`s; the pure `dispatch` and `apply` functions in core turn commands and engine events into state changes plus `EngineAction`s.

**Tech Stack:** Rust 1.97.1 (edition 2024), servo 0.7.0, winit 0.30.13, egui / egui-winit / egui_glow 0.34.3, glow 0.17.0, surfman 0.14.0, keyboard-types 0.8.3, directories 6, toml 1.1, clap 4, resvg 0.48.1, icns 0.5, ico 0.6, cargo-packager 0.11.8, GitHub Actions on `macos-15` and `windows-2025`.

**Scope:** 6 phases from original design (this file is phase 5 of 6)

**Codebase verified:** 2026-10-14. After Phase 4, `crates/paladin/src/` holds `main.rs`, `app.rs`, `window.rs`, `chrome.rs`, `input.rs`, `icon.rs`, and `keyutils.rs`; `App` already keeps a registry of windows keyed by winit window id and handles `OpenWindow`/`CloseWindow`/`Quit`, and `paladin_core::Keymap` exists from Phase 2. This phase adds the settings window and wires the keymap and the window menu in. The five replaced files are the Phase 4 files plus those additions; all were compiled, linted, and tested with the pinned toolchain.

Deviations from the design's sketch, each deliberate:

- The key-routing rule lives in a pure function, `input::route_key`, with unit tests, rather than inline in the event handler: it is the contract Phase 2's remappable hotkeys build on.
- The page only receives repeats and releases of keys whose press it received (`page_keys`, from Phase 4), so a chord that fired a command never leaks its repeats or release to the page: holding Cmd+T opens one tab, and a key still held while focus moved to another window (Cmd+N, Cmd+Shift+W) does not arrive there as a lone release.
- Cmd+L reselects the address bar text even when the bar already has focus, so the shortcut always means "replace the address".
- Settings are saved when an edited field loses focus, when a checkbox toggles, and when the window closes with an uncommitted edit, rather than on every keystroke; this is still "saving on change" without rewriting the file per character.
- Cmd+Q goes through the keymap because the app disables winit's default macOS menu (Phase 4); `ApplicationHandler::exiting` also tears everything down for the Dock's Quit, saving an uncommitted settings edit first.
- Bound chords that use Cmd or Ctrl fire even while the address bar has focus (none of the defaults clash with egui's text-editing keys, which are Cmd+A, C, V, X, Z); otherwise there would be no way to press Cmd+T or Cmd+Q after Cmd+L.
- The settings window also routes keys through the keymap: Quit and New window run as commands, Close window or Close tab close it, so Cmd+Q works whichever window has focus.

---

## Conventions for every task in every phase

- Run every command from the repository root, `/Users/thomas/vibes/paladin`.
- Commit to the current branch (`main`) with Conventional Commits. No feature branches.
- "Create" means the file does not exist yet; write exactly the content shown. "Replace" means overwrite the whole file with the content shown. Code blocks are complete files unless a step says otherwise.
- Before every commit run `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`. All three must be clean.
- Manual checks in this phase are run on the development Mac with `cargo run`.

---

## Phase 5: Multiple windows, settings window, keyboard shortcuts

**Goal:** The rest of the Phase 1 chrome, driven by the keymap.

**Done when:** Two windows render independently; settings edits persist across restarts; every `Command` is reachable by its default chord; closing the last window exits cleanly with no Servo shutdown errors. The manual checklist is extended and passes on macOS.

<!-- START_SUBCOMPONENT_A (tasks 1-1) -->
<!-- START_TASK_1 -->
### Task 1: Settings window, keymap lookup, and window menu

**Files:**
- Create: `crates/paladin/src/settings_window.rs`
- Replace: `crates/paladin/src/input.rs`
- Replace: `crates/paladin/src/main.rs`
- Replace: `crates/paladin/src/app.rs`
- Replace: `crates/paladin/src/window.rs`
- Replace: `crates/paladin/src/chrome.rs`

What changes in the replaced files, so the diff is reviewable:

- `input.rs`: adds `KeyRoute` and `route_key` (egui focus first, then the keymap on an initial press, then the page) with tests.
- `main.rs`: declares `mod settings_window`, builds `Keymap::defaults(Platform::current())`, and passes it to `App::new`.
- `app.rs`: holds `keymap` and `settings_window: Option<SettingsWindow>`; `OpenSettings` opens or focuses the settings window; `Quit` saves an uncommitted settings edit and drops the window; events for the settings window's winit id go to it, and `Changed` or `Close { save: true }` saves `settings.toml` into the profile (`App::save_settings`); browser windows receive the keymap.
- `window.rs`: `handle_event` and `frame` take the keymap; key events go through `route_key`, and a hit becomes a `Command` for this window and its active tab. The command route removes the key from `page_keys`, so the chord's repeats and release never reach the page; a release of a key the page is holding goes to the page whoever has focus.
- `chrome.rs`: a "☰" menu button with New window, Settings, and Quit; tooltips show the bound shortcut (`hint`, `describe_chord`, with a test).

**Step 0: Guard against losing Phase 4 fixes**

If Phase 4's checklist forced changes to `app.rs`, `window.rs`, `chrome.rs`, `input.rs`, or `main.rs` beyond what Phase 4 listed, replacing the files would undo them. Run:

```bash
git log --oneline -- crates/paladin/src
```

If any commit after `feat(app): browser window with tab strip, toolbar, and Servo content` touched those files, note what it changed (`git show <sha> -- crates/paladin/src`). After Step 6 below, re-apply each such change to the replaced files before building.

**Step 1: Create the settings window**

`crates/paladin/src/settings_window.rs`:

```rust
//! The settings window: a winit window with egui only (no tabs). It edits
//! `Settings` in place and reports when they should be saved. Phase 2 adds
//! the hotkey and profile editors here.

use egui_glow::EguiGlow;
use paladin_core::{CommandKind, Keymap, Settings};
use paladin_engine::{Engine, EngineError, GlSurface, PhysicalSize};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use crate::icon;
use crate::input::{self, KeyRoute};

pub enum SettingsOutcome {
    Nothing,
    /// An edited field was committed (focus left it, or a checkbox
    /// toggled); the caller should persist the settings.
    Changed,
    /// The user closed the window; the caller should drop it, saving first
    /// if `save` (an edit was still uncommitted).
    Close {
        save: bool,
    },
    /// A keymap chord fired while this window had focus. The caller
    /// handles app-level kinds (quit, new window); window kinds close this
    /// window.
    Shortcut(CommandKind),
}

pub struct SettingsWindow {
    /// A text field was edited and has not lost focus yet.
    dirty: bool,
    modifiers: ModifiersState,
    egui: EguiGlow,
    surface: GlSurface,
    // Dropped last, after the GL state that depends on it.
    window: Window,
}

/// What one frame of the form did.
#[derive(Default)]
struct FormActivity {
    /// Some text changed under the cursor.
    edited: bool,
    /// A field lost focus or a checkbox toggled: time to save.
    committed: bool,
}

impl SettingsWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        _engine: &Engine,
    ) -> Result<SettingsWindow, EngineError> {
        let attributes = Window::default_attributes()
            .with_title("Paladin Settings")
            .with_inner_size(LogicalSize::new(480.0, 320.0))
            .with_min_inner_size(LogicalSize::new(320.0, 200.0))
            .with_window_icon(icon::window_icon());
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| EngineError(format!("creating settings window: {e}")))?;
        let size = window.inner_size();
        let size = PhysicalSize::new(size.width.max(1), size.height.max(1));
        let display_handle = event_loop
            .display_handle()
            .map_err(|e| EngineError(format!("display handle: {e}")))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| EngineError(format!("window handle: {e}")))?;
        let surface = GlSurface::new(display_handle, window_handle, size)?;
        surface.make_current()?;
        let egui = EguiGlow::new(event_loop, surface.gl(), None, None, false);
        Ok(SettingsWindow {
            dirty: false,
            modifiers: ModifiersState::default(),
            egui,
            surface,
            window,
        })
    }

    pub fn winit_id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    pub fn focus(&self) {
        self.window.focus_window();
    }

    /// True if an edit has not been saved yet.
    pub fn needs_save(&self) -> bool {
        self.dirty
    }

    pub fn handle_event(
        &mut self,
        event: &WindowEvent,
        settings: &mut Settings,
        keymap: &Keymap,
    ) -> SettingsOutcome {
        match event {
            WindowEvent::CloseRequested => {
                return SettingsOutcome::Close { save: self.dirty };
            }
            WindowEvent::Resized(size) => {
                self.surface
                    .resize(PhysicalSize::new(size.width.max(1), size.height.max(1)));
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let activity = self.frame(settings);
                if activity.edited {
                    self.dirty = true;
                }
                return if activity.committed && self.dirty {
                    self.dirty = false;
                    SettingsOutcome::Changed
                } else {
                    SettingsOutcome::Nothing
                };
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                let key_input = crate::keyutils::key_input_from_winit(key, self.modifiers);
                let route = input::route_key(
                    self.egui.egui_ctx.egui_wants_keyboard_input(),
                    key.state == ElementState::Pressed,
                    key.repeat,
                    key_input.modifiers,
                    key_input.code,
                    keymap,
                );
                if let KeyRoute::Command(kind) = route {
                    return SettingsOutcome::Shortcut(kind);
                }
            }
            _ => {}
        }
        let response = self.egui.on_window_event(&self.window, event);
        if response.repaint {
            self.window.request_redraw();
        }
        SettingsOutcome::Nothing
    }

    fn frame(&mut self, settings: &mut Settings) -> FormActivity {
        let mut activity = FormActivity::default();
        self.egui.run(&self.window, |ui| {
            activity = draw_settings(ui, settings);
        });
        self.surface.begin_frame();
        self.egui.paint(&self.window);
        self.surface.present();
        if self.egui.egui_ctx.has_requested_repaint() {
            self.window.request_redraw();
        }
        activity
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        let _ = self.surface.make_current();
        self.egui.destroy();
    }
}

/// The settings form.
fn draw_settings(ui: &mut egui::Ui, settings: &mut Settings) -> FormActivity {
    let mut activity = FormActivity::default();
    ui.heading("Settings");
    ui.add_space(8.0);
    egui::Grid::new("settings-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Homepage");
            let homepage =
                ui.add(egui::TextEdit::singleline(&mut settings.homepage).desired_width(300.0));
            activity.edited |= homepage.changed();
            activity.committed |= homepage.lost_focus();
            ui.end_row();

            ui.label("New tab URL");
            let new_tab =
                ui.add(egui::TextEdit::singleline(&mut settings.new_tab_url).desired_width(300.0));
            activity.edited |= new_tab.changed();
            activity.committed |= new_tab.lost_focus();
            ui.end_row();

            ui.label("On launch");
            let restore = ui.checkbox(
                &mut settings.restore_windows_on_launch,
                "Restore previous windows (not yet implemented)",
            );
            if restore.changed() {
                activity.edited = true;
                activity.committed = true;
            }
            ui.end_row();
        });
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new("Keyboard shortcuts and profiles arrive in Phase 2.")
            .small()
            .weak(),
    );
    activity
}
```

**Step 2: Replace input.rs**

`crates/paladin/src/input.rs`:

```rust
//! winit input to engine input, and the key-routing rule. Coordinates are
//! converted by the window (toolbar offset, physical pixels); this module
//! maps the enums and decides where a key goes.

use keyboard_types::{Code, Modifiers};
use paladin_core::{CommandKind, KeyChord, Keymap};
use paladin_engine::{ButtonAction, EngineInput, MouseButton};
use winit::event::{ElementState, Ime, MouseScrollDelta};

/// Pixels per wheel "line", the value Servo's own shell uses.
const LINE_HEIGHT_PX: f32 = 76.0;

pub fn mouse_button(button: winit::event::MouseButton) -> MouseButton {
    match button {
        winit::event::MouseButton::Left => MouseButton::Primary,
        winit::event::MouseButton::Right => MouseButton::Secondary,
        winit::event::MouseButton::Middle => MouseButton::Auxiliary,
        winit::event::MouseButton::Back => MouseButton::Back,
        winit::event::MouseButton::Forward => MouseButton::Forward,
        winit::event::MouseButton::Other(n) => MouseButton::Other(n),
    }
}

pub fn button_action(state: ElementState) -> ButtonAction {
    match state {
        ElementState::Pressed => ButtonAction::Down,
        ElementState::Released => ButtonAction::Up,
    }
}

/// Wheel delta in pixels, `(dx, dy)`.
pub fn wheel_delta(delta: MouseScrollDelta) -> (f64, f64) {
    match delta {
        MouseScrollDelta::LineDelta(dx, dy) => {
            ((dx * LINE_HEIGHT_PX) as f64, (dy * LINE_HEIGHT_PX) as f64)
        }
        MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
    }
}

pub fn ime(ime: &Ime) -> Option<EngineInput> {
    match ime {
        Ime::Enabled => Some(EngineInput::ImeStart),
        Ime::Preedit(text, _) => Some(EngineInput::ImePreedit(text.clone())),
        Ime::Commit(text) => Some(EngineInput::ImeCommit(text.clone())),
        Ime::Disabled => Some(EngineInput::ImeDismissed),
    }
}

/// Where a key event goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRoute {
    /// An egui widget (the address bar) has keyboard focus.
    Egui,
    /// The initial press of a bound chord.
    Command(CommandKind),
    /// The page in the active tab.
    Page,
}

/// The routing rule: a bound chord with Cmd or Ctrl held fires its command
/// on the initial press even while an egui field (the address bar) has
/// focus, so shortcuts keep working there; otherwise egui takes every key
/// while it has focus; otherwise bound chords on their initial press
/// (auto-repeats and releases never trigger commands); otherwise the page.
pub fn route_key(
    egui_wants_keyboard: bool,
    pressed: bool,
    repeat: bool,
    modifiers: Modifiers,
    code: Code,
    keymap: &Keymap,
) -> KeyRoute {
    let with_primary = modifiers.intersects(Modifiers::CONTROL | Modifiers::META);
    let bound = if pressed && !repeat {
        keymap.resolve(KeyChord::new(modifiers, code))
    } else {
        None
    };
    match (egui_wants_keyboard, bound) {
        (true, Some(kind)) if with_primary => KeyRoute::Command(kind),
        (true, _) => KeyRoute::Egui,
        (false, Some(kind)) => KeyRoute::Command(kind),
        (false, None) => KeyRoute::Page,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paladin_core::Platform;

    #[test]
    fn line_deltas_scale_to_pixels_and_pixel_deltas_pass_through() {
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, -1.0)),
            (0.0, -76.0)
        );
        assert_eq!(
            wheel_delta(MouseScrollDelta::PixelDelta(
                winit::dpi::PhysicalPosition::new(3.0, 4.0)
            )),
            (3.0, 4.0)
        );
    }

    #[test]
    fn buttons_map_one_to_one() {
        assert_eq!(
            mouse_button(winit::event::MouseButton::Left),
            MouseButton::Primary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Right),
            MouseButton::Secondary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Middle),
            MouseButton::Auxiliary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Other(7)),
            MouseButton::Other(7)
        );
        assert_eq!(button_action(ElementState::Pressed), ButtonAction::Down);
    }

    fn mac() -> Keymap {
        Keymap::defaults(Platform::MacOs)
    }

    #[test]
    fn egui_focus_takes_unbound_keys_but_primary_chords_still_fire() {
        let r = route_key(true, true, false, Modifiers::empty(), Code::KeyA, &mac());
        assert_eq!(r, KeyRoute::Egui, "typing goes to the address bar");
        let r = route_key(true, true, false, Modifiers::META, Code::KeyA, &mac());
        assert_eq!(r, KeyRoute::Egui, "Cmd+A is unbound: egui selects all");
        let r = route_key(true, true, false, Modifiers::META, Code::KeyT, &mac());
        assert_eq!(
            r,
            KeyRoute::Command(CommandKind::NewTab),
            "Cmd+T works while the address bar has focus"
        );
        let r = route_key(true, false, false, Modifiers::META, Code::KeyT, &mac());
        assert_eq!(r, KeyRoute::Egui, "the release goes to egui");
    }

    #[test]
    fn bound_chord_on_initial_press_is_a_command() {
        let r = route_key(false, true, false, Modifiers::META, Code::KeyT, &mac());
        assert_eq!(r, KeyRoute::Command(CommandKind::NewTab));
    }

    #[test]
    fn repeats_and_releases_of_chords_never_fire_commands() {
        let r = route_key(false, true, true, Modifiers::META, Code::KeyT, &mac());
        assert_eq!(r, KeyRoute::Page);
        let r = route_key(false, false, false, Modifiers::META, Code::KeyT, &mac());
        assert_eq!(r, KeyRoute::Page);
    }

    #[test]
    fn unbound_keys_reach_the_page() {
        let r = route_key(false, true, false, Modifiers::empty(), Code::KeyA, &mac());
        assert_eq!(r, KeyRoute::Page);
        let r = route_key(false, true, false, Modifiers::empty(), Code::Escape, &mac());
        assert_eq!(r, KeyRoute::Page, "Escape belongs to the page");
        let r = route_key(false, true, false, Modifiers::META, Code::ArrowLeft, &mac());
        assert_eq!(r, KeyRoute::Page, "Cmd+Left edits text in fields");
    }
}
```

**Step 3: Replace main.rs**

`crates/paladin/src/main.rs`:

```rust
//! Paladin: a desktop browser on the Servo engine with an egui chrome.

mod app;
mod chrome;
mod icon;
mod input;
mod keyutils;
mod settings_window;
mod window;

use std::process::ExitCode;

use clap::Parser;
use paladin_core::command::open_window;
use paladin_core::profile::DEFAULT_PROFILE_NAME;
use paladin_core::{Browser, Command, Keymap, Platform, Profile, Settings, dispatch};
use url::Url;
use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::{App, AppEvent};

#[derive(Parser, Debug)]
#[command(name = "paladin", version, about = "Paladin desktop browser")]
struct Cli {
    /// Profile name; state lives under the per-user Paladin directory.
    #[arg(long, default_value = DEFAULT_PROFILE_NAME)]
    profile: String,
    /// URLs to open, one tab each. With none, the homepage opens.
    urls: Vec<String>,
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();

    let profile = match Profile::resolve(&cli.profile) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("paladin: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = profile.ensure_dirs() {
        eprintln!(
            "paladin: cannot create profile directory {}: {e}",
            profile.dir.display()
        );
        return ExitCode::FAILURE;
    }
    let settings = match Settings::load(&profile.settings_path()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("paladin: {e} (in {})", profile.settings_path().display());
            return ExitCode::FAILURE;
        }
    };
    log::info!("profile {:?} at {}", profile.name, profile.dir.display());

    let mut urls = Vec::new();
    for arg in &cli.urls {
        match chrome::location_input_to_url(arg) {
            Some(url) => urls.push(url),
            None => {
                eprintln!("paladin: not a URL: {arg:?}");
                return ExitCode::FAILURE;
            }
        }
    }

    let mut browser = Browser::new(profile, settings);
    let keymap = Keymap::defaults(Platform::current());
    let first: Url = urls.first().cloned().unwrap_or_else(|| browser.homepage());
    let mut actions = open_window(&mut browser, first);
    let window = browser.windows[0].id;
    for url in urls.into_iter().skip(1) {
        actions.extend(dispatch(
            &mut browser,
            &Command::NewTab {
                window,
                url: Some(url),
            },
        ));
    }

    let mut builder = EventLoop::<AppEvent>::with_user_event();
    // winit's default macOS menu owns Cmd+Q: AppKit runs `terminate:`
    // before the window sees the key. Without the menu, the keymap's Quit
    // command runs the normal shutdown instead.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_default_menu(false);
    }
    let event_loop = match builder.build() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("paladin: cannot create event loop: {e}");
            return ExitCode::FAILURE;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(browser, keymap, actions, &event_loop);
    match event_loop.run_app(&mut app) {
        Ok(()) if app.started_cleanly() => ExitCode::SUCCESS,
        Ok(()) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("paladin: event loop error: {e}");
            ExitCode::FAILURE
        }
    }
}
```

**Step 4: Replace app.rs**

`crates/paladin/src/app.rs`:

```rust
//! The winit application: owns the engine, the browser model, and the
//! native windows, and runs the dispatch loop.
//!
//! One iteration, for every winit event:
//! 1. The window the event belongs to handles it: chrome events go to egui,
//!    content events go to the active tab through the engine, key chords
//!    are resolved through the keymap. This yields `Command`s.
//! 2. `dispatch` turns each `Command` into model changes plus
//!    `EngineAction`s, which `perform` forwards to the engine or handles.
//! 3. The engine spins; its `EngineEvent`s are drained into `apply`, whose
//!    follow-up actions are performed too.
//! 4. Windows that were touched redraw; the redraw paints the active tab
//!    offscreen and egui blits it below the toolbar.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use paladin_core::{
    Browser, Command, CommandKind, EngineAction, EngineEvent, Keymap, TabId, WindowId, apply,
    dispatch,
};
use paladin_engine::{Engine, Waker};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};

use crate::icon;
use crate::settings_window::{SettingsOutcome, SettingsWindow};
use crate::window::BrowserWindow;

/// The only user event: the engine asked us to spin its event loop.
#[derive(Debug)]
pub enum AppEvent {
    Wake,
}

struct WinitWaker(Mutex<EventLoopProxy<AppEvent>>);

impl Waker for WinitWaker {
    fn wake(&self) {
        let proxy = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(e) = proxy.send_event(AppEvent::Wake) {
            log::warn!("failed to wake the event loop: {e}");
        }
    }
}

pub struct App {
    browser: Browser,
    keymap: Keymap,
    // Windows are declared before `engine` so they drop first: every tab
    // host and GL surface must be gone before the engine shuts down.
    windows: HashMap<winit::window::WindowId, BrowserWindow>,
    settings_window: Option<SettingsWindow>,
    pending_actions: Vec<EngineAction>,
    engine: Option<Engine>,
    waker: Arc<WinitWaker>,
    exiting: bool,
    engine_failed: bool,
}

impl App {
    /// `initial_actions` are the actions from building the initial model in
    /// `main` (opening the first window and its tabs). They run once the
    /// event loop is active and the engine exists.
    pub fn new(
        browser: Browser,
        keymap: Keymap,
        initial_actions: Vec<EngineAction>,
        event_loop: &EventLoop<AppEvent>,
    ) -> App {
        App {
            browser,
            keymap,
            windows: HashMap::new(),
            settings_window: None,
            pending_actions: initial_actions,
            engine: None,
            waker: Arc::new(WinitWaker(Mutex::new(event_loop.create_proxy()))),
            exiting: false,
            engine_failed: false,
        }
    }

    /// False if the engine could not be started (the app exits at once).
    pub fn started_cleanly(&self) -> bool {
        !self.engine_failed
    }

    fn window_for_tab(&mut self, tab: TabId) -> Option<&mut BrowserWindow> {
        self.windows.values_mut().find(|w| w.hosts_tab(tab))
    }

    fn window_for_core_id(&mut self, id: WindowId) -> Option<&mut BrowserWindow> {
        self.windows.values_mut().find(|w| w.core_id() == id)
    }

    fn run_commands(&mut self, commands: Vec<Command>, event_loop: &ActiveEventLoop) {
        for command in commands {
            let actions = dispatch(&mut self.browser, &command);
            self.perform_all(actions, event_loop);
        }
    }

    fn perform_all(&mut self, actions: Vec<EngineAction>, event_loop: &ActiveEventLoop) {
        for action in actions {
            if self.exiting {
                return;
            }
            self.perform(action, event_loop);
        }
    }

    fn perform(&mut self, action: EngineAction, event_loop: &ActiveEventLoop) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        match action {
            EngineAction::OpenWindow { window, tab, url } => {
                match BrowserWindow::open(event_loop, engine, window) {
                    Ok(mut bw) => {
                        bw.host_mut().create_tab(tab, url);
                        bw.host().set_focused(tab, true);
                        self.windows.insert(bw.winit_id(), bw);
                    }
                    Err(e) => {
                        // Keep the model honest: the window never existed.
                        // If it was the last one, this quits instead of
                        // waiting forever with nothing on screen.
                        eprintln!(
                            "paladin: cannot open a window (profile {:?} at {}): {e}",
                            self.browser.profile.name,
                            self.browser.profile.dir.display()
                        );
                        let actions = dispatch(&mut self.browser, &Command::CloseWindow(window));
                        self.perform_all(actions, event_loop);
                    }
                }
            }
            EngineAction::CloseWindow(window) => {
                self.windows.retain(|_, w| w.core_id() != window);
            }
            EngineAction::CreateTab { window, tab, url } => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.host_mut().create_tab(tab, url);
                    w.host().set_focused(tab, true);
                    w.request_redraw();
                }
            }
            EngineAction::AdoptTab {
                window,
                tab,
                pending,
            } => {
                let window_exists = match self.window_for_core_id(window) {
                    Some(w) => {
                        if w.host_mut().adopt_tab(pending, tab) {
                            w.host().set_focused(tab, true);
                        } else {
                            log::warn!("engine had no pending tab {pending:?} to adopt");
                        }
                        w.request_redraw();
                        true
                    }
                    None => false,
                };
                if !window_exists && let Some(engine) = self.engine.as_ref() {
                    // The opener's window closed meanwhile; let the engine
                    // drop the tab it created.
                    engine.discard_pending(pending);
                }
            }
            EngineAction::Navigate { tab, url } => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().navigate(tab, url);
                }
            }
            EngineAction::Back(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().back(tab);
                }
            }
            EngineAction::Forward(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().forward(tab);
                }
            }
            EngineAction::Reload(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().reload(tab);
                }
            }
            EngineAction::Stop(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().stop(tab);
                }
            }
            EngineAction::Show(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().show(tab);
                    w.host().set_focused(tab, true);
                    w.request_redraw();
                }
            }
            EngineAction::Hide(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host().set_focused(tab, false);
                    w.host().hide(tab);
                }
            }
            EngineAction::CloseTab(tab) => {
                if let Some(w) = self.window_for_tab(tab) {
                    w.host_mut().close_tab(tab);
                    w.request_redraw();
                }
            }
            EngineAction::FocusAddressBar(window) => {
                if let Some(w) = self.window_for_core_id(window) {
                    w.focus_address_bar();
                }
            }
            EngineAction::OpenSettings => match &self.settings_window {
                Some(sw) => sw.focus(),
                None => match SettingsWindow::open(event_loop, engine) {
                    Ok(sw) => self.settings_window = Some(sw),
                    Err(e) => log::error!("cannot open settings window: {e}"),
                },
            },
            EngineAction::Quit => {
                self.exiting = true;
                self.close_settings_window();
                self.windows.clear();
                event_loop.exit();
            }
        }
    }

    /// Spin the engine and fold its events into the model.
    fn pump(&mut self, event_loop: &ActiveEventLoop) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        engine.spin();
        for event in engine.drain_events() {
            if let Some(tab) = tab_of(&event)
                && let Some(w) = self.window_for_tab(tab)
            {
                w.request_redraw();
            }
            if let EngineEvent::EngineError(message) = &event {
                log::error!("engine error: {message}");
                for w in self.windows.values() {
                    w.request_redraw();
                }
            }
            let actions = apply(&mut self.browser, &event);
            if let EngineEvent::NewTabRequested { pending, .. } = &event
                && actions.is_empty()
                && let Some(engine) = self.engine.as_ref()
            {
                // The opener tab is gone from the model; nothing will adopt
                // the new tab, so let the engine drop it.
                engine.discard_pending(*pending);
            }
            self.perform_all(actions, event_loop);
            if self.exiting {
                return;
            }
        }
    }

    fn save_settings(&self) {
        let path = self.browser.profile.settings_path();
        if let Err(e) = self.browser.settings.save(&path) {
            log::error!("cannot save settings to {}: {e}", path.display());
        }
    }

    /// Drop the settings window, saving an uncommitted edit first.
    fn close_settings_window(&mut self) {
        if self
            .settings_window
            .as_ref()
            .is_some_and(|sw| sw.needs_save())
        {
            self.save_settings();
        }
        self.settings_window = None;
    }

    /// A keymap chord pressed while the settings window had focus.
    fn settings_shortcut(&mut self, kind: CommandKind, event_loop: &ActiveEventLoop) {
        match kind {
            CommandKind::Quit => self.run_commands(vec![Command::Quit], event_loop),
            CommandKind::NewWindow => self.run_commands(vec![Command::NewWindow], event_loop),
            CommandKind::CloseWindow | CommandKind::CloseTab => self.close_settings_window(),
            CommandKind::OpenSettings => {
                if let Some(sw) = &self.settings_window {
                    sw.focus();
                }
            }
            // Tab and page commands need a browser window; ignore them here.
            _ => {}
        }
    }
}

fn tab_of(event: &EngineEvent) -> Option<TabId> {
    match event {
        EngineEvent::UrlChanged { tab, .. }
        | EngineEvent::TitleChanged { tab, .. }
        | EngineEvent::LoadStateChanged { tab, .. }
        | EngineEvent::HistoryChanged { tab, .. }
        | EngineEvent::StatusChanged { tab, .. }
        | EngineEvent::FaviconChanged { tab, .. }
        | EngineEvent::TabCrashed { tab, .. }
        | EngineEvent::FrameReady(tab)
        | EngineEvent::TabClosed(tab) => Some(*tab),
        EngineEvent::NewTabRequested { opener, .. } => Some(*opener),
        EngineEvent::EngineError(_) => None,
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.engine.is_none() && !self.exiting {
            let servo_dir = self.browser.profile.servo_dir();
            match Engine::new(&servo_dir, self.waker.clone()) {
                Ok(engine) => self.engine = Some(engine),
                Err(e) => {
                    eprintln!(
                        "paladin: the browser engine failed to start for profile {:?} \
                         (directory {}): {e}",
                        self.browser.profile.name,
                        self.browser.profile.dir.display()
                    );
                    self.engine_failed = true;
                    self.exiting = true;
                    event_loop.exit();
                    return;
                }
            }
            icon::set_dock_icon();
            let actions = std::mem::take(&mut self.pending_actions);
            self.perform_all(actions, event_loop);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if self.exiting {
            return;
        }
        if self
            .settings_window
            .as_ref()
            .is_some_and(|sw| sw.winit_id() == window_id)
        {
            let sw = self.settings_window.as_mut().expect("checked above");
            match sw.handle_event(&event, &mut self.browser.settings, &self.keymap) {
                SettingsOutcome::Nothing => {}
                SettingsOutcome::Changed => self.save_settings(),
                SettingsOutcome::Close { save } => {
                    if save {
                        self.save_settings();
                    }
                    self.settings_window = None;
                }
                SettingsOutcome::Shortcut(kind) => self.settings_shortcut(kind, event_loop),
            }
        } else if let Some(window) = self.windows.get_mut(&window_id) {
            let commands = window.handle_event(&self.browser, &self.keymap, &event);
            self.run_commands(commands, event_loop);
        }
        self.pump(event_loop);
        if !self.exiting {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Wake => self.pump(event_loop),
        }
        if !self.exiting {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    /// Runs on every exit path, including macOS `terminate:` from the Dock
    /// menu, which otherwise ends the process without dropping `App`. Tear
    /// down in dependency order: windows (tab hosts, GL) before the engine.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.exiting = true;
        self.close_settings_window();
        self.windows.clear();
        self.engine = None;
    }
}
```

**Step 5: Replace window.rs**

`crates/paladin/src/window.rs`:

```rust
//! A browser window: a winit window, the engine's tab host for it, and an
//! egui context sharing the engine's GL context.

use std::collections::HashSet;
use std::sync::Arc;

use egui::{LayerId, PaintCallback};
use egui_glow::{CallbackFn, EguiGlow};
use keyboard_types::Code;
use paladin_core::{Browser, Command, Keymap, WindowId};
use paladin_engine::{
    BlitRect, Engine, EngineError, EngineInput, GlSurface, PhysicalSize, TabHost,
};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use crate::chrome::{self, ChromeState};
use crate::icon;
use crate::input::{self, KeyRoute};

/// Who receives pointer events: the chrome (egui) or the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PointerTarget {
    Egui,
    Page,
}

pub struct BrowserWindow {
    id: WindowId,
    chrome: ChromeState,
    modifiers: ModifiersState,
    cursor: Option<PhysicalPosition<f64>>,
    /// Physical-pixel height of the chrome above the page, from the last
    /// frame. Events below it go to the page, above it to egui.
    content_top_px: f32,
    /// True while egui draws the page area itself (the crash panel).
    content_is_egui: bool,
    pointer_in_content: bool,
    /// Set on a button press, cleared on release: the side that took the
    /// press gets every pointer event until then, so a drag that crosses
    /// the toolbar edge still ends where it started.
    pointer_capture: Option<PointerTarget>,
    /// Keys whose press went to the page. Only their repeats and releases
    /// go to the page too, so it never sees half a key stroke, not from a
    /// chord that fired a command and not from a key pressed in another
    /// window before focus moved here.
    page_keys: HashSet<Code>,
    egui: EguiGlow,
    host: TabHost,
    // Dropped last: the tab host and egui hold GL state tied to it.
    window: Window,
}

impl BrowserWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        engine: &Engine,
        id: WindowId,
    ) -> Result<BrowserWindow, EngineError> {
        let attributes = Window::default_attributes()
            .with_title("Paladin")
            .with_inner_size(LogicalSize::new(1024.0, 768.0))
            .with_min_inner_size(LogicalSize::new(400.0, 300.0))
            .with_window_icon(icon::window_icon());
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| EngineError(format!("creating window: {e}")))?;

        let size = non_zero(window.inner_size());
        let display_handle = event_loop
            .display_handle()
            .map_err(|e| EngineError(format!("display handle: {e}")))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| EngineError(format!("window handle: {e}")))?;
        let surface = GlSurface::new(display_handle, window_handle, size)?;
        surface.make_current()?;
        let host = TabHost::new(engine, surface, window.scale_factor() as f32)?;
        let gl = host.surface().expect("headed tab host has a surface").gl();
        let egui = EguiGlow::new(event_loop, gl, None, None, false);
        egui.egui_ctx.options_mut(|options| {
            options.zoom_with_keyboard = false;
        });

        Ok(BrowserWindow {
            id,
            chrome: ChromeState::default(),
            modifiers: ModifiersState::default(),
            cursor: None,
            content_top_px: 0.0,
            content_is_egui: false,
            pointer_in_content: false,
            pointer_capture: None,
            page_keys: HashSet::new(),
            egui,
            host,
            window,
        })
    }

    pub fn core_id(&self) -> WindowId {
        self.id
    }

    pub fn winit_id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    pub fn host(&self) -> &TabHost {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut TabHost {
        &mut self.host
    }

    pub fn hosts_tab(&self, tab: paladin_core::TabId) -> bool {
        self.host.has_tab(tab)
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn focus_address_bar(&mut self) {
        self.chrome.focus_address = true;
        self.window.request_redraw();
    }

    fn surface(&self) -> &GlSurface {
        self.host.surface().expect("headed tab host has a surface")
    }

    /// Who the pointer belongs to right now: a captured side during a
    /// drag; otherwise egui when the pointer is over the chrome, over an
    /// interactable egui layer (menus, the error overlay), while a popup is
    /// open (so a click outside closes it), or while the crash panel
    /// replaces the page; otherwise the page.
    fn pointer_target(&self) -> PointerTarget {
        if let Some(captured) = self.pointer_capture {
            return captured;
        }
        let over_chrome = match self.cursor {
            Some(p) => (p.y as f32) < self.content_top_px,
            None => true,
        };
        let egui_owns = over_chrome
            || self.content_is_egui
            || self.egui.egui_ctx.is_pointer_over_egui()
            || egui::Popup::is_any_open(&self.egui.egui_ctx);
        if egui_owns {
            PointerTarget::Egui
        } else {
            PointerTarget::Page
        }
    }

    /// Cursor position in physical pixels relative to the page area.
    fn content_point(&self) -> Option<(f32, f32)> {
        let p = self.cursor?;
        Some((p.x as f32, p.y as f32 - self.content_top_px))
    }

    /// Handle one winit event. Returns the commands the user issued.
    pub fn handle_event(
        &mut self,
        browser: &Browser,
        keymap: &Keymap,
        event: &WindowEvent,
    ) -> Vec<Command> {
        let active = browser.window(self.id).map(|w| w.active);
        let mut commands = Vec::new();
        let mut forward_to_egui = true;

        match event {
            WindowEvent::Resized(size) => {
                self.surface().resize(non_zero(*size));
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.host.set_hidpi(*scale_factor as f32);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                return self.frame(browser, keymap);
            }
            WindowEvent::CloseRequested => {
                return vec![Command::CloseWindow(self.id)];
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    // Releases of keys held while focus moved away go to
                    // the other window; do not wait for them here.
                    self.page_keys.clear();
                    self.pointer_capture = None;
                }
                if let Some(tab) = active {
                    self.host.set_focused(tab, *focused);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                // egui always sees cursor movement so it can hide tooltips
                // and hover state when the pointer leaves the chrome.
                self.cursor = Some(*position);
                if self.pointer_target() == PointerTarget::Page {
                    if let (Some(tab), Some((x, y))) = (active, self.content_point()) {
                        self.pointer_in_content = true;
                        self.host.input(tab, EngineInput::MouseMove { x, y });
                    }
                } else if self.pointer_in_content {
                    self.pointer_in_content = false;
                    if let Some(tab) = active {
                        self.host.input(tab, EngineInput::MouseLeft);
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.pointer_in_content {
                    self.pointer_in_content = false;
                    if let Some(tab) = active {
                        self.host.input(tab, EngineInput::MouseLeft);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let target = self.pointer_target();
                self.pointer_capture = match state {
                    ElementState::Pressed => Some(target),
                    ElementState::Released => None,
                };
                if target == PointerTarget::Page {
                    forward_to_egui = false;
                    self.surrender_egui_focus();
                    if let (Some(tab), Some((x, y))) = (active, self.content_point()) {
                        self.host.input(
                            tab,
                            EngineInput::MouseButton {
                                action: input::button_action(*state),
                                button: input::mouse_button(*button),
                                x,
                                y,
                            },
                        );
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.pointer_target() == PointerTarget::Page {
                    forward_to_egui = false;
                    if let (Some(tab), Some((x, y))) = (active, self.content_point()) {
                        let (dx, dy) = input::wheel_delta(*delta);
                        self.host.input(tab, EngineInput::Wheel { dx, dy, x, y });
                    }
                }
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                let key_input = crate::keyutils::key_input_from_winit(key, self.modifiers);
                let pressed = key.state == ElementState::Pressed;
                // A release of a key the page is holding goes to the page
                // whoever has focus now, so it never sees half a stroke.
                if !pressed && self.page_keys.remove(&key_input.code) {
                    forward_to_egui = false;
                    if let Some(tab) = active {
                        self.host.input(tab, EngineInput::Key(key_input));
                    }
                } else {
                    let route = input::route_key(
                        self.egui.egui_ctx.egui_wants_keyboard_input(),
                        pressed,
                        key.repeat,
                        key_input.modifiers,
                        key_input.code,
                        keymap,
                    );
                    match route {
                        KeyRoute::Egui => {}
                        KeyRoute::Command(kind) => {
                            forward_to_egui = false;
                            self.page_keys.remove(&key_input.code);
                            if let Some(tab) = active {
                                commands.push(kind.into_command(self.id, tab));
                            }
                        }
                        KeyRoute::Page => {
                            forward_to_egui = false;
                            // Releases were handled above; a repeat is
                            // delivered only while the page holds the key.
                            let deliver = if !pressed {
                                false
                            } else if key.repeat {
                                self.page_keys.contains(&key_input.code)
                            } else {
                                self.page_keys.insert(key_input.code);
                                true
                            };
                            if deliver && let Some(tab) = active {
                                self.host.input(tab, EngineInput::Key(key_input));
                            }
                        }
                    }
                }
            }
            WindowEvent::Ime(ime) if !self.egui.egui_ctx.egui_wants_keyboard_input() => {
                forward_to_egui = false;
                if let (Some(tab), Some(input)) = (active, input::ime(ime)) {
                    self.host.input(tab, input);
                }
            }
            _ => {}
        }

        if forward_to_egui {
            let response = self.egui.on_window_event(&self.window, event);
            if response.repaint {
                self.window.request_redraw();
            }
        }
        commands
    }

    fn surrender_egui_focus(&self) {
        self.egui.egui_ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    /// Draw the chrome, paint the active tab offscreen, blit it under the
    /// toolbar, and present.
    fn frame(&mut self, browser: &Browser, keymap: &Keymap) -> Vec<Command> {
        let Some(model) = browser.window(self.id) else {
            return Vec::new();
        };
        let active = model.active;
        let crashed = model.active_tab().crashed.clone();
        let engine_error = browser.engine_error.as_deref();
        let mut commands = Vec::new();
        let mut content_top_px = 0.0;

        let Self {
            egui,
            host,
            chrome: chrome_state,
            window,
            ..
        } = self;
        egui.run(window, |ui| {
            commands = chrome::draw(ui, model, engine_error, chrome_state, keymap);
            let rect = ui.available_rect_before_wrap();
            let ppp = ui.pixels_per_point();
            content_top_px = rect.min.y * ppp;
            let size = PhysicalSize::new(
                (rect.width() * ppp).round().max(1.0) as u32,
                (rect.height() * ppp).round().max(1.0) as u32,
            );
            host.resize_content(active, size);
            match &crashed {
                Some(reason) => {
                    if chrome::draw_crash_panel(ui, rect, reason) {
                        commands.push(Command::Reload(active));
                    }
                }
                None => {
                    host.paint(active);
                    // Fetched after resize and paint: the callback captures
                    // the offscreen framebuffer, which a resize recreates.
                    if let Some(blit) = host.blit_callback() {
                        ui.ctx()
                            .layer_painter(LayerId::background())
                            .add(PaintCallback {
                                rect,
                                callback: Arc::new(CallbackFn::new(move |info, painter| {
                                    let v = info.viewport_in_pixels();
                                    blit(
                                        painter.gl(),
                                        BlitRect {
                                            x: v.left_px,
                                            y_from_bottom: v.from_bottom_px,
                                            width: v.width_px,
                                            height: v.height_px,
                                        },
                                    );
                                })),
                            });
                    }
                }
            }
        });
        self.content_top_px = content_top_px;
        self.content_is_egui = crashed.is_some();

        let surface = self.host.surface().expect("headed tab host has a surface");
        surface.begin_frame();
        self.egui.paint(&self.window);
        surface.present();
        if self.egui.egui_ctx.has_requested_repaint() {
            self.window.request_redraw();
        }
        commands
    }
}

impl Drop for BrowserWindow {
    fn drop(&mut self) {
        if let Some(surface) = self.host.surface() {
            let _ = surface.make_current();
        }
        self.egui.destroy();
    }
}

/// GL surfaces reject zero-sized targets; a minimized window reports 0x0.
fn non_zero(size: winit::dpi::PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}
```

**Step 6: Replace chrome.rs**

`crates/paladin/src/chrome.rs`:

```rust
//! The window chrome: tab strip, toolbar, status overlay, and crash panel.
//! Pure functions of the model that return the commands the user issued.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{Button, ColorImage, Id, Key, Panel, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use keyboard_types::Code;
use paladin_core::{Command, CommandKind, Favicon, KeyChord, Keymap, LoadState, TabId, Window};
use url::{ParseError, Url};

const TAB_TITLE_MAX_CHARS: usize = 24;
const FAVICON_SIZE: f32 = 16.0;

/// Per-window chrome state that is not part of the browser model: the
/// address bar text the user may be editing, and favicon textures.
#[derive(Default)]
pub struct ChromeState {
    pub address_text: String,
    /// True while the user has edited the address bar since the last
    /// navigation; engine URL changes then leave the text alone.
    pub address_dirty: bool,
    /// Set by `Command::FocusAddressBar`; consumed by the next frame.
    pub focus_address: bool,
    shown_tab: Option<TabId>,
    shown_url: Option<Url>,
    favicons: HashMap<TabId, (u64, TextureHandle)>,
}

/// Draw the chrome for `window` and return the commands it produced.
/// `engine_error` is the browser-wide engine error to show in the status
/// area, if any.
pub fn draw(
    ui: &mut Ui,
    window: &Window,
    engine_error: Option<&str>,
    state: &mut ChromeState,
    keymap: &Keymap,
) -> Vec<Command> {
    let mut commands = Vec::new();
    let active = window.active_tab();
    sync_address_text(state, active.id, &active.url);
    state
        .favicons
        .retain(|tab, _| window.index_of(*tab).is_some());

    Panel::top("tabs").show_inside(ui, |ui| {
        egui::ScrollArea::horizontal()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for tab in &window.tabs {
                        let is_active = tab.id == window.active;
                        if let Some(texture) =
                            favicon_texture(ui, state, tab.id, tab.favicon.as_ref())
                        {
                            ui.add(egui::Image::new((texture.id(), Vec2::splat(FAVICON_SIZE))));
                        }
                        let title = truncate(tab.display_title(), TAB_TITLE_MAX_CHARS);
                        let response = ui.add(Button::selectable(is_active, title));
                        if response.clicked() && !is_active {
                            commands.push(Command::ActivateTab(tab.id));
                        }
                        if response.middle_clicked() {
                            commands.push(Command::CloseTab(tab.id));
                        }
                        if ui.small_button("×").on_hover_text("Close tab").clicked() {
                            commands.push(Command::CloseTab(tab.id));
                        }
                        ui.add_space(4.0);
                    }
                    if ui
                        .button("+")
                        .on_hover_text(hint(keymap, CommandKind::NewTab, "New tab"))
                        .clicked()
                    {
                        commands.push(Command::NewTab {
                            window: window.id,
                            url: None,
                        });
                    }
                    ui.menu_button("☰", |ui| {
                        if ui
                            .button(hint(keymap, CommandKind::NewWindow, "New window"))
                            .clicked()
                        {
                            commands.push(Command::NewWindow);
                            ui.close();
                        }
                        if ui
                            .button(hint(keymap, CommandKind::OpenSettings, "Settings"))
                            .clicked()
                        {
                            commands.push(Command::OpenSettings);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(hint(keymap, CommandKind::Quit, "Quit")).clicked() {
                            commands.push(Command::Quit);
                            ui.close();
                        }
                    });
                });
            });
    });

    Panel::top("toolbar").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(active.can_go_back, Button::new("◀"))
                .on_hover_text(hint(keymap, CommandKind::Back, "Back"))
                .clicked()
            {
                commands.push(Command::Back(active.id));
            }
            if ui
                .add_enabled(active.can_go_forward, Button::new("▶"))
                .on_hover_text(hint(keymap, CommandKind::Forward, "Forward"))
                .clicked()
            {
                commands.push(Command::Forward(active.id));
            }
            // Reload is always available (a hung load must be recoverable);
            // Stop appears beside it while loading.
            if ui
                .button("⟳")
                .on_hover_text(hint(keymap, CommandKind::Reload, "Reload"))
                .clicked()
            {
                state.address_dirty = false;
                commands.push(Command::Reload(active.id));
            }
            // Servo 0.7.0 cannot stop a load; the button shows the state but
            // stays disabled so the UI does not promise what the engine lacks.
            if active.load == LoadState::Loading {
                ui.add_enabled(false, Button::new("✕"))
                    .on_disabled_hover_text(format!(
                        "{} (not supported by Servo 0.7.0)",
                        hint(keymap, CommandKind::Stop, "Stop")
                    ));
            }

            let address_id = Id::new(("address", window.id));
            let field = ui.add_sized(
                ui.available_size(),
                egui::TextEdit::singleline(&mut state.address_text)
                    .id(address_id)
                    .hint_text("Enter address"),
            );
            if field.changed() {
                state.address_dirty = true;
            }
            // Select the text on focus, and again on Cmd+L while already
            // focused, so the shortcut always means "replace the address".
            let focus_requested = state.focus_address;
            if state.focus_address {
                state.focus_address = false;
                field.request_focus();
            }
            if field.gained_focus() || focus_requested {
                select_all(ui, address_id, state.address_text.len());
            }
            if field.lost_focus()
                && ui.input(|i| i.key_pressed(Key::Enter))
                && let Some(url) = location_input_to_url(&state.address_text)
            {
                state.address_dirty = false;
                commands.push(Command::Navigate {
                    tab: active.id,
                    url,
                });
            }
        });
    });

    // The status text floats over the bottom-left corner of the page as an
    // overlay, so showing or hiding it never resizes the page.
    let status = match engine_error {
        Some(error) => Some(format!("Engine error: {error}")),
        None if !active.status.is_empty() => Some(active.status.clone()),
        None => None,
    };
    if let Some(text) = status {
        egui::Area::new(Id::new(("status", window.id)))
            .anchor(egui::Align2::LEFT_BOTTOM, Vec2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(engine_error.is_some())
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(text).small());
                        if engine_error.is_some()
                            && ui.small_button("×").on_hover_text("Dismiss").clicked()
                        {
                            commands.push(Command::DismissEngineError);
                        }
                    });
                });
            });
    }

    commands
}

/// Replace the page area with an error panel. Returns true if the user
/// asked to reload.
pub fn draw_crash_panel(ui: &mut Ui, rect: Rect, reason: &str) -> bool {
    let mut reload = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("This tab crashed");
            ui.label(reason);
            ui.add_space(12.0);
            if ui.button("Reload").clicked() {
                reload = true;
            }
        });
    });
    reload
}

/// Turn address-bar text into a URL. Bare domains get `https://`,
/// `localhost` and `host:port` get `http://`. Returns `None` for text that
/// is not a URL (Phase 1 has no search engine).
pub fn location_input_to_url(input: &str) -> Option<Url> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    match Url::parse(text) {
        // "localhost:8080" and "example.com:8080" parse as scheme + path.
        Ok(url)
            if url.host_str().is_none()
                && (url.scheme() == "localhost" || url.scheme().contains('.')) =>
        {
            Url::parse(&format!("http://{text}")).ok()
        }
        Ok(url) => Some(url),
        Err(ParseError::RelativeUrlWithoutBase) if looks_like_host(text) => {
            let scheme = if text.starts_with("localhost") {
                "http"
            } else {
                "https"
            };
            Url::parse(&format!("{scheme}://{text}")).ok()
        }
        Err(_) => None,
    }
}

fn looks_like_host(text: &str) -> bool {
    !text.contains(char::is_whitespace)
        && !text.starts_with('.')
        && (text == "localhost" || text.starts_with("localhost:") || text.contains('.'))
}

fn sync_address_text(state: &mut ChromeState, tab: TabId, url: &Url) {
    let tab_changed = state.shown_tab != Some(tab);
    let url_changed = state.shown_url.as_ref() != Some(url);
    if tab_changed || (url_changed && !state.address_dirty) {
        state.address_text = url.to_string();
        state.address_dirty = false;
    }
    state.shown_tab = Some(tab);
    state.shown_url = Some(url.clone());
}

fn select_all(ui: &Ui, id: Id, len: usize) {
    use egui::text::{CCursor, CCursorRange};
    use egui::text_edit::TextEditState;
    if let Some(mut edit) = TextEditState::load(ui.ctx(), id) {
        edit.cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
        edit.store(ui.ctx(), id);
    }
}

fn favicon_texture<'a>(
    ui: &Ui,
    state: &'a mut ChromeState,
    tab: TabId,
    favicon: Option<&Favicon>,
) -> Option<&'a TextureHandle> {
    let favicon = favicon?;
    let key = fingerprint(favicon);
    let fresh = state
        .favicons
        .get(&tab)
        .is_some_and(|(existing, _)| *existing == key);
    if !fresh {
        let image = ColorImage::from_rgba_unmultiplied(
            [favicon.width as usize, favicon.height as usize],
            &favicon.rgba,
        );
        let texture =
            ui.ctx()
                .load_texture(format!("favicon-{}", tab.0), image, TextureOptions::LINEAR);
        state.favicons.insert(tab, (key, texture));
    }
    state.favicons.get(&tab).map(|(_, t)| t)
}

fn fingerprint(favicon: &Favicon) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    favicon.width.hash(&mut hasher);
    favicon.height.hash(&mut hasher);
    favicon.rgba.hash(&mut hasher);
    hasher.finish()
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// A label with its shortcut, like "New tab (Cmd+T)".
fn hint(keymap: &Keymap, kind: CommandKind, label: &str) -> String {
    match keymap.chord_for(kind) {
        Some(chord) => format!("{label} ({})", describe_chord(chord)),
        None => label.to_string(),
    }
}

/// A readable name for a chord: modifiers in Ctrl, Alt, Shift, Cmd order,
/// then the key.
fn describe_chord(chord: KeyChord) -> String {
    use keyboard_types::Modifiers;
    let mut parts: Vec<String> = Vec::new();
    if chord.modifiers.contains(Modifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if chord.modifiers.contains(Modifiers::ALT) {
        parts.push(
            if cfg!(target_os = "macos") {
                "Option"
            } else {
                "Alt"
            }
            .to_string(),
        );
    }
    if chord.modifiers.contains(Modifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    if chord.modifiers.contains(Modifiers::META) {
        parts.push(
            if cfg!(target_os = "macos") {
                "Cmd"
            } else {
                "Win"
            }
            .to_string(),
        );
    }
    parts.push(key_name(chord.code));
    parts.join("+")
}

fn key_name(code: Code) -> String {
    let name = match code {
        Code::BracketLeft => "[",
        Code::BracketRight => "]",
        Code::Comma => ",",
        Code::Period => ".",
        Code::ArrowLeft => "Left",
        Code::ArrowRight => "Right",
        Code::ArrowUp => "Up",
        Code::ArrowDown => "Down",
        Code::Escape => "Esc",
        other => {
            let debug = format!("{other:?}");
            return debug
                .strip_prefix("Key")
                .or_else(|| debug.strip_prefix("Digit"))
                .unwrap_or(&debug)
                .to_string();
        }
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyboard_types::Modifiers;

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(
            location_input_to_url("https://example.com/a?b=c")
                .unwrap()
                .as_str(),
            "https://example.com/a?b=c"
        );
        assert_eq!(
            location_input_to_url("about:blank").unwrap().as_str(),
            "about:blank"
        );
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(
            location_input_to_url("example.com").unwrap().as_str(),
            "https://example.com/"
        );
        assert_eq!(
            location_input_to_url("  www.example.com/path ")
                .unwrap()
                .as_str(),
            "https://www.example.com/path"
        );
    }

    #[test]
    fn localhost_and_host_port_get_http() {
        assert_eq!(
            location_input_to_url("localhost").unwrap().as_str(),
            "http://localhost/"
        );
        assert_eq!(
            location_input_to_url("localhost:8080").unwrap().as_str(),
            "http://localhost:8080/"
        );
        assert_eq!(
            location_input_to_url("example.com:8080").unwrap().as_str(),
            "http://example.com:8080/"
        );
    }

    #[test]
    fn non_urls_are_rejected() {
        assert_eq!(location_input_to_url(""), None);
        assert_eq!(location_input_to_url("hello world"), None);
        assert_eq!(location_input_to_url("justaword"), None);
        assert_eq!(location_input_to_url(".hidden"), None);
    }

    #[test]
    fn truncate_adds_ellipsis_only_when_cut() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a longer title here", 8), "a longer…");
    }

    #[test]
    fn chord_labels_are_readable() {
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::CONTROL, Code::KeyT)),
            "Ctrl+T"
        );
        assert_eq!(
            describe_chord(KeyChord::new(
                Modifiers::CONTROL | Modifiers::SHIFT,
                Code::BracketRight
            )),
            "Ctrl+Shift+]"
        );
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::CONTROL, Code::Comma)),
            "Ctrl+,"
        );
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::CONTROL, Code::Digit1)),
            "Ctrl+1"
        );
        assert_eq!(
            describe_chord(KeyChord::new(Modifiers::empty(), Code::F5)),
            "F5"
        );
    }
}
```

Now re-apply any Phase 4 fixes noted in Step 0, then run `git diff -- crates/paladin/src` and confirm every hunk is either one of the additions listed above or a carried-forward fix.

**Step 7: Build, lint, and test**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo test -p paladin`
Expected: clippy `Finished` with no warnings; `13 passed`: the Phase 4 tests plus `input::tests::{egui_focus_takes_unbound_keys_but_primary_chords_still_fire, bound_chord_on_initial_press_is_a_command, repeats_and_releases_of_chords_never_fire_commands, unbound_keys_reach_the_page}` and `chrome::tests::chord_labels_are_readable`.

**Step 8: Check the behaviour by hand**

Run: `cargo run`, then:

- Press Cmd+N: a second window opens at `about:blank`. Load a different page in each; both render and scroll independently. Close one; the other keeps working.
- In a window press Cmd+T (new tab), Cmd+Shift+] and Cmd+Shift+[ (next and previous tab, wrapping), Cmd+W (close tab), Cmd+L (the address bar gains focus with its text selected), type `example.com`, Enter, Cmd+R (reload), navigate again then Cmd+[ and Cmd+] (back and forward), Cmd+. while a page loads (Stop; a no-op in Servo 0.7.0, so nothing visible happens), Cmd+Shift+W (close window). Each shortcut performs its action and the key does not reach the page: with a page text field focused, hold Cmd+T briefly and release; exactly one tab opens and nothing is typed into the field.
- Keys pages need still reach them: click into a page text field, type text, press Escape (nothing in the chrome reacts), and Cmd+Left/Cmd+Right (the caret moves in the field; the page does not navigate).
- Press Cmd+L, then Cmd+T while the address bar still has focus: a new tab opens (shortcuts work in the address bar), and typing a letter afterwards goes into the address bar, not the page.
- Open the "☰" menu with the mouse: it lists New window, Settings, and Quit with their shortcuts in the labels (`Cmd+N`, `Cmd+,`, `Cmd+Q`), and its lower items hang over the page area. Click New window with the mouse: a window opens and the menu closes. Open the menu again and click on the page outside it: the menu closes and the click does not reach the page. Choose Settings with the mouse (or press Cmd+,): the "Paladin Settings" window opens.
- With the settings window focused, press Cmd+W: it closes. Open it again and press Cmd+Q: the whole app quits cleanly. Run `cargo run` again. Change the homepage to `https://example.com/` and press Tab (the field loses focus and the settings are saved), close the settings window, quit with Cmd+Q, and run `cargo run` again: the first tab opens at `https://example.com/`.
- Run: `cat ~/Library/Application\ Support/Paladin/profiles/default/settings.toml`
  Expected: `homepage = "https://example.com/"` among the three keys.
- Open Settings again, edit the new-tab URL, and close the settings window without leaving the field: the file is updated on close.
- Close the last browser window with its close button: the process exits with exit code 0 and no Servo error output. Repeat with the Dock's Quit menu item: same result.

Then set the homepage back to `https://servo.org/` and the new-tab URL back to `about:blank` through the settings window (or delete the `settings.toml` file).

**Step 9: Commit**

```bash
git add crates/paladin
git commit -m "feat(app): settings window, keyboard shortcuts through the keymap, and a window menu"
```
<!-- END_TASK_1 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_2 -->
### Task 2: Extend the manual checklist and the changelog

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`

**Step 1: Extend the checklist**

In `README.md`, append these items to the numbered list in `## Manual checklist` (after item 7):

```markdown
8. **Windows and menu.** Cmd+N (Ctrl+N on Windows), or New window chosen
   with the mouse from the "☰" menu, opens a second window; each window's
   tabs, address bar, and page are independent, and closing one leaves the
   other working. Clicking on the page outside an open menu closes the menu
   without the click reaching the page.
9. **Shortcuts.** With Cmd on macOS or Ctrl on Windows: T new tab, W close
   tab, Shift+W close window, N new window, L focus the address bar with
   its text selected, R reload, period stop, Shift+] and Shift+[ next and
   previous tab (wrapping), comma opens settings, Q quits. Back and forward
   are Cmd+[ and Cmd+] on macOS and Alt+Left and Alt+Right on Windows. A
   chord held down fires once, and none of its key strokes reach the page.
   Shortcuts also work while the address bar has focus. Escape and
   Cmd+Left/Right are not shortcuts and do reach the page.
10. **Settings.** Settings (Cmd+, or chosen with the mouse from the menu)
    opens a window with Homepage, New tab URL, and the restore checkbox.
    Edits are saved when a field loses focus or the window closes;
    relaunching opens the new homepage, and `settings.toml` in the profile
    directory holds it. With the settings window focused, Cmd+W closes it
    and Cmd+Q quits.
11. **Clean exit.** Closing the last window, Cmd+Q from a browser window or
    from the settings window, and Quit from the Dock menu (macOS) all exit
    with code 0 and no Servo error output.
```

**Step 2: Record the change**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, add after the last bullet:

```markdown
- Multiple windows, a settings window that persists the homepage, new-tab
  URL, and restore option to the profile, keyboard shortcuts for every
  command with macOS and Windows defaults, and a window menu.
```

**Step 3: Run the full checklist**

Run: `cargo run`
Expected: items 1 to 11 pass.

**Step 4: Commit and push**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: checklist items for windows, shortcuts, and settings"
git push
```

Run: `gh run watch --exit-status`
Expected: CI green on both runners.
<!-- END_TASK_2 -->

---

## Phase 5 verification

- All 11 manual checklist items pass on the Mac.
- `cargo test --workspace`, clippy, and fmt are clean; CI is green.
- `paladin_core::keymap::tests::every_command_kind_has_a_default_chord_on_both_platforms` (from Phase 2) is the machine check that every command is reachable by a chord, and `paladin::input::tests` pin the routing rule; item 9 is the human check.
