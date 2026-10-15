# Paladin Phase 1 Desktop Browser Implementation Plan

**Goal:** A desktop browser for macOS and Windows that renders pages with Servo under an egui chrome, with multiple windows and tabs, a settings window, a logo, installable packages, and CI on both platforms.

**Architecture:** Three-crate Cargo workspace plus an `xtask` helper. `paladin-core` holds the Servo-free model, named commands, keymap, settings, and profile paths. `paladin-engine` is the only crate that imports Servo and exposes `Engine`, `GlSurface`, and `TabHost`. The `paladin` binary runs the winit event loop and draws the chrome with egui, blitting Servo's offscreen framebuffer under the toolbar. The UI only emits `Command`s; the pure `dispatch` and `apply` functions in core turn commands and engine events into state changes plus `EngineAction`s.

**Tech Stack:** Rust 1.97.1 (edition 2024), servo 0.7.0, winit 0.30.13, egui / egui-winit / egui_glow 0.34.3, glow 0.17.0, surfman 0.14.0, keyboard-types 0.8.3, directories 6, toml 1.1, clap 4, resvg 0.48.1, icns 0.5, ico 0.6, cargo-packager 0.11.8, GitHub Actions on `macos-15` and `windows-2025`.

**Scope:** 6 phases from original design (this file is phase 3 of 6)

**Codebase verified:** 2026-10-14. After Phase 2, `crates/paladin-engine` holds a placeholder `lib.rs` and a `Cargo.toml` depending only on `paladin-core`; `xtask` already has the `angle-dlls` task. The code below was type-checked, linted, built, and its headless test run against the published `servo` 0.7.0 crate on the development Mac (build plus test: about 3 minutes after dependencies were fetched). Facts about Servo 0.7.0 that shaped it, all verified in its source at tag `v0.7.0`:

- `ServoBuilder::default().opts(..).preferences(..).event_loop_waker(..).build()` is the only way to build Servo; `Servo::new` is private. Building twice in one process panics (`opts::initialize_options` is process-global).
- `WebViewBuilder::new(&servo, rendering_context)` takes the rendering context per WebView; a WebView is as big as its context and cannot move to another.
- `Servo::spin_event_loop()` returns nothing; shutdown happens when the last `Servo` handle drops (all `WebView`s hold one). There is no `stop` API on `WebView`.
- `WebViewDelegate::request_create_new` hands over a `CreateNewWebViewRequest`; the embedder must build and keep the `WebView` at once or Servo drops it.
- `OffscreenRenderingContext::render_to_parent_callback()` returns the blit closure egui calls; `present()` on the offscreen context is a no-op, the parent `WindowRenderingContext::present()` swaps.
- SpiderMonkey arrives as a prebuilt archive (`mozjs-sys` downloads it with `curl` when `js_jit` is on), so no CMake, MozTools, or GStreamer is needed. Servo's WebIDL codegen needs Python 3.11+; bindgen (for ANGLE) needs `libclang`, which Xcode Command Line Tools provide on the Mac.
- A fresh lockfile resolves `pkcs1` to `0.8.0-rc.5`, which does not compile with Servo's `rsa 0.10.0-rc.18`; Servo's own lockfile pins `pkcs1 0.8.0-rc.4`. Task 1 pins it.

Deviations from the design's sketch of this crate, each deliberate:

- `GlSurface` is split out of `TabHost`: a window's GL context is created once and shared by its tab host and its egui, and the settings window (Phase 5) needs a GL surface with no tabs at all. `TabHost::new` therefore takes a `GlSurface` rather than raw handles; `present` and window resizing live on `GlSurface`; the design's `resize` becomes `resize_content(tab, size)` (the page area under the toolbar), and `blit_callback()` returns a closure the app hands to egui.
- `Waker` is a trait with one method instead of a boxed closure, so the app can pass an `Arc` it also keeps.
- `Engine::new` catches Servo's start-up panic and returns it as an error, because the design wants a message naming the profile directory when the engine fails to start.
- There is no `stop`: Servo 0.7.0 has no API for it. `TabHost::stop` logs; the chrome keeps Reload available while loading.

---

## Conventions for every task in every phase

- Run every command from the repository root, `/Users/thomas/vibes/paladin`.
- Commit to the current branch (`main`) with Conventional Commits. No feature branches.
- "Create" means the file does not exist yet; write exactly the content shown. "Replace" means overwrite the whole file with the content shown. Code blocks are complete files unless a step says otherwise.
- Before every commit run `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`. All three must be clean. From this phase on these commands compile Servo; the first run takes minutes, later runs seconds.

---

## Phase 3: Engine crate

**Goal:** Servo builds inside the workspace and loads a page headlessly.

**Done when:** The headless test passes on macOS locally and on both CI runners; the README records expected build times and the macOS and Windows prerequisites (already written in Phase 1; this phase confirms them).

<!-- START_SUBCOMPONENT_A (tasks 1-2) -->
<!-- START_TASK_1 -->
### Task 1: The engine crate: Servo dependency, engine, delegate, input, and tab host

