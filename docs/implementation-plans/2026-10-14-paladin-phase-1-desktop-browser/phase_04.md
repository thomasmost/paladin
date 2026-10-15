# Paladin Phase 1 Desktop Browser Implementation Plan

**Goal:** A desktop browser for macOS and Windows that renders pages with Servo under an egui chrome, with multiple windows and tabs, a settings window, a logo, installable packages, and CI on both platforms.

**Architecture:** Three-crate Cargo workspace plus an `xtask` helper. `paladin-core` holds the Servo-free model, named commands, keymap, settings, and profile paths. `paladin-engine` is the only crate that imports Servo and exposes `Engine`, `GlSurface`, and `TabHost`. The `paladin` binary runs the winit event loop and draws the chrome with egui, blitting Servo's offscreen framebuffer under the toolbar. The UI only emits `Command`s; the pure `dispatch` and `apply` functions in core turn commands and engine events into state changes plus `EngineAction`s.

**Tech Stack:** Rust 1.97.1 (edition 2024), servo 0.7.0, winit 0.30.13, egui / egui-winit / egui_glow 0.34.3, glow 0.17.0, surfman 0.14.0, keyboard-types 0.8.3, directories 6, toml 1.1, clap 4, resvg 0.48.1, icns 0.5, ico 0.6, cargo-packager 0.11.8, GitHub Actions on `macos-15` and `windows-2025`.

**Scope:** 6 phases from original design (this file is phase 4 of 6)

**Codebase verified:** 2026-10-14. After Phase 3, `crates/paladin` is the Phase 1 placeholder (a `Cargo.toml` depending on the two library crates and a `main.rs` that prints the version). The engine API this phase calls (`Engine::new`, `Engine::discard_pending`, `GlSurface`, `TabHost`, `EngineInput`) is exactly what Phase 3 built. Every file below was compiled against winit 0.30.13 and egui 0.34.3 with the pinned toolchain, clippy clean, and its unit tests pass; the window and app modules were compiled against the same engine API surface. Key facts verified in the egui 0.34.3 and winit 0.30.13 sources:

- `EguiGlow::new(event_loop, gl, None, None, false)` and `EguiGlow::run(window, |ui: &mut Ui| ..)` (the closure receives a `Ui`, not a `Context`); `Panel::top(..).show_inside(ui, ..)` is the 0.34 panel API.
- `egui_glow::CallbackFn::new(|info, painter| ..)` with `info.viewport_in_pixels()` gives `left_px`, `from_bottom_px`, `width_px`, `height_px`: GL coordinates for the blit.
- `Context::wants_keyboard_input` is deprecated in 0.34; `egui_wants_keyboard_input` is the name.
- winit's `set_window_icon` is a no-op on macOS, so the Dock icon is set through AppKit (`objc2-app-kit`) after the event loop exists.
- winit's default macOS menu owns Cmd+Q and runs AppKit's `terminate:` before any window sees the key; winit then dispatches `ApplicationHandler::exiting` but never drops the handler. The app disables that menu and tears itself down in `exiting`.
- The `png` 0.18 decoder needs a `Seek` reader; the embedded icon bytes are wrapped in a `Cursor`.
- `Context::is_pointer_over_egui` and `egui::Popup::is_any_open` read egui memory and are valid between frames (where winit events are handled); `Context::any_popup_open` reads per-pass state that egui documents as valid only during a frame, so the routing uses the former two.

Deviations from the design's sketch of this crate, each deliberate:

- The chrome is one module, `chrome.rs`, rather than a `chrome/` directory; the tab strip and toolbar together are under 400 lines.
- `keyutils.rs` is added: Servo's own winit-to-`keyboard-types` table (MPL-2.0, header kept), because winit and Servo name keys differently and the mapping is 500 lines nobody should retype.
- The status area is an overlay anchored at the bottom-left of the page, not a panel: a panel would resize the page every time a link is hovered.
- `Tab::crashed` (Phase 2) drives the crash panel the design asks for; `Browser::engine_error` drives the engine error in the status area, with a dismiss button.
- Reload stays available while a page loads, because Servo 0.7.0 cannot stop a load and a hung page must be recoverable; a disabled Stop button appears beside it whose tooltip says so.
- Pointer routing is not just "above or below the toolbar": a click goes to egui when the pointer is over any interactable egui layer (the error overlay's dismiss button, a menu), while a popup is open (so clicking outside closes it), or while the crash panel replaces the page. Otherwise egui could never receive clicks on controls drawn over the page area. A button press also captures the pointer for its side until the release, so a drag that crosses the toolbar edge ends where it started.
- The page only receives repeats and releases of keys whose press it received (`page_keys`), so a key pressed in another window before focus moved never reaches it as a lone release.

---

## Conventions for every task in every phase

- Run every command from the repository root, `/Users/thomas/vibes/paladin`.
- Commit to the current branch (`main`) with Conventional Commits. No feature branches.
- "Create" means the file does not exist yet; write exactly the content shown. "Replace" means overwrite the whole file with the content shown. Code blocks are complete files unless a step says otherwise.
- Before every commit run `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`. All three must be clean.
- Manual checks in this phase are run on the development Mac with `cargo run`. Record anything that does not pass; the phase is not done until the checklist passes.

---

## Phase 4: Single browser window

**Goal:** `cargo run` opens a window with the logo, tabs, and a toolbar, and renders web pages.

**Done when:** On macOS, `cargo run` shows the Paladin icon, a URL typed in the address bar loads and renders, back/forward/reload work, tabs open and close, resizing and HiDPI changes keep the page aligned under the toolbar. The manual checklist in `README.md` is written and passes.

How the pieces fit (the dispatch loop from the design, as implemented in `app.rs`):

1. `BrowserWindow::handle_event` routes each winit event: resize and scale changes go to the GL surface and tab host; pointer events over the chrome go to egui, pointer events below it go to the active tab as `EngineInput` with the toolbar height subtracted; key events go to egui when it has focus (the address bar), otherwise to the tab. `RedrawRequested` runs a frame. The method returns the `Command`s the user issued.
2. `App::run_commands` calls `paladin_core::dispatch` and performs the returned `EngineAction`s (`App::perform`): tab host calls, opening and closing native windows, quitting. If a native window cannot be opened, the model's window is closed again so the app never waits with nothing on screen.
3. `App::pump` spins the engine, drains `EngineEvent`s into `paladin_core::apply`, performs any follow-up actions, and requests a redraw of the window that hosts the event's tab.
4. A frame (`BrowserWindow::frame`) runs egui: the chrome draws from the model and returns commands; the remaining rect becomes the page area, the tab host resizes the offscreen target to it, paints the active tab, and registers a background paint callback that blits the offscreen framebuffer into that rect; then egui paints and the surface presents.

In this phase, `OpenSettings` only logs, keyboard shortcuts are not yet resolved (keys go to the page), and the tab strip has no window menu. Phase 5 adds those.

<!-- START_SUBCOMPONENT_A (tasks 1-1) -->
<!-- START_TASK_1 -->
### Task 1: The binary crate

All eight files are created together because `main.rs` declares the modules and nothing compiles until they all exist.

**Files:**
- Replace: `crates/paladin/Cargo.toml`
- Create: `crates/paladin/src/keyutils.rs`
- Create: `crates/paladin/src/input.rs`
- Create: `crates/paladin/src/icon.rs`
- Create: `crates/paladin/src/chrome.rs`
- Create: `crates/paladin/src/window.rs`
- Create: `crates/paladin/src/app.rs`
- Replace: `crates/paladin/src/main.rs`

**Step 1: Replace the manifest**

`crates/paladin/Cargo.toml`:

```toml
[package]
name = "paladin"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
description = "Paladin desktop browser"

[[bin]]
name = "paladin"
path = "src/main.rs"

[dependencies]
paladin-core = { path = "../paladin-core" }
paladin-engine = { path = "../paladin-engine" }
clap = { version = "4", features = ["derive"] }
egui = { workspace = true }
egui-winit = { workspace = true }
egui_glow = { workspace = true }
env_logger = "0.11"
keyboard-types = { workspace = true }
log = { workspace = true }
png = "0.18"
url = { workspace = true }
winit = { workspace = true }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-app-kit = { version = "0.3", default-features = false, features = [
    "std",
    "NSApplication",
    "NSResponder",
    "NSImage",
    "NSRunningApplication",
] }
objc2-foundation = { version = "0.3", default-features = false, features = ["std", "NSData"] }
```

**Step 2: Create the winit-to-keyboard-types mapping**

This file is Servo's own `ports/servoshell/desktop/keyutils.rs` at tag `v0.7.0` (MPL-2.0; the license header stays) with its Servo-specific import replaced and the final function returning Paladin's `KeyInput`. It is a 1:1 table of winit key names to W3C `keyboard-types` names.

`crates/paladin/src/keyutils.rs`:

```rust
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! winit key events to `keyboard-types`. Copied from Servo's reference
//! shell (`ports/servoshell/desktop/keyutils.rs` at tag v0.7.0, MPL-2.0)
//! with the Servo-specific imports replaced; see the task that added it.

use keyboard_types::{Code, Key, KeyState, Location, Modifiers, NamedKey};
use paladin_engine::KeyInput;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{
    Key as WinitKey, KeyCode, KeyLocation as WinitKeyLocation, ModifiersState,
    NamedKey as WinitNamedKey, PhysicalKey,
};

trait FromWinitKeyEvent {
    fn from_winit_key_event(key_event: &KeyEvent) -> Self;
}

#[expect(deprecated)]
impl FromWinitKeyEvent for Key {
    fn from_winit_key_event(key_event: &KeyEvent) -> Self {
        let named_key = match key_event.logical_key {
            WinitKey::Named(named_key) => named_key,
            WinitKey::Character(ref string) => return Key::Character(string.to_string()),
            WinitKey::Unidentified(_) | WinitKey::Dead(_) => {
                return Key::Named(NamedKey::Unidentified);
            }
        };

        match named_key {
            WinitNamedKey::AVRInput => Key::Named(NamedKey::AVRInput),
            WinitNamedKey::AVRPower => Key::Named(NamedKey::AVRPower),
            WinitNamedKey::Accept => Key::Named(NamedKey::Accept),
            WinitNamedKey::Again => Key::Named(NamedKey::Again),
            WinitNamedKey::AllCandidates => Key::Named(NamedKey::AllCandidates),
            WinitNamedKey::Alphanumeric => Key::Named(NamedKey::Alphanumeric),
            WinitNamedKey::Alt => Key::Named(NamedKey::Alt),
            WinitNamedKey::AltGraph => Key::Named(NamedKey::AltGraph),
            WinitNamedKey::AppSwitch => Key::Named(NamedKey::AppSwitch),
            WinitNamedKey::ArrowDown => Key::Named(NamedKey::ArrowDown),
            WinitNamedKey::ArrowLeft => Key::Named(NamedKey::ArrowLeft),
            WinitNamedKey::ArrowRight => Key::Named(NamedKey::ArrowRight),
            WinitNamedKey::ArrowUp => Key::Named(NamedKey::ArrowUp),
            WinitNamedKey::Attn => Key::Named(NamedKey::Attn),
            WinitNamedKey::AudioBalanceLeft => Key::Named(NamedKey::AudioBalanceLeft),
            WinitNamedKey::AudioBalanceRight => Key::Named(NamedKey::AudioBalanceRight),
            WinitNamedKey::AudioBassBoostDown => Key::Named(NamedKey::AudioBassBoostDown),
            WinitNamedKey::AudioBassBoostToggle => Key::Named(NamedKey::AudioBassBoostToggle),
            WinitNamedKey::AudioBassBoostUp => Key::Named(NamedKey::AudioBassBoostUp),
            WinitNamedKey::AudioFaderFront => Key::Named(NamedKey::AudioFaderFront),
            WinitNamedKey::AudioFaderRear => Key::Named(NamedKey::AudioFaderRear),
            WinitNamedKey::AudioSurroundModeNext => Key::Named(NamedKey::AudioSurroundModeNext),
            WinitNamedKey::AudioTrebleDown => Key::Named(NamedKey::AudioTrebleDown),
            WinitNamedKey::AudioTrebleUp => Key::Named(NamedKey::AudioTrebleUp),
            WinitNamedKey::AudioVolumeDown => Key::Named(NamedKey::AudioVolumeDown),
            WinitNamedKey::AudioVolumeMute => Key::Named(NamedKey::AudioVolumeMute),
            WinitNamedKey::AudioVolumeUp => Key::Named(NamedKey::AudioVolumeUp),
            WinitNamedKey::Backspace => Key::Named(NamedKey::Backspace),
            WinitNamedKey::BrightnessDown => Key::Named(NamedKey::BrightnessDown),
            WinitNamedKey::BrightnessUp => Key::Named(NamedKey::BrightnessUp),
            WinitNamedKey::BrowserBack => Key::Named(NamedKey::BrowserBack),
            WinitNamedKey::BrowserFavorites => Key::Named(NamedKey::BrowserFavorites),
            WinitNamedKey::BrowserForward => Key::Named(NamedKey::BrowserForward),
            WinitNamedKey::BrowserHome => Key::Named(NamedKey::BrowserHome),
            WinitNamedKey::BrowserRefresh => Key::Named(NamedKey::BrowserRefresh),
            WinitNamedKey::BrowserSearch => Key::Named(NamedKey::BrowserSearch),
            WinitNamedKey::BrowserStop => Key::Named(NamedKey::BrowserStop),
            WinitNamedKey::Call => Key::Named(NamedKey::Call),
            WinitNamedKey::Camera => Key::Named(NamedKey::Camera),
            WinitNamedKey::CameraFocus => Key::Named(NamedKey::CameraFocus),
            WinitNamedKey::Cancel => Key::Named(NamedKey::Cancel),
            WinitNamedKey::CapsLock => Key::Named(NamedKey::CapsLock),
            WinitNamedKey::ChannelDown => Key::Named(NamedKey::ChannelDown),
            WinitNamedKey::ChannelUp => Key::Named(NamedKey::ChannelUp),
            WinitNamedKey::Clear => Key::Named(NamedKey::Clear),
            WinitNamedKey::Close => Key::Named(NamedKey::Close),
            WinitNamedKey::ClosedCaptionToggle => Key::Named(NamedKey::ClosedCaptionToggle),
            WinitNamedKey::CodeInput => Key::Named(NamedKey::CodeInput),
            WinitNamedKey::ColorF0Red => Key::Named(NamedKey::ColorF0Red),
            WinitNamedKey::ColorF1Green => Key::Named(NamedKey::ColorF1Green),
            WinitNamedKey::ColorF2Yellow => Key::Named(NamedKey::ColorF2Yellow),
            WinitNamedKey::ColorF3Blue => Key::Named(NamedKey::ColorF3Blue),
            WinitNamedKey::ColorF4Grey => Key::Named(NamedKey::ColorF4Grey),
            WinitNamedKey::ColorF5Brown => Key::Named(NamedKey::ColorF5Brown),
            WinitNamedKey::Compose => Key::Named(NamedKey::Compose),
            WinitNamedKey::ContextMenu => Key::Named(NamedKey::ContextMenu),
            WinitNamedKey::Control => Key::Named(NamedKey::Control),
            WinitNamedKey::Convert => Key::Named(NamedKey::Convert),
            WinitNamedKey::Copy => Key::Named(NamedKey::Copy),
            WinitNamedKey::CrSel => Key::Named(NamedKey::CrSel),
            WinitNamedKey::Cut => Key::Named(NamedKey::Cut),
            WinitNamedKey::DVR => Key::Named(NamedKey::DVR),
            WinitNamedKey::Delete => Key::Named(NamedKey::Delete),
            WinitNamedKey::Dimmer => Key::Named(NamedKey::Dimmer),
            WinitNamedKey::DisplaySwap => Key::Named(NamedKey::DisplaySwap),
            WinitNamedKey::Eisu => Key::Named(NamedKey::Eisu),
            WinitNamedKey::Eject => Key::Named(NamedKey::Eject),
            WinitNamedKey::End => Key::Named(NamedKey::End),
            WinitNamedKey::EndCall => Key::Named(NamedKey::EndCall),
            WinitNamedKey::Enter => Key::Named(NamedKey::Enter),
            WinitNamedKey::EraseEof => Key::Named(NamedKey::EraseEof),
            WinitNamedKey::Escape => Key::Named(NamedKey::Escape),
            WinitNamedKey::ExSel => Key::Named(NamedKey::ExSel),
            WinitNamedKey::Execute => Key::Named(NamedKey::Execute),
            WinitNamedKey::Exit => Key::Named(NamedKey::Exit),
            WinitNamedKey::F1 => Key::Named(NamedKey::F1),
            WinitNamedKey::F10 => Key::Named(NamedKey::F10),
            WinitNamedKey::F11 => Key::Named(NamedKey::F11),
            WinitNamedKey::F12 => Key::Named(NamedKey::F12),
            WinitNamedKey::F13 => Key::Named(NamedKey::F13),
            WinitNamedKey::F14 => Key::Named(NamedKey::F14),
            WinitNamedKey::F15 => Key::Named(NamedKey::F15),
            WinitNamedKey::F16 => Key::Named(NamedKey::F16),
            WinitNamedKey::F17 => Key::Named(NamedKey::F17),
            WinitNamedKey::F18 => Key::Named(NamedKey::F18),
            WinitNamedKey::F19 => Key::Named(NamedKey::F19),
            WinitNamedKey::F2 => Key::Named(NamedKey::F2),
            WinitNamedKey::F20 => Key::Named(NamedKey::F20),
            WinitNamedKey::F21 => Key::Named(NamedKey::F21),
            WinitNamedKey::F22 => Key::Named(NamedKey::F22),
            WinitNamedKey::F23 => Key::Named(NamedKey::F23),
            WinitNamedKey::F24 => Key::Named(NamedKey::F24),
            WinitNamedKey::F25 => Key::Named(NamedKey::F25),
            WinitNamedKey::F26 => Key::Named(NamedKey::F26),
            WinitNamedKey::F27 => Key::Named(NamedKey::F27),
            WinitNamedKey::F28 => Key::Named(NamedKey::F28),
            WinitNamedKey::F29 => Key::Named(NamedKey::F29),
            WinitNamedKey::F3 => Key::Named(NamedKey::F3),
            WinitNamedKey::F30 => Key::Named(NamedKey::F30),
            WinitNamedKey::F31 => Key::Named(NamedKey::F31),
            WinitNamedKey::F32 => Key::Named(NamedKey::F32),
            WinitNamedKey::F33 => Key::Named(NamedKey::F33),
            WinitNamedKey::F34 => Key::Named(NamedKey::F34),
            WinitNamedKey::F35 => Key::Named(NamedKey::F35),
            WinitNamedKey::F4 => Key::Named(NamedKey::F4),
            WinitNamedKey::F5 => Key::Named(NamedKey::F5),
            WinitNamedKey::F6 => Key::Named(NamedKey::F6),
            WinitNamedKey::F7 => Key::Named(NamedKey::F7),
            WinitNamedKey::F8 => Key::Named(NamedKey::F8),
            WinitNamedKey::F9 => Key::Named(NamedKey::F9),
            WinitNamedKey::FavoriteClear0 => Key::Named(NamedKey::FavoriteClear0),
            WinitNamedKey::FavoriteClear1 => Key::Named(NamedKey::FavoriteClear1),
            WinitNamedKey::FavoriteClear2 => Key::Named(NamedKey::FavoriteClear2),
            WinitNamedKey::FavoriteClear3 => Key::Named(NamedKey::FavoriteClear3),
            WinitNamedKey::FavoriteRecall0 => Key::Named(NamedKey::FavoriteRecall0),
            WinitNamedKey::FavoriteRecall1 => Key::Named(NamedKey::FavoriteRecall1),
            WinitNamedKey::FavoriteRecall2 => Key::Named(NamedKey::FavoriteRecall2),
            WinitNamedKey::FavoriteRecall3 => Key::Named(NamedKey::FavoriteRecall3),
            WinitNamedKey::FavoriteStore0 => Key::Named(NamedKey::FavoriteStore0),
            WinitNamedKey::FavoriteStore1 => Key::Named(NamedKey::FavoriteStore1),
            WinitNamedKey::FavoriteStore2 => Key::Named(NamedKey::FavoriteStore2),
            WinitNamedKey::FavoriteStore3 => Key::Named(NamedKey::FavoriteStore3),
            WinitNamedKey::FinalMode => Key::Named(NamedKey::FinalMode),
            WinitNamedKey::Find => Key::Named(NamedKey::Find),
            WinitNamedKey::Fn => Key::Named(NamedKey::Fn),
            WinitNamedKey::FnLock => Key::Named(NamedKey::FnLock),
            WinitNamedKey::GoBack => Key::Named(NamedKey::GoBack),
            WinitNamedKey::GoHome => Key::Named(NamedKey::GoHome),
            WinitNamedKey::GroupFirst => Key::Named(NamedKey::GroupFirst),
            WinitNamedKey::GroupLast => Key::Named(NamedKey::GroupLast),
            WinitNamedKey::GroupNext => Key::Named(NamedKey::GroupNext),
            WinitNamedKey::GroupPrevious => Key::Named(NamedKey::GroupPrevious),
            WinitNamedKey::Guide => Key::Named(NamedKey::Guide),
            WinitNamedKey::GuideNextDay => Key::Named(NamedKey::GuideNextDay),
            WinitNamedKey::GuidePreviousDay => Key::Named(NamedKey::GuidePreviousDay),
            WinitNamedKey::HangulMode => Key::Named(NamedKey::HangulMode),
            WinitNamedKey::HanjaMode => Key::Named(NamedKey::HanjaMode),
            WinitNamedKey::Hankaku => Key::Named(NamedKey::Hankaku),
            WinitNamedKey::HeadsetHook => Key::Named(NamedKey::HeadsetHook),
            WinitNamedKey::Help => Key::Named(NamedKey::Help),
            WinitNamedKey::Hibernate => Key::Named(NamedKey::Hibernate),
            WinitNamedKey::Hiragana => Key::Named(NamedKey::Hiragana),
            WinitNamedKey::HiraganaKatakana => Key::Named(NamedKey::HiraganaKatakana),
            WinitNamedKey::Home => Key::Named(NamedKey::Home),
            WinitNamedKey::Hyper => Key::Named(NamedKey::Hyper),
            WinitNamedKey::Info => Key::Named(NamedKey::Info),
            WinitNamedKey::Insert => Key::Named(NamedKey::Insert),
            WinitNamedKey::InstantReplay => Key::Named(NamedKey::InstantReplay),
            WinitNamedKey::JunjaMode => Key::Named(NamedKey::JunjaMode),
            WinitNamedKey::KanaMode => Key::Named(NamedKey::KanaMode),
            WinitNamedKey::KanjiMode => Key::Named(NamedKey::KanjiMode),
            WinitNamedKey::Katakana => Key::Named(NamedKey::Katakana),
            WinitNamedKey::Key11 => Key::Named(NamedKey::Key11),
            WinitNamedKey::Key12 => Key::Named(NamedKey::Key12),
            WinitNamedKey::LastNumberRedial => Key::Named(NamedKey::LastNumberRedial),
            WinitNamedKey::LaunchApplication1 => Key::Named(NamedKey::LaunchApplication1),
            WinitNamedKey::LaunchApplication2 => Key::Named(NamedKey::LaunchApplication2),
            WinitNamedKey::LaunchCalendar => Key::Named(NamedKey::LaunchCalendar),
            WinitNamedKey::LaunchContacts => Key::Named(NamedKey::LaunchContacts),
            WinitNamedKey::LaunchMail => Key::Named(NamedKey::LaunchMail),
            WinitNamedKey::LaunchMediaPlayer => Key::Named(NamedKey::LaunchMediaPlayer),
            WinitNamedKey::LaunchMusicPlayer => Key::Named(NamedKey::LaunchMusicPlayer),
            WinitNamedKey::LaunchPhone => Key::Named(NamedKey::LaunchPhone),
            WinitNamedKey::LaunchScreenSaver => Key::Named(NamedKey::LaunchScreenSaver),
            WinitNamedKey::LaunchSpreadsheet => Key::Named(NamedKey::LaunchSpreadsheet),
            WinitNamedKey::LaunchWebBrowser => Key::Named(NamedKey::LaunchWebBrowser),
            WinitNamedKey::LaunchWebCam => Key::Named(NamedKey::LaunchWebCam),
            WinitNamedKey::LaunchWordProcessor => Key::Named(NamedKey::LaunchWordProcessor),
            WinitNamedKey::Link => Key::Named(NamedKey::Link),
            WinitNamedKey::ListProgram => Key::Named(NamedKey::ListProgram),
            WinitNamedKey::LiveContent => Key::Named(NamedKey::LiveContent),
            WinitNamedKey::Lock => Key::Named(NamedKey::Lock),
            WinitNamedKey::LogOff => Key::Named(NamedKey::LogOff),
            WinitNamedKey::MailForward => Key::Named(NamedKey::MailForward),
            WinitNamedKey::MailReply => Key::Named(NamedKey::MailReply),
            WinitNamedKey::MailSend => Key::Named(NamedKey::MailSend),
            WinitNamedKey::MannerMode => Key::Named(NamedKey::MannerMode),
            WinitNamedKey::MediaApps => Key::Named(NamedKey::MediaApps),
            WinitNamedKey::MediaAudioTrack => Key::Named(NamedKey::MediaAudioTrack),
            WinitNamedKey::MediaClose => Key::Named(NamedKey::MediaClose),
            WinitNamedKey::MediaFastForward => Key::Named(NamedKey::MediaFastForward),
            WinitNamedKey::MediaLast => Key::Named(NamedKey::MediaLast),
            WinitNamedKey::MediaPause => Key::Named(NamedKey::MediaPause),
            WinitNamedKey::MediaPlay => Key::Named(NamedKey::MediaPlay),
            WinitNamedKey::MediaPlayPause => Key::Named(NamedKey::MediaPlayPause),
            WinitNamedKey::MediaRecord => Key::Named(NamedKey::MediaRecord),
            WinitNamedKey::MediaRewind => Key::Named(NamedKey::MediaRewind),
            WinitNamedKey::MediaSkipBackward => Key::Named(NamedKey::MediaSkipBackward),
            WinitNamedKey::MediaSkipForward => Key::Named(NamedKey::MediaSkipForward),
            WinitNamedKey::MediaStepBackward => Key::Named(NamedKey::MediaStepBackward),
            WinitNamedKey::MediaStepForward => Key::Named(NamedKey::MediaStepForward),
            WinitNamedKey::MediaStop => Key::Named(NamedKey::MediaStop),
            WinitNamedKey::MediaTopMenu => Key::Named(NamedKey::MediaTopMenu),
            WinitNamedKey::MediaTrackNext => Key::Named(NamedKey::MediaTrackNext),
            WinitNamedKey::MediaTrackPrevious => Key::Named(NamedKey::MediaTrackPrevious),
            WinitNamedKey::Meta => Key::Named(NamedKey::Meta),
            WinitNamedKey::MicrophoneToggle => Key::Named(NamedKey::MicrophoneToggle),
            WinitNamedKey::MicrophoneVolumeDown => Key::Named(NamedKey::MicrophoneVolumeDown),
            WinitNamedKey::MicrophoneVolumeMute => Key::Named(NamedKey::MicrophoneVolumeMute),
            WinitNamedKey::MicrophoneVolumeUp => Key::Named(NamedKey::MicrophoneVolumeUp),
            WinitNamedKey::ModeChange => Key::Named(NamedKey::ModeChange),
            WinitNamedKey::NavigateIn => Key::Named(NamedKey::NavigateIn),
            WinitNamedKey::NavigateNext => Key::Named(NamedKey::NavigateNext),
            WinitNamedKey::NavigateOut => Key::Named(NamedKey::NavigateOut),
            WinitNamedKey::NavigatePrevious => Key::Named(NamedKey::NavigatePrevious),
            WinitNamedKey::New => Key::Named(NamedKey::New),
            WinitNamedKey::NextCandidate => Key::Named(NamedKey::NextCandidate),
            WinitNamedKey::NextFavoriteChannel => Key::Named(NamedKey::NextFavoriteChannel),
            WinitNamedKey::NextUserProfile => Key::Named(NamedKey::NextUserProfile),
            WinitNamedKey::NonConvert => Key::Named(NamedKey::NonConvert),
            WinitNamedKey::Notification => Key::Named(NamedKey::Notification),
            WinitNamedKey::NumLock => Key::Named(NamedKey::NumLock),
            WinitNamedKey::OnDemand => Key::Named(NamedKey::OnDemand),
            WinitNamedKey::Open => Key::Named(NamedKey::Open),
            WinitNamedKey::PageDown => Key::Named(NamedKey::PageDown),
            WinitNamedKey::PageUp => Key::Named(NamedKey::PageUp),
            WinitNamedKey::Pairing => Key::Named(NamedKey::Pairing),
            WinitNamedKey::Paste => Key::Named(NamedKey::Paste),
            WinitNamedKey::Pause => Key::Named(NamedKey::Pause),
            WinitNamedKey::PinPDown => Key::Named(NamedKey::PinPDown),
            WinitNamedKey::PinPMove => Key::Named(NamedKey::PinPMove),
            WinitNamedKey::PinPToggle => Key::Named(NamedKey::PinPToggle),
            WinitNamedKey::PinPUp => Key::Named(NamedKey::PinPUp),
            WinitNamedKey::Play => Key::Named(NamedKey::Play),
            WinitNamedKey::PlaySpeedDown => Key::Named(NamedKey::PlaySpeedDown),
            WinitNamedKey::PlaySpeedReset => Key::Named(NamedKey::PlaySpeedReset),
            WinitNamedKey::PlaySpeedUp => Key::Named(NamedKey::PlaySpeedUp),
            WinitNamedKey::Power => Key::Named(NamedKey::Power),
            WinitNamedKey::PowerOff => Key::Named(NamedKey::PowerOff),
            WinitNamedKey::PreviousCandidate => Key::Named(NamedKey::PreviousCandidate),
            WinitNamedKey::Print => Key::Named(NamedKey::Print),
            WinitNamedKey::PrintScreen => Key::Named(NamedKey::PrintScreen),
            WinitNamedKey::Process => Key::Named(NamedKey::Process),
            WinitNamedKey::Props => Key::Named(NamedKey::Props),
            WinitNamedKey::RandomToggle => Key::Named(NamedKey::RandomToggle),
            WinitNamedKey::RcLowBattery => Key::Named(NamedKey::RcLowBattery),
            WinitNamedKey::RecordSpeedNext => Key::Named(NamedKey::RecordSpeedNext),
            WinitNamedKey::Redo => Key::Named(NamedKey::Redo),
            WinitNamedKey::RfBypass => Key::Named(NamedKey::RfBypass),
            WinitNamedKey::Romaji => Key::Named(NamedKey::Romaji),
            WinitNamedKey::STBInput => Key::Named(NamedKey::STBInput),
            WinitNamedKey::STBPower => Key::Named(NamedKey::STBPower),
            WinitNamedKey::Save => Key::Named(NamedKey::Save),
            WinitNamedKey::ScanChannelsToggle => Key::Named(NamedKey::ScanChannelsToggle),
            WinitNamedKey::ScreenModeNext => Key::Named(NamedKey::ScreenModeNext),
            WinitNamedKey::ScrollLock => Key::Named(NamedKey::ScrollLock),
            WinitNamedKey::Select => Key::Named(NamedKey::Select),
            WinitNamedKey::Settings => Key::Named(NamedKey::Settings),
            WinitNamedKey::Shift => Key::Named(NamedKey::Shift),
            WinitNamedKey::SingleCandidate => Key::Named(NamedKey::SingleCandidate),
            WinitNamedKey::Soft1 => Key::Named(NamedKey::Soft1),
            WinitNamedKey::Soft2 => Key::Named(NamedKey::Soft2),
            WinitNamedKey::Soft3 => Key::Named(NamedKey::Soft3),
            WinitNamedKey::Soft4 => Key::Named(NamedKey::Soft4),
            WinitNamedKey::Space => Key::Character(" ".to_string()),
            WinitNamedKey::SpeechCorrectionList => Key::Named(NamedKey::SpeechCorrectionList),
            WinitNamedKey::SpeechInputToggle => Key::Named(NamedKey::SpeechInputToggle),
            WinitNamedKey::SpellCheck => Key::Named(NamedKey::SpellCheck),
            WinitNamedKey::SplitScreenToggle => Key::Named(NamedKey::SplitScreenToggle),
            WinitNamedKey::Standby => Key::Named(NamedKey::Standby),
            WinitNamedKey::Subtitle => Key::Named(NamedKey::Subtitle),
            WinitNamedKey::Super => Key::Named(NamedKey::Meta),
            WinitNamedKey::Symbol => Key::Named(NamedKey::Symbol),
            WinitNamedKey::SymbolLock => Key::Named(NamedKey::SymbolLock),
            WinitNamedKey::TV => Key::Named(NamedKey::TV),
            WinitNamedKey::TV3DMode => Key::Named(NamedKey::TV3DMode),
            WinitNamedKey::TVAntennaCable => Key::Named(NamedKey::TVAntennaCable),
            WinitNamedKey::TVAudioDescription => Key::Named(NamedKey::TVAudioDescription),
            WinitNamedKey::TVAudioDescriptionMixDown => {
                Key::Named(NamedKey::TVAudioDescriptionMixDown)
            }
            WinitNamedKey::TVAudioDescriptionMixUp => Key::Named(NamedKey::TVAudioDescriptionMixUp),
            WinitNamedKey::TVContentsMenu => Key::Named(NamedKey::TVContentsMenu),
            WinitNamedKey::TVDataService => Key::Named(NamedKey::TVDataService),
            WinitNamedKey::TVInput => Key::Named(NamedKey::TVInput),
            WinitNamedKey::TVInputComponent1 => Key::Named(NamedKey::TVInputComponent1),
            WinitNamedKey::TVInputComponent2 => Key::Named(NamedKey::TVInputComponent2),
            WinitNamedKey::TVInputComposite1 => Key::Named(NamedKey::TVInputComposite1),
            WinitNamedKey::TVInputComposite2 => Key::Named(NamedKey::TVInputComposite2),
            WinitNamedKey::TVInputHDMI1 => Key::Named(NamedKey::TVInputHDMI1),
            WinitNamedKey::TVInputHDMI2 => Key::Named(NamedKey::TVInputHDMI2),
            WinitNamedKey::TVInputHDMI3 => Key::Named(NamedKey::TVInputHDMI3),
            WinitNamedKey::TVInputHDMI4 => Key::Named(NamedKey::TVInputHDMI4),
            WinitNamedKey::TVInputVGA1 => Key::Named(NamedKey::TVInputVGA1),
            WinitNamedKey::TVMediaContext => Key::Named(NamedKey::TVMediaContext),
            WinitNamedKey::TVNetwork => Key::Named(NamedKey::TVNetwork),
            WinitNamedKey::TVNumberEntry => Key::Named(NamedKey::TVNumberEntry),
            WinitNamedKey::TVPower => Key::Named(NamedKey::TVPower),
            WinitNamedKey::TVRadioService => Key::Named(NamedKey::TVRadioService),
            WinitNamedKey::TVSatellite => Key::Named(NamedKey::TVSatellite),
            WinitNamedKey::TVSatelliteBS => Key::Named(NamedKey::TVSatelliteBS),
            WinitNamedKey::TVSatelliteCS => Key::Named(NamedKey::TVSatelliteCS),
            WinitNamedKey::TVSatelliteToggle => Key::Named(NamedKey::TVSatelliteToggle),
            WinitNamedKey::TVTerrestrialAnalog => Key::Named(NamedKey::TVTerrestrialAnalog),
            WinitNamedKey::TVTerrestrialDigital => Key::Named(NamedKey::TVTerrestrialDigital),
            WinitNamedKey::TVTimer => Key::Named(NamedKey::TVTimer),
            WinitNamedKey::Tab => Key::Named(NamedKey::Tab),
            WinitNamedKey::Teletext => Key::Named(NamedKey::Teletext),
            WinitNamedKey::Undo => Key::Named(NamedKey::Undo),
            WinitNamedKey::VideoModeNext => Key::Named(NamedKey::VideoModeNext),
            WinitNamedKey::VoiceDial => Key::Named(NamedKey::VoiceDial),
            WinitNamedKey::WakeUp => Key::Named(NamedKey::WakeUp),
            WinitNamedKey::Wink => Key::Named(NamedKey::Wink),
            WinitNamedKey::Zenkaku => Key::Named(NamedKey::Zenkaku),
            WinitNamedKey::ZenkakuHankaku => Key::Named(NamedKey::ZenkakuHankaku),
            WinitNamedKey::ZoomIn => Key::Named(NamedKey::ZoomIn),
            WinitNamedKey::ZoomOut => Key::Named(NamedKey::ZoomOut),
            WinitNamedKey::ZoomToggle => Key::Named(NamedKey::ZoomToggle),
            _ => Key::Named(NamedKey::Unidentified),
        }
    }
}

impl FromWinitKeyEvent for Location {
    fn from_winit_key_event(key_event: &KeyEvent) -> Self {
        match key_event.location {
            WinitKeyLocation::Left => Location::Left,
            WinitKeyLocation::Numpad => Location::Numpad,
            WinitKeyLocation::Right => Location::Right,
            WinitKeyLocation::Standard => Location::Standard,
        }
    }
}

#[allow(deprecated)]
impl FromWinitKeyEvent for Code {
    fn from_winit_key_event(key_event: &KeyEvent) -> Self {
        let key_code = match key_event.physical_key {
            PhysicalKey::Code(key_code) => key_code,
            PhysicalKey::Unidentified(_os_code) => return Code::Unidentified,
        };

        match key_code {
            KeyCode::Abort => Code::Abort,
            KeyCode::Again => Code::Again,
            KeyCode::AltLeft => Code::AltLeft,
            KeyCode::AltRight => Code::AltRight,
            KeyCode::ArrowDown => Code::ArrowDown,
            KeyCode::ArrowLeft => Code::ArrowLeft,
            KeyCode::ArrowRight => Code::ArrowRight,
            KeyCode::ArrowUp => Code::ArrowUp,
            KeyCode::AudioVolumeDown => Code::AudioVolumeDown,
            KeyCode::AudioVolumeMute => Code::AudioVolumeMute,
            KeyCode::AudioVolumeUp => Code::AudioVolumeUp,
            KeyCode::Backquote => Code::Backquote,
            KeyCode::Backslash => Code::Backslash,
            KeyCode::Backspace => Code::Backspace,
            KeyCode::BracketLeft => Code::BracketLeft,
            KeyCode::BracketRight => Code::BracketRight,
            KeyCode::BrowserBack => Code::BrowserBack,
            KeyCode::BrowserFavorites => Code::BrowserFavorites,
            KeyCode::BrowserForward => Code::BrowserForward,
            KeyCode::BrowserHome => Code::BrowserHome,
            KeyCode::BrowserRefresh => Code::BrowserRefresh,
            KeyCode::BrowserSearch => Code::BrowserSearch,
            KeyCode::BrowserStop => Code::BrowserStop,
            KeyCode::CapsLock => Code::CapsLock,
            KeyCode::Comma => Code::Comma,
            KeyCode::ContextMenu => Code::ContextMenu,
            KeyCode::ControlLeft => Code::ControlLeft,
            KeyCode::ControlRight => Code::ControlRight,
            KeyCode::Convert => Code::Convert,
            KeyCode::Copy => Code::Copy,
            KeyCode::Cut => Code::Cut,
            KeyCode::Delete => Code::Delete,
            KeyCode::Digit0 => Code::Digit0,
            KeyCode::Digit1 => Code::Digit1,
            KeyCode::Digit2 => Code::Digit2,
            KeyCode::Digit3 => Code::Digit3,
            KeyCode::Digit4 => Code::Digit4,
            KeyCode::Digit5 => Code::Digit5,
            KeyCode::Digit6 => Code::Digit6,
            KeyCode::Digit7 => Code::Digit7,
            KeyCode::Digit8 => Code::Digit8,
            KeyCode::Digit9 => Code::Digit9,
            KeyCode::Eject => Code::Eject,
            KeyCode::End => Code::End,
            KeyCode::Enter => Code::Enter,
            KeyCode::Equal => Code::Equal,
            KeyCode::Escape => Code::Escape,
            KeyCode::F1 => Code::F1,
            KeyCode::F10 => Code::F10,
            KeyCode::F11 => Code::F11,
            KeyCode::F12 => Code::F12,
            KeyCode::F13 => Code::F13,
            KeyCode::F14 => Code::F14,
            KeyCode::F15 => Code::F15,
            KeyCode::F16 => Code::F16,
            KeyCode::F17 => Code::F17,
            KeyCode::F18 => Code::F18,
            KeyCode::F19 => Code::F19,
            KeyCode::F2 => Code::F2,
            KeyCode::F20 => Code::F20,
            KeyCode::F21 => Code::F21,
            KeyCode::F22 => Code::F22,
            KeyCode::F23 => Code::F23,
            KeyCode::F24 => Code::F24,
            KeyCode::F25 => Code::F25,
            KeyCode::F26 => Code::F26,
            KeyCode::F27 => Code::F27,
            KeyCode::F28 => Code::F28,
            KeyCode::F29 => Code::F29,
            KeyCode::F3 => Code::F3,
            KeyCode::F30 => Code::F30,
            KeyCode::F31 => Code::F31,
            KeyCode::F32 => Code::F32,
            KeyCode::F33 => Code::F33,
            KeyCode::F34 => Code::F34,
            KeyCode::F35 => Code::F35,
            KeyCode::F4 => Code::F4,
            KeyCode::F5 => Code::F5,
            KeyCode::F6 => Code::F6,
            KeyCode::F7 => Code::F7,
            KeyCode::F8 => Code::F8,
            KeyCode::F9 => Code::F9,
            KeyCode::Find => Code::Find,
            KeyCode::Fn => Code::Fn,
            KeyCode::FnLock => Code::FnLock,
            KeyCode::Help => Code::Help,
            KeyCode::Hiragana => Code::Hiragana,
            KeyCode::Home => Code::Home,
            KeyCode::Hyper => Code::Hyper,
            KeyCode::Insert => Code::Insert,
            KeyCode::IntlBackslash => Code::IntlBackslash,
            KeyCode::IntlRo => Code::IntlRo,
            KeyCode::IntlYen => Code::IntlYen,
            KeyCode::KanaMode => Code::KanaMode,
            KeyCode::Katakana => Code::Katakana,
            KeyCode::KeyA => Code::KeyA,
            KeyCode::KeyB => Code::KeyB,
            KeyCode::KeyC => Code::KeyC,
            KeyCode::KeyD => Code::KeyD,
            KeyCode::KeyE => Code::KeyE,
            KeyCode::KeyF => Code::KeyF,
            KeyCode::KeyG => Code::KeyG,
            KeyCode::KeyH => Code::KeyH,
            KeyCode::KeyI => Code::KeyI,
            KeyCode::KeyJ => Code::KeyJ,
            KeyCode::KeyK => Code::KeyK,
            KeyCode::KeyL => Code::KeyL,
            KeyCode::KeyM => Code::KeyM,
            KeyCode::KeyN => Code::KeyN,
            KeyCode::KeyO => Code::KeyO,
            KeyCode::KeyP => Code::KeyP,
            KeyCode::KeyQ => Code::KeyQ,
            KeyCode::KeyR => Code::KeyR,
            KeyCode::KeyS => Code::KeyS,
            KeyCode::KeyT => Code::KeyT,
            KeyCode::KeyU => Code::KeyU,
            KeyCode::KeyV => Code::KeyV,
            KeyCode::KeyW => Code::KeyW,
            KeyCode::KeyX => Code::KeyX,
            KeyCode::KeyY => Code::KeyY,
            KeyCode::KeyZ => Code::KeyZ,
            KeyCode::Lang1 => Code::Lang1,
            KeyCode::Lang2 => Code::Lang2,
            KeyCode::Lang3 => Code::Lang3,
            KeyCode::Lang4 => Code::Lang4,
            KeyCode::Lang5 => Code::Lang5,
            KeyCode::LaunchApp1 => Code::LaunchApp1,
            KeyCode::LaunchApp2 => Code::LaunchApp2,
            KeyCode::LaunchMail => Code::LaunchMail,
            KeyCode::MediaPlayPause => Code::MediaPlayPause,
            KeyCode::MediaSelect => Code::MediaSelect,
            KeyCode::MediaStop => Code::MediaStop,
            KeyCode::MediaTrackNext => Code::MediaTrackNext,
            KeyCode::MediaTrackPrevious => Code::MediaTrackPrevious,
            KeyCode::Meta => Code::Super,
            KeyCode::Minus => Code::Minus,
            KeyCode::NonConvert => Code::NonConvert,
            KeyCode::NumLock => Code::NumLock,
            KeyCode::Numpad0 => Code::Numpad0,
            KeyCode::Numpad1 => Code::Numpad1,
            KeyCode::Numpad2 => Code::Numpad2,
            KeyCode::Numpad3 => Code::Numpad3,
            KeyCode::Numpad4 => Code::Numpad4,
            KeyCode::Numpad5 => Code::Numpad5,
            KeyCode::Numpad6 => Code::Numpad6,
            KeyCode::Numpad7 => Code::Numpad7,
            KeyCode::Numpad8 => Code::Numpad8,
            KeyCode::Numpad9 => Code::Numpad9,
            KeyCode::NumpadAdd => Code::NumpadAdd,
            KeyCode::NumpadBackspace => Code::NumpadBackspace,
            KeyCode::NumpadClear => Code::NumpadClear,
            KeyCode::NumpadClearEntry => Code::NumpadClearEntry,
            KeyCode::NumpadComma => Code::NumpadComma,
            KeyCode::NumpadDecimal => Code::NumpadDecimal,
            KeyCode::NumpadDivide => Code::NumpadDivide,
            KeyCode::NumpadEnter => Code::NumpadEnter,
            KeyCode::NumpadEqual => Code::NumpadEqual,
            KeyCode::NumpadHash => Code::NumpadHash,
            KeyCode::NumpadMemoryAdd => Code::NumpadMemoryAdd,
            KeyCode::NumpadMemoryClear => Code::NumpadMemoryClear,
            KeyCode::NumpadMemoryRecall => Code::NumpadMemoryRecall,
            KeyCode::NumpadMemoryStore => Code::NumpadMemoryStore,
            KeyCode::NumpadMemorySubtract => Code::NumpadMemorySubtract,
            KeyCode::NumpadMultiply => Code::NumpadMultiply,
            KeyCode::NumpadParenLeft => Code::NumpadParenLeft,
            KeyCode::NumpadParenRight => Code::NumpadParenRight,
            KeyCode::NumpadStar => Code::NumpadStar,
            KeyCode::NumpadSubtract => Code::NumpadSubtract,
            KeyCode::Open => Code::Open,
            KeyCode::PageDown => Code::PageDown,
            KeyCode::PageUp => Code::PageUp,
            KeyCode::Paste => Code::Paste,
            KeyCode::Pause => Code::Pause,
            KeyCode::Period => Code::Period,
            KeyCode::Power => Code::Power,
            KeyCode::PrintScreen => Code::PrintScreen,
            KeyCode::Props => Code::Props,
            KeyCode::Quote => Code::Quote,
            KeyCode::Resume => Code::Resume,
            KeyCode::ScrollLock => Code::ScrollLock,
            KeyCode::Select => Code::Select,
            KeyCode::Semicolon => Code::Semicolon,
            KeyCode::ShiftLeft => Code::ShiftLeft,
            KeyCode::ShiftRight => Code::ShiftRight,
            KeyCode::Slash => Code::Slash,
            KeyCode::Sleep => Code::Sleep,
            KeyCode::Space => Code::Space,
            KeyCode::SuperLeft => Code::MetaLeft,
            KeyCode::SuperRight => Code::MetaRight,
            KeyCode::Suspend => Code::Suspend,
            KeyCode::Tab => Code::Tab,
            KeyCode::Turbo => Code::Turbo,
            KeyCode::Undo => Code::Undo,
            KeyCode::WakeUp => Code::WakeUp,
            _ => Code::Unidentified,
        }
    }
}

impl FromWinitKeyEvent for KeyState {
    fn from_winit_key_event(key_event: &KeyEvent) -> Self {
        match key_event.state {
            ElementState::Pressed => KeyState::Down,
            ElementState::Released => KeyState::Up,
        }
    }
}

pub fn keyboard_modifiers_from_winit_modifiers(mods: ModifiersState) -> Modifiers {
    let mut modifiers = Modifiers::empty();
    modifiers.set(Modifiers::CONTROL, mods.control_key());
    modifiers.set(Modifiers::SHIFT, mods.shift_key());
    modifiers.set(Modifiers::ALT, mods.alt_key());
    modifiers.set(Modifiers::META, mods.super_key());
    modifiers
}

pub fn key_input_from_winit(key_event: &KeyEvent, state: ModifiersState) -> KeyInput {
    KeyInput {
        state: KeyState::from_winit_key_event(key_event),
        key: Key::from_winit_key_event(key_event),
        code: Code::from_winit_key_event(key_event),
        location: Location::from_winit_key_event(key_event),
        modifiers: keyboard_modifiers_from_winit_modifiers(state),
        repeat: key_event.repeat,
    }
}
```

**Step 3: Create the input mapping**

`crates/paladin/src/input.rs`:

```rust
//! winit input to engine input. Coordinates are converted by the window
//! (toolbar offset, physical pixels); this module maps the enums.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

**Step 4: Create the icon module**

`crates/paladin/src/icon.rs`:

```rust
//! The application icon: the window icon (Windows title bar) and, on
//! macOS, the Dock icon, so `cargo run` shows the logo without an `.app`
//! bundle. Both come from the generated PNG checked in under `assets/`.

use winit::window::Icon;

const ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/paladin-256.png");

/// Decode the embedded PNG to straight RGBA8.
fn decode_icon() -> Result<(Vec<u8>, u32, u32), png::DecodingError> {
    let decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer)?;
    buffer.truncate(info.buffer_size());
    debug_assert_eq!(info.color_type, png::ColorType::Rgba);
    debug_assert_eq!(info.bit_depth, png::BitDepth::Eight);
    Ok((buffer, info.width, info.height))
}