**Files:**
- Replace: `crates/paladin-engine/Cargo.toml`
- Replace: `crates/paladin-engine/src/lib.rs`
- Create: `crates/paladin-engine/src/engine.rs`
- Create: `crates/paladin-engine/src/delegate.rs`
- Create: `crates/paladin-engine/src/input.rs`
- Create: `crates/paladin-engine/src/host.rs`
- Modify: `Cargo.lock` (pin `pkcs1`)

**Step 1: Replace the manifest**

`crates/paladin-engine/Cargo.toml`:

```toml
[package]
name = "paladin-engine"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
description = "Servo glue: the only crate that imports the servo crate"

[dependencies]
paladin-core = { path = "../paladin-core" }
dpi = { workspace = true }
euclid = { workspace = true }
glow = { workspace = true }
keyboard-types = { workspace = true }
log = { workspace = true }
raw-window-handle = { workspace = true }
# Servo's networking needs a process-wide rustls crypto provider; the engine
# installs it. aws-lc-rs is what Servo's own shell uses.
rustls = { version = "0.23", default-features = false, features = ["aws-lc-rs"] }
servo = { version = "=0.7.0", default-features = false, features = [
    "baked-in-resources",
    "default_web_features",
    "js_jit",
    "multiprocess",
] }
url = { workspace = true }

[target.'cfg(target_os = "windows")'.dependencies]
# ANGLE's EGL instead of WGL, as Servo's shell does. Produces libEGL.dll and
# libGLESv2.dll, which `cargo xtask angle-dlls` copies next to the binaries.
servo = { version = "=0.7.0", default-features = false, features = ["no-wgl"] }

[dev-dependencies]
tempfile = "3"
```

**Step 2: Resolve dependencies and pin the pre-release crate**

Run: `cargo fetch`
Expected: downloads the `servo` crate family and its dependencies (a few minutes), updating `Cargo.lock`.

Run: `cargo update -p pkcs1 --precise 0.8.0-rc.4`
Expected: `Downgrading pkcs1 v0.8.0-rc.5 -> v0.8.0-rc.4`.

Run: `grep -A1 '^name = "pkcs1"$' Cargo.lock`
Expected:

```
name = "pkcs1"
version = "0.8.0-rc.4"
```

**Step 3: Create the engine module**

`crates/paladin-engine/src/engine.rs`:

```rust
//! Servo construction, the event-loop waker bridge, and the event queue.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use paladin_core::{EngineEvent, PendingTab};
use servo::{EventLoopWaker, Opts, Preferences, Servo, ServoBuilder};

use crate::delegate::DelegateState;

/// Wakes the application's event loop from any thread. Servo calls this
/// when it has work for the main thread; the app then calls
/// [`Engine::spin`].
pub trait Waker: Send + Sync + 'static {
    fn wake(&self);
}

#[derive(Debug)]
pub struct EngineError(pub String);

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EngineError {}

#[derive(Clone)]
struct ServoWaker(Arc<dyn Waker>);

impl EventLoopWaker for ServoWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        self.0.wake();
    }
}

/// One Servo instance. Servo's options and preferences are process-global,
/// so build exactly one `Engine` per process.
pub struct Engine {
    servo: Servo,
    delegate: Rc<DelegateState>,
}

impl Engine {
    /// Start Servo with its cookies, local storage, and HTTP cache under
    /// `servo_config_dir` (the profile's `servo/` directory).
    pub fn new(servo_config_dir: &Path, waker: Arc<dyn Waker>) -> Result<Engine, EngineError> {
        install_crypto_provider();
        std::fs::create_dir_all(servo_config_dir).map_err(|e| {
            EngineError(format!(
                "creating engine directory {}: {e}",
                servo_config_dir.display()
            ))
        })?;
        let opts = Opts {
            config_dir: Some(servo_config_dir.to_path_buf()),
            ..Opts::default()
        };
        // Servo reports start-up failures by panicking. Turn that into an
        // error so the app can name the profile and exit cleanly.
        let servo = catch_unwind(AssertUnwindSafe(|| {
            ServoBuilder::default()
                .opts(opts)
                .preferences(Preferences::default())
                .event_loop_waker(Box::new(ServoWaker(waker)))
                .build()
        }))
        .map_err(|payload| {
            EngineError(format!(
                "Servo failed to start: {}",
                panic_message(&payload)
            ))
        })?;
        let delegate = Rc::new(DelegateState::default());
        servo.set_delegate(delegate.clone());
        Ok(Engine { servo, delegate })
    }

    /// Run one iteration of Servo's embedder loop: delivers delegate
    /// callbacks (which become [`EngineEvent`]s) and updates painting state.
    /// Call after every window event and every [`Waker::wake`].
    pub fn spin(&self) {
        self.servo.spin_event_loop();
    }

    /// Take every event produced since the last call.
    pub fn drain_events(&self) -> Vec<EngineEvent> {
        self.delegate.drain()
    }

    /// Drop a tab that page content opened but that no window adopted.
    pub fn discard_pending(&self, pending: PendingTab) {
        if self.delegate.take_pending(pending).is_none() {
            log::debug!("no pending tab {pending:?} to discard");
        }
    }

    pub(crate) fn servo(&self) -> &Servo {
        &self.servo
    }

    pub(crate) fn delegate(&self) -> &Rc<DelegateState> {
        &self.delegate
    }
}

impl Drop for Engine {
    /// Pending tabs hold `Servo` handles; release them so Servo can shut
    /// down when the `servo` field drops next.
    fn drop(&mut self) {
        self.delegate.clear_pending();
    }
}

/// Servo's networking reads the process default rustls provider. Installing
/// twice is an error, which is harmless here.
fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
```

**Step 4: Create the delegate module**

`crates/paladin-engine/src/delegate.rs`:

```rust
//! Servo delegate callbacks become [`EngineEvent`]s keyed by core tab ids.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};

use paladin_core::{EngineEvent, Favicon, LoadState, PendingTab, TabId};
use servo::{
    ConsoleLogLevel, CreateNewWebViewRequest, Image, LoadStatus, PixelFormat, ServoDelegate,
    ServoError, WebView, WebViewDelegate, WebViewId,
};
use url::Url;

/// Shared by every `WebView` and by `Servo` itself. All callbacks arrive on
/// the engine thread from inside `spin_event_loop`, so plain `RefCell`s
/// suffice.
#[derive(Default)]
pub(crate) struct DelegateState {
    events: RefCell<VecDeque<EngineEvent>>,
    tabs: RefCell<HashMap<WebViewId, TabId>>,
    /// Tabs that page content opened (`window.open`), held alive until the
    /// model adopts them.
    pending: RefCell<HashMap<PendingTab, WebView>>,
    next_pending: Cell<u64>,
}

impl DelegateState {
    pub(crate) fn register(&self, webview: &WebView, tab: TabId) {
        self.tabs.borrow_mut().insert(webview.id(), tab);
    }

    pub(crate) fn unregister(&self, webview: &WebView) {
        self.tabs.borrow_mut().remove(&webview.id());
    }

    pub(crate) fn take_pending(&self, pending: PendingTab) -> Option<WebView> {
        self.pending.borrow_mut().remove(&pending)
    }

    /// Drop every tab page content opened that no window adopted.
    pub(crate) fn clear_pending(&self) {
        self.pending.borrow_mut().clear();
    }

    pub(crate) fn drain(&self) -> Vec<EngineEvent> {
        self.events.borrow_mut().drain(..).collect()
    }

    pub(crate) fn push(&self, event: EngineEvent) {
        self.events.borrow_mut().push_back(event);
    }

    fn tab(&self, webview: &WebView) -> Option<TabId> {
        let tab = self.tabs.borrow().get(&webview.id()).copied();
        if tab.is_none() {
            log::debug!("event for unregistered webview {:?}", webview.id());
        }
        tab
    }
}

impl WebViewDelegate for DelegateState {
    fn notify_url_changed(&self, webview: WebView, url: Url) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::UrlChanged { tab, url });
        }
    }

    fn notify_page_title_changed(&self, webview: WebView, title: Option<String>) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::TitleChanged {
                tab,
                title: title.unwrap_or_default(),
            });
        }
    }

    fn notify_status_text_changed(&self, webview: WebView, status: Option<String>) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::StatusChanged {
                tab,
                text: status.unwrap_or_default(),
            });
        }
    }

    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        if let Some(tab) = self.tab(&webview) {
            let state = match status {
                LoadStatus::Started | LoadStatus::HeadParsed => LoadState::Loading,
                LoadStatus::Complete => LoadState::Complete,
            };
            self.push(EngineEvent::LoadStateChanged { tab, state });
        }
    }

    fn notify_favicon_changed(&self, webview: WebView) {
        let Some(tab) = self.tab(&webview) else {
            return;
        };
        let favicon = webview.favicon().and_then(|image| favicon_from(&image));
        if let Some(favicon) = favicon {
            self.push(EngineEvent::FaviconChanged { tab, favicon });
        }
    }

    fn notify_new_frame_ready(&self, webview: WebView) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::FrameReady(tab));
        }
    }

    fn notify_history_changed(&self, webview: WebView, entries: Vec<Url>, current: usize) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::HistoryChanged {
                tab,
                can_go_back: current > 0,
                can_go_forward: current + 1 < entries.len(),
            });
        }
    }

    fn notify_closed(&self, webview: WebView) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::TabClosed(tab));
        }
    }

    fn notify_crashed(&self, webview: WebView, reason: String, backtrace: Option<String>) {
        if let Some(backtrace) = backtrace {
            log::error!("tab crashed: {reason}\n{backtrace}");
        } else {
            log::error!("tab crashed: {reason}");
        }
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::TabCrashed { tab, reason });
        }
    }

    fn request_create_new(&self, parent: WebView, request: CreateNewWebViewRequest) {
        let Some(opener) = self.tab(&parent) else {
            return;
        };
        // The new WebView must be built now and kept alive, or Servo drops
        // it. It shares the parent's rendering context, so it belongs to
        // the same window; the model decides where it goes via AdoptTab.
        let webview = request
            .builder(parent.rendering_context())
            .hidpi_scale_factor(parent.hidpi_scale_factor())
            .delegate(parent.delegate())
            .build();
        let pending = PendingTab(self.next_pending.get());
        self.next_pending.set(pending.0 + 1);
        self.pending.borrow_mut().insert(pending, webview);
        self.push(EngineEvent::NewTabRequested { opener, pending });
    }

    fn show_console_message(&self, _webview: WebView, level: ConsoleLogLevel, message: String) {
        log::log!(level.into(), "{message}");
    }
}

impl ServoDelegate for DelegateState {
    fn notify_error(&self, error: ServoError) {
        self.push(EngineEvent::EngineError(format!("{error:?}")));
    }

    fn show_console_message(&self, level: ConsoleLogLevel, message: String) {
        log::log!(level.into(), "{message}");
    }
}

/// Convert Servo's favicon image to straight RGBA8.
fn favicon_from(image: &Image) -> Option<Favicon> {
    let (width, height) = (image.width, image.height);
    let pixels = (width as usize) * (height as usize);
    let data = image.data();
    let rgba: Vec<u8> = match image.format {
        PixelFormat::RGBA8 => data.to_vec(),
        PixelFormat::BGRA8 => data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect(),
        PixelFormat::RGB8 => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        PixelFormat::K8 => data.iter().flat_map(|&k| [k, k, k, 255]).collect(),
        PixelFormat::KA8 => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
    };
    if rgba.len() != pixels * 4 {
        log::warn!("favicon pixel data has unexpected length");
        return None;
    }
    Some(Favicon {
        width,
        height,
        rgba,
    })
}
```