/// The window icon. winit ignores it on macOS, where the Dock icon is
/// set separately.
pub fn window_icon() -> Option<Icon> {
    let (rgba, width, height) = match decode_icon() {
        Ok(decoded) => decoded,
        Err(e) => {
            log::warn!("cannot decode embedded icon: {e}");
            return None;
        }
    };
    match Icon::from_rgba(rgba, width, height) {
        Ok(icon) => Some(icon),
        Err(e) => {
            log::warn!("cannot build window icon: {e}");
            None
        }
    }
}

/// Set the Dock icon from the embedded PNG. Must run on the main thread
/// after the event loop (and so `NSApplication`) exists.
#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("set_dock_icon called off the main thread; skipping");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let data = NSData::with_bytes(ICON_PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        log::warn!("AppKit could not decode the embedded icon");
        return;
    };
    // SAFETY: on the main thread with a valid NSImage; AppKit retains it.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_decodes_to_256_square_rgba() {
        let (rgba, width, height) = decode_icon().unwrap();
        assert_eq!((width, height), (256, 256));
        assert_eq!(rgba.len(), 256 * 256 * 4);
    }
}
```

**Step 5: Create the chrome**

`crates/paladin/src/chrome.rs`:

```rust
//! The window chrome: tab strip, toolbar, status overlay, and crash panel.
//! Pure functions of the model that return the commands the user issued.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{Button, ColorImage, Id, Key, Panel, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use paladin_core::{Command, Favicon, LoadState, TabId, Window};
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
                    if ui.button("+").on_hover_text("New tab").clicked() {
                        commands.push(Command::NewTab {
                            window: window.id,
                            url: None,
                        });
                    }
                });
            });
    });

    Panel::top("toolbar").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(active.can_go_back, Button::new("◀"))
                .on_hover_text("Back")
                .clicked()
            {
                commands.push(Command::Back(active.id));
            }
            if ui
                .add_enabled(active.can_go_forward, Button::new("▶"))
                .on_hover_text("Forward")
                .clicked()
            {
                commands.push(Command::Forward(active.id));
            }
            // Reload is always available (a hung load must be recoverable);
            // Stop appears beside it while loading.
            if ui.button("⟳").on_hover_text("Reload").clicked() {
                state.address_dirty = false;
                commands.push(Command::Reload(active.id));
            }
            // Servo 0.7.0 cannot stop a load; the button shows the state but
            // stays disabled so the UI does not promise what the engine lacks.
            if active.load == LoadState::Loading {
                ui.add_enabled(false, Button::new("✕"))
                    .on_disabled_hover_text("Stop (not supported by Servo 0.7.0)");
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

**Step 6: Create the browser window**

`crates/paladin/src/window.rs`:

```rust
//! A browser window: a winit window, the engine's tab host for it, and an
//! egui context sharing the engine's GL context.

use std::collections::HashSet;
use std::sync::Arc;

use egui::{LayerId, PaintCallback};
use egui_glow::{CallbackFn, EguiGlow};
use keyboard_types::Code;
use paladin_core::{Browser, Command, WindowId};
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
use crate::{icon, input};

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
    /// go to the page too, so it never sees half a key stroke, for example
    /// from a key pressed in another window before focus moved here.
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
    /// interactable egui layer (the error overlay), while a popup is open
    /// (so a click outside closes it), or while the crash panel replaces
    /// the page; otherwise the page.
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
    pub fn handle_event(&mut self, browser: &Browser, event: &WindowEvent) -> Vec<Command> {
        let active = browser.window(self.id).map(|w| w.active);
        let commands = Vec::new();
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
                return self.frame(browser);
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
                } else if !self.egui.egui_ctx.egui_wants_keyboard_input() {
                    forward_to_egui = false;
                    // Releases were handled above; a repeat is delivered
                    // only while the page holds the key.
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
    fn frame(&mut self, browser: &Browser) -> Vec<Command> {
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
            commands = chrome::draw(ui, model, engine_error, chrome_state);
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

**Step 7: Create the app**

`crates/paladin/src/app.rs`:

```rust
//! The winit application: owns the engine, the browser model, and the
//! native windows, and runs the dispatch loop.
//!
//! One iteration, for every winit event:
//! 1. The window the event belongs to handles it: chrome events go to egui,
//!    content events go to the active tab through the engine. This yields
//!    `Command`s.
//! 2. `dispatch` turns each `Command` into model changes plus
//!    `EngineAction`s, which `perform` forwards to the engine or handles.
//! 3. The engine spins; its `EngineEvent`s are drained into `apply`, whose
//!    follow-up actions are performed too.
//! 4. Windows that were touched redraw; the redraw paints the active tab
//!    offscreen and egui blits it below the toolbar.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use paladin_core::{Browser, Command, EngineAction, EngineEvent, TabId, WindowId, apply, dispatch};
use paladin_engine::{Engine, Waker};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};