**Step 5: Create the input module**

`crates/paladin-engine/src/input.rs`:

```rust
//! Paladin's input events and their conversion to Servo's. The app builds
//! `EngineInput` from winit events; this keeps winit out of the engine and
//! Servo out of the app.

use keyboard_types::{Code, Key, KeyState, Location, Modifiers};
use servo::{
    CompositionEvent, CompositionState, DevicePoint, ImeEvent, InputEvent, KeyboardEvent,
    MouseButton as ServoMouseButton, MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent,
    MouseMoveEvent, WheelDelta, WheelEvent, WheelMode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Primary,
    Secondary,
    Auxiliary,
    Back,
    Forward,
    Other(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    Down,
    Up,
}

/// A key event in `keyboard-types` terms (the W3C UI Events model).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInput {
    pub state: KeyState,
    pub key: Key,
    pub code: Code,
    pub location: Location,
    pub modifiers: Modifiers,
    pub repeat: bool,
}

/// Input for a tab. Positions are physical pixels relative to the top-left
/// of the page area (below the toolbar).
#[derive(Clone, Debug, PartialEq)]
pub enum EngineInput {
    MouseMove {
        x: f32,
        y: f32,
    },
    MouseButton {
        action: ButtonAction,
        button: MouseButton,
        x: f32,
        y: f32,
    },
    /// The pointer left the page area.
    MouseLeft,
    /// Scroll by `(dx, dy)` pixels with the pointer at `(x, y)`.
    Wheel {
        dx: f64,
        dy: f64,
        x: f32,
        y: f32,
    },
    Key(KeyInput),
    ImeStart,
    ImePreedit(String),
    ImeCommit(String),
    ImeDismissed,
}

pub(crate) fn to_servo(input: EngineInput) -> InputEvent {
    match input {
        EngineInput::MouseMove { x, y } => {
            InputEvent::MouseMove(MouseMoveEvent::new(DevicePoint::new(x, y).into()))
        }
        EngineInput::MouseButton {
            action,
            button,
            x,
            y,
        } => InputEvent::MouseButton(MouseButtonEvent::new(
            match action {
                ButtonAction::Down => MouseButtonAction::Down,
                ButtonAction::Up => MouseButtonAction::Up,
            },
            match button {
                MouseButton::Primary => ServoMouseButton::Primary,
                MouseButton::Secondary => ServoMouseButton::Secondary,
                MouseButton::Auxiliary => ServoMouseButton::Auxiliary,
                MouseButton::Back => ServoMouseButton::Back,
                MouseButton::Forward => ServoMouseButton::Forward,
                MouseButton::Other(n) => ServoMouseButton::Other(n),
            },
            DevicePoint::new(x, y).into(),
        )),
        EngineInput::MouseLeft => InputEvent::MouseLeftViewport(MouseLeftViewportEvent::default()),
        EngineInput::Wheel { dx, dy, x, y } => InputEvent::Wheel(WheelEvent::new(
            WheelDelta {
                x: dx,
                y: dy,
                z: 0.0,
                mode: WheelMode::DeltaPixel,
            },
            DevicePoint::new(x, y).into(),
        )),
        EngineInput::Key(k) => InputEvent::Keyboard(KeyboardEvent::new_without_event(
            k.state,
            k.key,
            k.code,
            k.location,
            k.modifiers,
            k.repeat,
            false,
        )),
        EngineInput::ImeStart => composition(CompositionState::Start, String::new()),
        EngineInput::ImePreedit(text) => composition(CompositionState::Update, text),
        EngineInput::ImeCommit(text) => composition(CompositionState::End, text),
        EngineInput::ImeDismissed => InputEvent::Ime(ImeEvent::Dismissed),
    }
}

fn composition(state: CompositionState, data: String) -> InputEvent {
    InputEvent::Ime(ImeEvent::Composition(CompositionEvent { state, data }))
}
```

**Step 6: Create the host module**

`crates/paladin-engine/src/host.rs`:

```rust
//! GL surfaces and the per-window tab host.
//!
//! Rendering follows Servo's reference shell: a `WindowRenderingContext`
//! owns the window's GL context; an `OffscreenRenderingContext` derived
//! from it is what every tab in the window paints into; the app's egui
//! shares the window context and blits the offscreen framebuffer below the
//! toolbar through [`TabHost::blit_callback`].

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use dpi::PhysicalSize;
use euclid::{Point2D, Scale, Size2D};
use paladin_core::{EngineEvent, LoadState, PendingTab, TabId};
use raw_window_handle::{DisplayHandle, WindowHandle};
use servo::{
    LoadStatus, OffscreenRenderingContext, RenderingContext, Servo, SoftwareRenderingContext,
    WebView, WebViewBuilder, WindowRenderingContext,
};
use url::Url;

use crate::delegate::DelegateState;
use crate::engine::{Engine, EngineError};
use crate::input::{EngineInput, to_servo};

/// The GL context and surface of one native window.
pub struct GlSurface {
    context: Rc<WindowRenderingContext>,
}

impl GlSurface {
    pub fn new(
        display: DisplayHandle<'_>,
        window: WindowHandle<'_>,
        size: PhysicalSize<u32>,
    ) -> Result<GlSurface, EngineError> {
        let context = WindowRenderingContext::new(display, window, size)
            .map_err(|e| EngineError(format!("creating the window GL context: {e:?}")))?;
        Ok(GlSurface {
            context: Rc::new(context),
        })
    }

    pub fn make_current(&self) -> Result<(), EngineError> {
        self.context
            .make_current()
            .map_err(|e| EngineError(format!("making the GL context current: {e:?}")))
    }

    /// Call on every window resize, in physical pixels, never zero.
    pub fn resize(&self, size: PhysicalSize<u32>) {
        self.context.resize(size);
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        self.context.size()
    }

    /// The glow handle egui renders through.
    pub fn gl(&self) -> Arc<glow::Context> {
        self.context.glow_gl_api()
    }

    /// Make current and bind the window's framebuffer before egui paints.
    pub fn begin_frame(&self) {
        if let Err(e) = self.make_current() {
            log::error!("{e}");
        }
        self.context.prepare_for_rendering();
    }

    /// Swap buffers.
    pub fn present(&self) {
        self.context.present();
    }
}

/// A target rectangle for the blit, in GL window coordinates (origin at
/// the bottom left, physical pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlitRect {
    pub x: i32,
    pub y_from_bottom: i32,
    pub width: i32,
    pub height: i32,
}

/// Copies the active tab's offscreen framebuffer into the window.
pub type BlitFn = Box<dyn Fn(&glow::Context, BlitRect) + Send + Sync>;

/// The engine side of one browser window: its tabs and render targets.
/// A `WebView` is bound to the rendering context it was created with, so
/// tabs never move between hosts.
pub struct TabHost {
    // Dropped first: WebViews must go before their rendering context.
    tabs: HashMap<TabId, WebView>,
    delegate: Rc<DelegateState>,
    servo: Servo,
    /// What tabs paint into: the offscreen context in a window, or a
    /// software context in tests.
    content: Rc<dyn RenderingContext>,
    offscreen: Option<Rc<OffscreenRenderingContext>>,
    surface: Option<GlSurface>,
    hidpi: Cell<f32>,
}

impl TabHost {
    /// A host rendering into `surface`. Tabs start at the surface's size
    /// and shrink to the content area via [`TabHost::resize_content`].
    pub fn new(engine: &Engine, surface: GlSurface, hidpi: f32) -> Result<TabHost, EngineError> {
        surface.make_current()?;
        let offscreen = Rc::new(surface.context.offscreen_context(surface.size()));
        let content: Rc<dyn RenderingContext> = offscreen.clone();
        Ok(TabHost {
            tabs: HashMap::new(),
            delegate: engine.delegate().clone(),
            servo: engine.servo().clone(),
            content,
            offscreen: Some(offscreen),
            surface: Some(surface),
            hidpi: Cell::new(hidpi),
        })
    }

    /// A host with no window, rendering in software. For tests.
    pub fn new_software(engine: &Engine, size: PhysicalSize<u32>) -> Result<TabHost, EngineError> {
        let context = SoftwareRenderingContext::new(size)
            .map_err(|e| EngineError(format!("creating a software GL context: {e:?}")))?;
        context
            .make_current()
            .map_err(|e| EngineError(format!("making the software context current: {e:?}")))?;
        Ok(TabHost {
            tabs: HashMap::new(),
            delegate: engine.delegate().clone(),
            servo: engine.servo().clone(),
            content: Rc::new(context),
            offscreen: None,
            surface: None,
            hidpi: Cell::new(1.0),
        })
    }

    /// The window surface; `None` for a software host.
    pub fn surface(&self) -> Option<&GlSurface> {
        self.surface.as_ref()
    }

    /// Create a visible tab loading `url`.
    pub fn create_tab(&mut self, tab: TabId, url: Url) {
        let webview = WebViewBuilder::new(&self.servo, self.content.clone())
            .url(url)
            .hidpi_scale_factor(Scale::new(self.hidpi.get()))
            .delegate(self.delegate.clone())
            .build();
        self.delegate.register(&webview, tab);
        webview.show();
        self.tabs.insert(tab, webview);
    }

    /// Take over a tab that page content opened. Returns false if the
    /// engine no longer holds it.
    pub fn adopt_tab(&mut self, pending: PendingTab, tab: TabId) -> bool {
        let Some(webview) = self.delegate.take_pending(pending) else {
            return false;
        };
        self.delegate.register(&webview, tab);
        // Events before adoption had no tab id and were dropped; report
        // the current state so the model catches up.
        if let Some(url) = webview.url() {
            self.delegate.push(EngineEvent::UrlChanged { tab, url });
        }
        if let Some(title) = webview.page_title() {
            self.delegate.push(EngineEvent::TitleChanged { tab, title });
        }
        let state = match webview.load_status() {
            LoadStatus::Started | LoadStatus::HeadParsed => LoadState::Loading,
            LoadStatus::Complete => LoadState::Complete,
        };
        self.delegate
            .push(EngineEvent::LoadStateChanged { tab, state });
        webview.show();
        self.tabs.insert(tab, webview);
        true
    }

    /// Drop the engine tab. Servo closes it when the last handle goes.
    pub fn close_tab(&mut self, tab: TabId) {
        if let Some(webview) = self.tabs.remove(&tab) {
            self.delegate.unregister(&webview);
        }
    }

    pub fn has_tab(&self, tab: TabId) -> bool {
        self.tabs.contains_key(&tab)
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    fn with(&self, tab: TabId, f: impl FnOnce(&WebView)) {
        match self.tabs.get(&tab) {
            Some(webview) => f(webview),
            None => log::warn!("no engine tab for {tab:?}"),
        }
    }

    pub fn navigate(&self, tab: TabId, url: Url) {
        self.with(tab, |w| w.load(url));
    }

    pub fn back(&self, tab: TabId) {
        self.with(tab, |w| {
            if w.can_go_back() {
                w.go_back(1);
            }
        });
    }

    pub fn forward(&self, tab: TabId) {
        self.with(tab, |w| {
            if w.can_go_forward() {
                w.go_forward(1);
            }
        });
    }

    pub fn reload(&self, tab: TabId) {
        self.with(tab, |w| w.reload());
    }

    /// Servo 0.7.0 has no API to stop a load; this only logs.
    pub fn stop(&self, tab: TabId) {
        log::warn!("stop requested for {tab:?}, but Servo 0.7.0 cannot stop a load");
    }

    pub fn show(&self, tab: TabId) {
        self.with(tab, |w| w.show());
    }

    pub fn hide(&self, tab: TabId) {
        self.with(tab, |w| w.hide());
    }

    pub fn set_focused(&self, tab: TabId, focused: bool) {
        self.with(tab, |w| w.set_focused(focused));
    }

    /// Resize the tab's render target to the page area, if it changed.
    /// All tabs in a host share one target, so this resizes them all.
    pub fn resize_content(&self, tab: TabId, size: PhysicalSize<u32>) {
        self.with(tab, |w| {
            let current = w.size();
            if current.width as u32 != size.width || current.height as u32 != size.height {
                w.resize(size);
            }
        });
    }

    pub fn set_hidpi(&self, scale: f32) {
        self.hidpi.set(scale);
        for webview in self.tabs.values() {
            webview.set_hidpi_scale_factor(Scale::new(scale));
        }
    }

    pub fn input(&self, tab: TabId, input: EngineInput) {
        self.with(tab, |w| {
            w.notify_input_event(to_servo(input));
        });
    }

    /// Paint the tab into the host's render target.
    pub fn paint(&self, tab: TabId) {
        self.with(tab, |w| {
            if let Err(e) = self.content.make_current() {
                log::error!("making the content context current: {e:?}");
            }
            w.paint();
        });
    }

    /// A callback that blits the offscreen target into the window; `None`
    /// before the first paint or for a software host.
    pub fn blit_callback(&self) -> Option<BlitFn> {
        let render = self.offscreen.as_ref()?.render_to_parent_callback()?;
        Some(Box::new(move |gl, rect: BlitRect| {
            render(
                gl,
                euclid::default::Rect::new(
                    Point2D::new(rect.x, rect.y_from_bottom),
                    Size2D::new(rect.width, rect.height),
                ),
            );
        }))
    }

    pub fn url(&self, tab: TabId) -> Option<Url> {
        self.tabs.get(&tab).and_then(|w| w.url())
    }

    pub fn page_title(&self, tab: TabId) -> Option<String> {
        self.tabs.get(&tab).and_then(|w| w.page_title())
    }

    pub fn load_complete(&self, tab: TabId) -> bool {
        self.tabs
            .get(&tab)
            .is_some_and(|w| w.load_status() == LoadStatus::Complete)
    }
}
```