use crate::icon;
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
    // Windows are declared before `engine` so they drop first: every tab
    // host and GL surface must be gone before the engine shuts down.
    windows: HashMap<winit::window::WindowId, BrowserWindow>,
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
        initial_actions: Vec<EngineAction>,
        event_loop: &EventLoop<AppEvent>,
    ) -> App {
        App {
            browser,
            windows: HashMap::new(),
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
            EngineAction::OpenSettings => {
                log::info!("the settings window arrives in Phase 5");
            }
            EngineAction::Quit => {
                self.exiting = true;
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
        if let Some(window) = self.windows.get_mut(&window_id) {
            let commands = window.handle_event(&self.browser, &event);
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
        self.windows.clear();
        self.engine = None;
    }
}
```

**Step 8: Replace main**

`crates/paladin/src/main.rs`:

```rust
//! Paladin: a desktop browser on the Servo engine with an egui chrome.

mod app;
mod chrome;
mod icon;
mod input;
mod keyutils;
mod window;

use std::process::ExitCode;

use clap::Parser;
use paladin_core::command::open_window;
use paladin_core::profile::DEFAULT_PROFILE_NAME;
use paladin_core::{Browser, Command, Profile, Settings, dispatch};
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
    // before the window sees the key. Without the menu, the app's own
    // shutdown path runs instead (the keymap binds Cmd+Q in Phase 5).
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
    let mut app = App::new(browser, actions, &event_loop);
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

**Step 9: Build, lint, and test**

Run: `cargo build -p paladin`
Expected: fetches and builds clap, egui, egui-winit, egui_glow, winit, png, and (macOS) objc2; `Finished`, no warnings.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: `Finished`, no warnings.

Run: `cargo test -p paladin`
Expected: `8 passed`: five `chrome::tests` for `location_input_to_url` and `truncate`, two `input::tests`, and `icon::tests::embedded_icon_decodes_to_256_square_rgba`.

**Step 10: Run the browser**

Run: `cargo run`
Expected: a 1024x768 window titled "Paladin" opens, the Dock shows the Paladin logo, the tab strip shows one tab, and the page area renders `https://servo.org/` (the default homepage). The log prints `profile "default" at /Users/thomas/Library/Application Support/Paladin/profiles/default`.

Then run the first seven items of the manual checklist from Task 2 (the README is written there; the list is the same). Close the window to exit.

Run: `cargo run -- example.com https://servo.org/`
Expected: two tabs, the first at `https://example.com/`.

Run: `cargo run -- --profile test`
Expected: a window opens; `ls ~/Library/Application\ Support/Paladin/profiles/` lists `default` and `test`.

**Step 11: Commit**

```bash
git add Cargo.lock crates/paladin
git commit -m "feat(app): browser window with tab strip, toolbar, and Servo content"
```
<!-- END_TASK_1 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_2 -->
### Task 2: Manual checklist and changelog

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`

**Step 1: Remove the status note and add the checklist to the README**

In `README.md`, delete the three-line blockquote that starts `> Phase 1 is in progress.` (just below the opening paragraph). Then insert this section between `## Tests` and `## Todo`:

```markdown
## Manual checklist

Run on macOS with `cargo run` after any change to the chrome, input handling,
or engine glue. Each item must pass.

1. **Launch.** A window titled Paladin opens with one tab at the homepage,
   the page renders, and the Dock (macOS) or title bar (Windows) shows the
   Paladin logo.
2. **Address bar.** Type `example.com` and press Enter: the page loads, the
   tab title becomes "Example Domain", and the address bar shows
   `https://example.com/`. Typing `localhost:8080` targets `http://`.
3. **History.** Navigate to a second page, then use Back and Forward: the
   buttons enable and disable with the history and the pages change. Reload
   is always present and reloads the current page; while a page loads, a
   disabled Stop button appears beside it whose tooltip explains that Servo
   cannot stop a load.
4. **Tabs.** "+" opens a new tab and activates it; clicking a tab title
   switches tabs and the address bar follows; "×" and middle-click close a
   tab and the neighbour to the right becomes active; closing the last tab
   closes the window and the process exits.
5. **Resize and HiDPI.** Resize the window: the page stays aligned under the
   toolbar with no gap or stretching. Drag the window to a display with a
   different scale factor (or change the display scaling): text stays sharp
   and clicks land where expected.
6. **Input.** Scrolling with the wheel or trackpad over the page scrolls it;
   clicking a text field on a page and typing enters text; typing in the
   address bar does not reach the page. Hovering a link shows its URL in the
   status overlay at the bottom left without the page moving. Drag-select
   text in the page, release the button over the toolbar, then move the
   pointer back over the page: the selection does not keep growing. Press
   in the address bar, release over the page, then move over the page: the
   address bar's selection does not keep growing either.
7. **Exit.** Closing the window exits with no Servo error output on the
   terminal. Quitting from the Dock menu does the same.
```

**Step 2: Record the change**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, add after the last bullet:

```markdown
- A browser window: tab strip with favicons, toolbar with back, forward,
  reload, and an address bar, Servo content rendered under the toolbar, a
  status overlay for link targets and engine errors, the logo as window and
  Dock icon, `--profile` and URL arguments, and a manual checklist in the
  README.
```

**Step 3: Run the checklist**

Run: `cargo run`
Expected: items 1 to 7 pass. If an item fails, fix it before committing; the fix belongs in this phase, and Phase 5's instructions carry such fixes forward.

**Step 4: Commit and push**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: manual checklist for the browser window"
git push
```

Run: `gh run watch --exit-status`
Expected: CI green on both runners.
<!-- END_TASK_2 -->

---

## Phase 4 verification

- `cargo run` on the Mac passes manual checklist items 1 to 7.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all --check` are clean.
- CI is green on both runners.