**Step 7: Replace lib.rs**

`crates/paladin-engine/src/lib.rs`:

```rust
//! Servo glue. This is the only crate in the workspace that imports the
//! `servo` crate. Everything that crosses into the model or the UI is
//! expressed in Paladin's own types: core ids, [`EngineEvent`]s, and
//! [`EngineInput`]. Servo's `WebView`s, ids, and delegate traits never leave
//! this crate.
//!
//! - [`Engine`] builds Servo once per process and owns the event queue.
//! - [`GlSurface`] wraps Servo's GL context on a native window; the app's
//!   egui shares it.
//! - [`TabHost`] owns the tabs of one window, painting them into an
//!   offscreen target that egui blits under the toolbar.
//!
//! [`EngineEvent`]: paladin_core::EngineEvent

mod delegate;
mod engine;
mod host;
mod input;

pub use dpi::PhysicalSize;
pub use keyboard_types;

pub use crate::engine::{Engine, EngineError, Waker};
pub use crate::host::{BlitFn, BlitRect, GlSurface, TabHost};
pub use crate::input::{ButtonAction, EngineInput, KeyInput, MouseButton};
```

**Step 8: Verify it compiles**

Run: `cargo check -p paladin-engine`
Expected: the first run builds every Servo crate's build script and type-checks the tree; `mozjs-sys` prints that it downloads `libmozjs-aarch64-apple-darwin.tar.gz`. Expect 5 to 15 minutes, then `Finished` with no warnings. If it fails inside `rsa`, Step 2 was skipped. If it fails inside `mozjs-sys` with a download error, re-run once (the archive comes from GitHub releases); it is cached in `target/` afterwards.

Run: `cargo clippy -p paladin-engine --all-targets -- -D warnings`
Expected: `Finished`, no warnings.

**Step 9: Commit**

```bash
git add Cargo.lock crates/paladin-engine
git commit -m "feat(engine): embed Servo 0.7.0 behind Engine, GlSurface, and TabHost"
```
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: Headless engine test

**Files:**
- Create: `crates/paladin-engine/tests/headless.rs`

**Step 1: Create the test**

`crates/paladin-engine/tests/headless.rs`:

```rust
//! Starts Servo without a window and loads a page. One test function on
//! purpose: Servo's options are process-global, so a test binary may build
//! exactly one engine.

use std::sync::Arc;
use std::time::{Duration, Instant};

use paladin_core::{EngineEvent, LoadState, TabId};
use paladin_engine::{Engine, PhysicalSize, TabHost, Waker};

struct NoopWaker;

impl Waker for NoopWaker {
    fn wake(&self) {}
}

#[test]
fn loads_a_data_url_headlessly() {
    let dir = tempfile::tempdir().expect("temp dir");
    let engine =
        Engine::new(&dir.path().join("servo"), Arc::new(NoopWaker)).expect("engine starts");
    let mut host =
        TabHost::new_software(&engine, PhysicalSize::new(400, 300)).expect("software host");

    let tab = TabId(1);
    let url = url::Url::parse("data:text/html,<title>Paladin headless</title><p>hello</p>")
        .expect("data URL parses");
    host.create_tab(tab, url);

    let deadline = Instant::now() + Duration::from_secs(120);
    let mut saw_title = false;
    let mut saw_complete = false;
    while !(saw_title && saw_complete) {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the page: title={saw_title} complete={saw_complete}"
        );
        engine.spin();
        for event in engine.drain_events() {
            match event {
                // Servo reports an empty title when navigation starts and
                // the real one once <title> is parsed; wait for the latter.
                EngineEvent::TitleChanged { tab: t, title }
                    if t == tab && title == "Paladin headless" =>
                {
                    saw_title = true;
                }
                EngineEvent::LoadStateChanged {
                    tab: t,
                    state: LoadState::Complete,
                } if t == tab => saw_complete = true,
                EngineEvent::FrameReady(t) if t == tab => host.paint(t),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(host.page_title(tab).as_deref(), Some("Paladin headless"));
    assert!(host.load_complete(tab));

    host.close_tab(tab);
    assert!(host.is_empty());
    drop(host);
    // Dropping the engine shuts Servo down and waits for it to finish.
    drop(engine);
}
```

**Step 2: Run it**

Run: `cargo test -p paladin-engine`
Expected: the first run compiles Servo in full (about 5 minutes on the Mac), then:

```
     Running tests/headless.rs (target/debug/deps/headless-...)

running 1 test
test loads_a_data_url_headlessly ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Servo logs a few `WARN` lines about the software renderer; that is normal. If the test times out after 120 s, run with `RUST_LOG=info cargo test -p paladin-engine -- --nocapture` and check that `Engine::new` got past `install_crypto_provider` and that `TitleChanged` events arrive (an empty title first, then `Paladin headless`).

**Step 3: Commit**

```bash
git add crates/paladin-engine/tests/headless.rs
git commit -m "test(engine): load a data URL headlessly through Servo"
```
<!-- END_TASK_2 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_3 -->
### Task 3: CI builds Servo on both runners

**Files:**
- Replace: `.github/workflows/ci.yml`
- Modify: `CHANGELOG.md`

The workflow gains a 180-minute timeout and, on Windows, the `cargo xtask angle-dlls debug` step between building and testing, because the engine's test binary links `libEGL.dll` and loads it by name.

**Step 1: Replace the workflow**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  # sccache cannot cache incremental artifacts.
  CARGO_INCREMENTAL: "0"
  SCCACHE_GHA_ENABLED: "true"
  RUSTC_WRAPPER: sccache

jobs:
  check:
    name: ${{ matrix.os }}
    runs-on: ${{ matrix.os }}
    # Servo is a long build; the first run on each runner can exceed an hour.
    timeout-minutes: 180
    strategy:
      fail-fast: false
      matrix:
        include:
          # Apple Silicon, like the development Mac.
          - os: macos-15
            check-icons: true
          - os: windows-2025
            check-icons: false
    steps:
      - uses: actions/checkout@v7

      # dtolnay/rust-toolchain does not read rust-toolchain.toml; keep the
      # version in sync with it so rustup installs one toolchain, not two.
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: 1.97.1
          components: rustfmt, clippy

      # Servo's WebIDL code generation runs Python at build time.
      - uses: actions/setup-python@v6
        with:
          python-version: "3.12"

      # bindgen (used by Servo's ANGLE bindings) loads libclang at build time.
      - name: Install LLVM (Windows)
        if: runner.os == 'Windows'
        uses: KyleMayes/install-llvm-action@v2
        with:
          version: "20.1"
      - name: Set LIBCLANG_PATH (Windows)
        if: runner.os == 'Windows'
        shell: pwsh
        run: echo "LIBCLANG_PATH=$env:LLVM_PATH\bin" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append

      - uses: Swatinem/rust-cache@v2
        with:
          cache-on-failure: true
      - uses: mozilla-actions/sccache-action@v0.0.11

      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo build --workspace --all-targets

      # The engine links libEGL.dll and loads it by name at runtime; the
      # test binaries need it beside them.
      - name: Copy ANGLE DLLs next to the binaries (Windows)
        if: runner.os == 'Windows'
        run: cargo xtask angle-dlls debug

      - run: cargo test --workspace

      # tiny-skia's output is not guaranteed identical across CPU
      # architectures, so only the Apple Silicon runner diffs the icons.
      - name: Icons are up to date
        if: matrix.check-icons
        run: |
          cargo xtask icons
          git diff --exit-code -- assets/icons

      - name: sccache stats
        shell: bash
        run: ${SCCACHE_PATH} --show-stats
```

**Step 2: Record the change**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, add after the last bullet:

```markdown
- Engine crate embedding Servo 0.7.0: one engine per process, a GL surface
  per window, a tab host that paints tabs offscreen, and a headless test that
  loads a page without a window.
```

**Step 3: Commit and push**

```bash
git add .github/workflows/ci.yml CHANGELOG.md
git commit -m "ci: build Servo on both runners with a longer timeout and Windows ANGLE DLLs"
git push
```

**Step 4: Watch the first slow CI run**

Run: `gh run watch --exit-status`
Expected: both jobs succeed. The first run compiles Servo on each runner with a cold cache; allow up to an hour each. If the Windows job fails:

- `libclang` errors from `mozangle`: confirm the `Set LIBCLANG_PATH` step ran and printed nothing; `KyleMayes/install-llvm-action` exports `LLVM_PATH`.
- `The specified module could not be found` when running tests: the `angle-dlls` step did not find `target\debug\build\mozangle-*\out\libEGL.dll`; open the `cargo build` log and check `mozangle` built with the `build_dlls` feature (it does when `servo/no-wgl` is on, which the manifest sets for Windows).
- Python errors from `servo-script-bindings`: the `setup-python` step is missing or an older Python shadowed it.

Fix with a `ci:` commit, push, and watch again. Phase 3 is not done until both runners pass the headless test.

Fallback, only after two fix attempts: the Windows runner has no GPU, and `SoftwareRenderingContext` under ANGLE was verified only on macOS. If the Windows test still fails for a rendering-context reason (not a build or DLL reason), mark the test `#[cfg_attr(windows, ignore = "software rendering context on GPU-less Windows runners; tracked in README Todo")]`, add a Todo bullet to `README.md`, commit, and report it in the phase summary. The macOS run and the Windows build remain required.
<!-- END_TASK_3 -->

---

## Phase 3 verification

- `cargo test -p paladin-engine` passes locally with `1 passed`.
- `cargo test --workspace` passes (49 tests across crates) and `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `gh run list --limit 1` shows the latest CI run succeeded on `macos-15` and `windows-2025`.
- `Cargo.lock` pins `pkcs1 0.8.0-rc.4`, and `CLAUDE.md`'s Conventions section already documents why.
