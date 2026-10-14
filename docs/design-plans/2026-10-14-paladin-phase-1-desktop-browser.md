# Paladin Phase 1: Desktop Browser Design

## Summary

Paladin is a desktop web browser for macOS and Windows, written in Rust, that
renders web pages with Servo, the experimental Rust browser engine. Its window
chrome (tab strip, address bar, back, forward, and reload buttons, and a
settings window) is drawn with egui, a Rust immediate-mode UI library, on top
of winit for windowing. There is no HTML in the chrome. Phase 1 delivers a
working multi-window, multi-tab browser with a logo, installable packages, and
CI, and lays the groundwork for remappable hotkeys and profiles in Phase 2.

The approach is to keep Servo contained. A Cargo workspace splits the code
into a Servo-free core crate (tab and window model, named commands, keymap,
settings, profile paths), an engine crate that is the only code allowed to
import Servo, and a binary crate with the winit app and egui UI. The UI only
emits `Command` values. A pure `dispatch` function turns them into state
changes plus `EngineAction`s for the engine. In the other direction, Servo
callbacks become `EngineEvent`s that update the model. Because every user
action is a named command, Phase 2 hotkey remapping is a data change rather
than a rewrite. The rendering and event-loop pattern is borrowed from Servo's
own reference shell: Servo paints each tab offscreen, and egui blits that image
beneath the toolbar. The work is split into six implementation phases, starting
with conventions, logo, and CI, and ending with packaging and verification on
Windows.

## Definition of Done

Phase 1 is a desktop browser built on the Servo engine that Thomas can run on
macOS and Windows, with the project conventions used in geniami and
novelstrength in place from the first commit.

### Deliverables

- A git repository in this directory holding a Rust Cargo workspace for
  Paladin. Servo is confined to a single engine crate. The tab and window
  model, command system, settings, and egui UI never import Servo.
- A desktop browser with one or more windows, each with a tab strip, address
  bar, back, forward, and reload, rendering pages through Servo. A basic
  settings window exists so Phase 2 hotkey and profile editors have a home.
- Every user action is a named command, so Phase 2 hotkey remapping is a data
  change, not a rewrite. Settings persist to disk in a per-user config
  directory.
- The Paladin logo, a black and white knight with a lance on a pegasus charging
  from bottom left to top right, authored as an SVG and checked in, with
  generated icon files used as the window, dock, and app icon.
- Packaged macOS `.app` and Windows installer built with cargo-packager,
  locally and in GitHub Actions CI, which also builds and tests on macOS and
  Windows runners.
- Project conventions matching geniami and novelstrength: `README.md` with a
  stack table, layout, local setup, and a todo list; `CHANGELOG.md` in Keep a
  Changelog format; `CLAUDE.md` with the Definition of Done; `ROADMAP.md` with
  numbered phases; design and implementation plans under `docs/`. Conventional
  Commits on the current branch.

### Success criteria

- On Thomas's Mac, `cargo run` opens a window with the icon, a URL typed in
  the address bar renders, and tabs can be opened and closed and the settings
  window opened.
- The same works on Thomas's Windows machine, verified by Thomas.
- CI is green on both platforms and uploads the packaged artifacts.

### Out of scope for Phase 1

Hotkey remapping UI, profiles, tab multi-select and moving, extensions,
bookmarks, history, downloads UI, sync, auto-update. `ROADMAP.md` records
hotkeys, profiles, and tab moves as Phase 2, and extensions as a later phase
with the Servo risk stated.

### Decisions already made during clarification

- **Engine:** depend on the `servo` crate directly (crates.io 0.7.0, git for
  patches). No engine trait. Isolation is by crate boundary, not abstraction.
- **UI:** pure Rust, egui on winit, the same stack as Servo's reference shell.
  No HTML in the chrome.
- **Windows:** verified on Thomas's Windows machine, plus a CI build.
- **Packaging:** cargo-packager for `.app` and Windows installer.
- **Verso** (an earlier Servo-based browser) is archived and not a starting
  point.

### Servo facts that constrain the design (verified 2026-10-14)

- One Servo instance per process in practice: preferences are process-global
  and the accessibility layer assumes a single instance. Storage (cookies,
  local storage) is per instance, not per tab. There is no profile or
  container concept. Phase 2 profile isolation therefore means one engine
  process per profile.
- A `WebView` is bound to the `RenderingContext` (window) it was created in and
  cannot be moved. Moving a tab between windows or profiles means creating a
  new `WebView` and reloading, which Thomas accepted.
- No WebExtension support. The only related issue has been open since 2016.
  Real Chrome or Firefox extensions are a bet on Servo's roadmap and are the
  most likely reason Paladin would ever change engines.
- Servo pins stable Rust 1.97.1 (`rust-toolchain.toml`); minimum 1.88.
  macOS build needs Xcode command line tools, Homebrew (cmake, pkg-config),
  uv, and a GStreamer download. Windows needs Visual Studio build tools
  (MSVC v143, Windows SDK 10.0.19041+, C++ ATL), CMake, LLVM, Ninja, and
  GStreamer.

## Glossary

- **Servo**: Experimental web browser engine written in Rust. Paladin depends
  on the `servo` crate directly and confines it to `paladin-engine`.
- **SpiderMonkey**: Mozilla's JavaScript engine, which Servo embeds. Its build
  is the main reason compile times are long.
- **servoshell**: Servo's own reference browser shell (`ports/servoshell`).
  Paladin copies its rendering and event-loop pattern but not its structure.
- **Verso**: An earlier Servo-based browser. Archived; not a starting point.
- **egui**: Pure-Rust immediate-mode GUI library. It draws Paladin's tab
  strip, toolbar, and settings window.
- **EguiGlow**: The egui integration that renders through OpenGL via glow and
  shares the engine's GL context.
- **winit**: Cross-platform Rust windowing and event-loop library. Paladin's
  `App` implements its `ApplicationHandler`, and it supplies the raw window
  handles Servo renders into.
- **surfman**: Rust library for native GL surfaces and contexts. Servo uses
  it, so its version must match Servo's.
- **glow**: Rust OpenGL bindings used by egui_glow and Servo's shell. Pinned
  alongside winit, egui, and surfman.
- **WebView**: Servo's object for one page, which Paladin shows as a tab. It
  is bound to the `RenderingContext` of the window that created it and cannot
  be moved; moving a tab means creating a new `WebView` and reloading.
- **RenderingContext**: Servo's abstraction for a render target.
  `WindowRenderingContext` wraps the native window surface;
  `SoftwareRenderingContext` is used for headless tests.
- **Offscreen framebuffer / OffscreenRenderingContext**: A render target
  derived from the window context that tabs paint into. egui then blits it
  into the content area as a background paint callback, which is how egui
  chrome and Servo content share one window.
- **PaintCallback**: egui mechanism for running custom GL drawing in the
  middle of a frame. Used to blit the Servo output.
- **HiDPI**: High-pixel-density displays where physical pixels differ from
  logical points. Paladin sets egui's zoom so its points match Servo's scale
  and offsets input coordinates accordingly.
- **WebViewDelegate / ServoDelegate**: Servo callback traits for page events
  (title, URL, load state, new-tab requests) and engine-level events (errors).
  `DelegateState` implements both and converts callbacks into `EngineEvent`s.
- **EventLoopWaker**: A closure that wakes the winit event loop from Servo's
  threads by sending a user event, so the app can wait instead of poll.
- **Command / EngineEvent / EngineAction**: Paladin's three boundary types.
  `Command` is a named user action, `EngineEvent` a notification from the
  engine, and `EngineAction` a request that `dispatch` returns for the app to
  forward to the engine. They keep `dispatch` pure and testable.
- **Keymap**: Map from key chord to `Command`, with built-in defaults. Phase 2
  adds persistence and an editor.
- **Profile**: A named set of per-user state (settings, keymap, Servo cookies
  and storage) under a per-OS directory. Servo has no profile concept, so
  Phase 2 isolation means one engine process per profile.
- **xtask**: Convention for a workspace helper crate run as
  `cargo xtask <task>`, used here to generate icons from the SVG.
- **cargo-packager**: Tool that builds installable bundles from Cargo
  metadata: `.app` and `.dmg` on macOS, an installer on Windows.
- **NSIS**: Nullsoft Scriptable Install System, the Windows installer format
  cargo-packager produces.
- **GStreamer**: Multimedia framework Servo uses for media playback. The
  `media-gstreamer` feature is off in Phase 1, so there is no media playback
  and no GStreamer install is required.
- **no-wgl**: Servo feature that uses ANGLE's EGL instead of Windows' native
  OpenGL interface. Enabled on Windows, as Servo's shell does.
- **baked-in-resources, js_jit, multiprocess**: Servo cargo features that
  embed resource files in the binary, enable the JavaScript JIT, and enable
  Servo's multiprocess mode.
- **WebExtension**: The Chrome and Firefox browser extension API. Servo does
  not support it. This is the main long-term risk and the likeliest reason to
  revisit the engine choice.
- **Keep a Changelog**: Format for `CHANGELOG.md` with an `Unreleased`
  section and categorized entries.
- **Conventional Commits**: Commit message convention (`feat:`, `fix:`,
  `docs:`, `chore:`).
- **geniami / novelstrength**: Sibling projects in `../` whose repository
  conventions Paladin copies. They are TypeScript monorepos, so their tooling
  does not carry over.
- **sccache / rust-cache**: Compiler-output and dependency caching used in CI
  to shorten Servo's long builds.
- **directories**: Rust crate that resolves per-OS config and data
  directories for `Profile::resolve`.
- **AppKit**: macOS native UI framework, used to set the dock icon so
  `cargo run` shows the logo.

## Architecture

Three-crate Cargo workspace plus an `xtask` helper. Servo is confined to one
crate; the model and UI never import it.

```
paladin/
  Cargo.toml                 workspace; pins winit, egui, glow, surfman to the
                             versions Servo's shell uses
  rust-toolchain.toml        stable channel Servo tests with (1.97.1 today)
  crates/
    paladin-core/            model, commands, keymap, settings, profile paths
                             (no servo, no egui, no winit)
    paladin-engine/          Servo glue: Engine, TabHost, delegate -> EngineEvent
    paladin/                 binary: winit app, egui chrome, dispatch loop,
                             cargo-packager metadata
  xtask/                     `cargo xtask icons`: logo.svg -> icns/ico/png
  assets/
    logo.svg                 the knight-on-pegasus mark
    icons/                   generated, checked in
  docs/
    design-plans/
    implementation-plans/
  .github/workflows/ci.yml   macOS + Windows matrix
  README.md CHANGELOG.md CLAUDE.md ROADMAP.md
```

### Data flow

```
egui chrome  --Command-->  dispatch  --engine call-->  paladin-engine (Servo)
     ^                        |                              |
     |                   core::apply                  WebViewDelegate
     |                        v                              v
     +---- renders from -- core::Browser  <--EngineEvent--  event queue
```

One loop iteration in the `paladin` binary, in wait mode with wakeups:

1. Forward the winit event to egui if the cursor is over the chrome or a
   dialog has focus, otherwise to the active tab via the engine.
2. Drain `EngineEvent`s from the engine queue into `core::apply`.
3. Run the egui frame: tab strip and toolbar drawn from `core::Browser`,
   emitting `Command`s.
4. Dispatch each `Command`: pure state changes go to `core::apply`; engine
   actions (navigate, back, resize, show, hide, close) go to the `TabHost`.
5. Engine paints the active tab into its offscreen framebuffer; egui blits it
   into the rect below the toolbar as a background paint callback; present.

### paladin-core

Plain Rust library. Types:

- `Browser { profile: Profile, windows: Vec<Window>, settings: Settings }`
- `Window { id: WindowId, tabs: Vec<Tab>, active: TabId }`
- `Tab { id: TabId, url: Url, title: String, favicon: Option<Vec<u8>>,
  load: LoadState, can_go_back: bool, can_go_forward: bool, status: String }`
- `WindowId`, `TabId`: newtypes over `u64`, allocated by core. Servo ids never
  leave the engine crate.
- `Profile { name: String, dir: PathBuf }` with `Profile::resolve(name)`
  computing the per-OS directory.
- `Settings`: serde struct, TOML on disk, every field defaulted. Phase 1
  fields: `homepage`, `new_tab_url`, `restore_windows_on_launch`.
- `Keymap`: map from key chord to `Command`, with built-in defaults
  (Cmd/Ctrl+T, W, L, R, Shift+[ ], etc.).

Contracts crossing crate boundaries:

```rust
pub enum Command {
    NewTab { window: WindowId, url: Option<Url> },
    CloseTab(TabId),
    ActivateTab(TabId),
    NextTab(WindowId),
    PreviousTab(WindowId),
    Navigate { tab: TabId, url: Url },
    Back(TabId),
    Forward(TabId),
    Reload(TabId),
    Stop(TabId),
    FocusAddressBar(WindowId),
    NewWindow,
    CloseWindow(WindowId),
    OpenSettings,
    Quit,
}

pub enum EngineEvent {
    UrlChanged { tab: TabId, url: Url },
    TitleChanged { tab: TabId, title: String },
    LoadStateChanged { tab: TabId, state: LoadState },
    HistoryChanged { tab: TabId, can_go_back: bool, can_go_forward: bool },
    StatusChanged { tab: TabId, text: String },
    FaviconChanged { tab: TabId, png: Vec<u8> },
    NewTabRequested { opener: TabId, url: Url },
    TabClosed(TabId),
    TabCrashed { tab: TabId, reason: String },
    FrameReady(TabId),
}

pub enum LoadState { Idle, Loading, Complete }

pub fn apply(browser: &mut Browser, event: &EngineEvent);
pub fn dispatch(browser: &mut Browser, cmd: &Command) -> Vec<EngineAction>;
```

`EngineAction` is the small set of things the app must forward to the engine
after a command (create tab, navigate, back, forward, reload, stop, show, hide,
close). Keeping it a returned value keeps `dispatch` pure and testable.

### paladin-engine

Wraps the `servo` crate (crates.io 0.7.0; git for patches).

- `Engine::new(profile_servo_dir, waker)` builds Servo via `ServoBuilder` with
  `Opts.config_dir` set to `<profile>/servo/`, installs the `ServoDelegate`,
  and owns the shared event queue. One per process.
- `TabHost::new(&engine, display_handle, window_handle, size)` creates the
  `WindowRenderingContext` over the native surface and the derived
  `OffscreenRenderingContext` that tabs render into. One per browser window.
- `TabHost` methods take core ids and plain types: `create_tab(TabId, Url)`,
  `navigate`, `back`, `forward`, `reload`, `stop`, `show`, `hide`,
  `resize(PhysicalSize)`, `set_hidpi(f32)`, `input(TabId, InputEvent)`,
  `paint(TabId)`, `blit_callback(rect)`, `present()`, `close_tab(TabId)`.
  Internally a `HashMap<TabId, servo::WebView>` plus the reverse map.
- `DelegateState` implements `servo::WebViewDelegate` and `ServoDelegate`
  once; each callback becomes an `EngineEvent` pushed onto the queue, keyed by
  the core `TabId` looked up from the Servo `WebViewId`.
- `EngineInput` is a Paladin-defined input enum (mouse, wheel, key, IME) the
  app builds from winit events; the engine converts it to Servo's
  `InputEvent`. This keeps winit out of the engine and Servo out of the app.
- `EventLoopWaker` is a boxed closure supplied by the app that sends a winit
  user event.
- Features: `servo` with `default-features = false` plus `baked-in-resources`,
  `default_web_features`, `js_jit`, `multiprocess`; `no-wgl` on Windows.
  `media-gstreamer` is off in Phase 1.

### paladin (binary)

- `App` implements winit's `ApplicationHandler<AppEvent>`; `AppEvent::Wake`
  is what the waker sends.
- `BrowserWindow` owns a winit window, a `TabHost`, an `EguiGlow` sharing the
  engine's GL context, and the `WindowId`. `SettingsWindow` owns a winit
  window and egui only.
- `chrome` module: tab strip (favicon, title, close, "+"), toolbar (back,
  forward, reload/stop, address bar). Pure functions of `&Browser` returning
  `Vec<Command>`.
- `input` module: winit event to `EngineInput`, with the toolbar-height offset
  and HiDPI scaling the Servo shell uses (set egui zoom so its points match
  Servo's scale).
- `icon` module: window icon from the embedded PNG; on macOS also sets the dock
  icon through AppKit so `cargo run` shows the logo.
- `main`: parses `--profile <name>` (default `default`) and optional URL
  arguments, resolves the profile directory, loads settings, builds the engine,
  opens the first window.
- `[package.metadata.packager]`: identifier `org.paladin.software`, icons,
  `.app`/`.dmg` on macOS, NSIS on Windows.

### On-disk layout

| OS | Profile directory |
|---|---|
| macOS | `~/Library/Application Support/Paladin/profiles/<name>/` |
| Windows | `%APPDATA%\Paladin\profiles\<name>\` |

Inside: `settings.toml`, `keymap.toml` (Phase 2), `servo/` (Servo config
dir: cookies, local storage, prefs). Phase 2 adds `profiles.toml` one level up.

## Existing Patterns

This directory is empty; there is no Paladin code to follow. Two sources of
patterns were used:

**Sibling projects (`../geniami`, `../novelstrength`).** Paladin adopts their
repository conventions verbatim: `README.md` with a stack table, layout tree,
local setup, and a todo list; `CHANGELOG.md` in Keep a Changelog format with an
`Unreleased` section; `CLAUDE.md` with a freshness date, commands, conventions,
and a three-part Definition of Done (tests, checks, docs); `ROADMAP.md` with
numbered phases; `docs/design-plans/` and `docs/implementation-plans/`;
Conventional Commits on the current branch, no feature branches unless asked.
Those projects are TypeScript monorepos, so their tooling (pnpm, Biome, Vitest)
does not carry over; the Rust equivalents are `cargo fmt`, `cargo clippy`,
`cargo test`.

**Servo's reference shell (`ports/servoshell` in servo/servo).** The rendering
and event-loop pattern is borrowed exactly: `WindowRenderingContext` plus a
derived `OffscreenRenderingContext`, egui sharing the GL context through
`EguiGlow`, Servo output blitted via a background `PaintCallback`, tabs shown
and hidden rather than moved, one rendering context and egui instance per
window, `ControlFlow::Wait` with an `EventLoopWaker` that sends a winit user
event. Paladin diverges in structure: servoshell keeps Servo types throughout
its `RunningAppState`; Paladin confines them to `paladin-engine` and drives the
UI from a Servo-free model.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Workspace, conventions, logo, CI skeleton
**Goal:** A buildable, documented, continuously integrated repository with the
brand assets in place, before any Servo code exists.

**Components:**
- `Cargo.toml` workspace with `crates/paladin-core`, `crates/paladin-engine`,
  `crates/paladin`, `xtask` as empty-but-compiling crates
- `rust-toolchain.toml`, `rustfmt.toml`, `clippy` config, `.gitignore`
- `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `ROADMAP.md`
- `assets/logo.svg` and `xtask` `icons` task producing `assets/icons/`
  (`paladin.icns`, `paladin.ico`, `paladin.png` sizes); generated files
  committed
- `.github/workflows/ci.yml`: macOS and Windows matrix running fmt, clippy,
  test, build, with `rust-cache` and `sccache`

**Dependencies:** None.

**Done when:** `git init` done and first commit made; `cargo build` and
`cargo test` succeed on macOS; `cargo xtask icons` regenerates identical icon
files; CI is green on both runners.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Core model
**Goal:** The Servo-free model, commands, keymap, settings, and profile
resolution, fully unit-tested.

**Components:**
- `crates/paladin-core/src/model.rs`: `Browser`, `Window`, `Tab`, ids,
  `LoadState`
- `crates/paladin-core/src/command.rs`: `Command`, `EngineAction`, `dispatch`
- `crates/paladin-core/src/event.rs`: `EngineEvent`, `apply`
- `crates/paladin-core/src/keymap.rs`: `KeyChord`, `Keymap`, defaults per OS
- `crates/paladin-core/src/settings.rs`: `Settings` with TOML load/save
- `crates/paladin-core/src/profile.rs`: `Profile::resolve` using `directories`

**Dependencies:** Phase 1.

**Done when:** Tests cover tab creation, activation, close with
next-active selection, next/previous tab wraparound, every `EngineEvent`
mutating state, keymap resolving default chords, settings round-trip with
missing fields defaulting, profile paths per OS. `cargo test -p paladin-core`
passes in CI.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Engine crate
**Goal:** Servo builds inside the workspace and loads a page headlessly.

**Components:**
- `crates/paladin-engine/Cargo.toml`: `servo` dependency with the feature set
  above; platform-specific features
- `crates/paladin-engine/src/engine.rs`: `Engine::new`, waker, event queue
- `crates/paladin-engine/src/host.rs`: `TabHost` over window and offscreen
  rendering contexts, plus a software-context constructor for tests
- `crates/paladin-engine/src/delegate.rs`: `DelegateState` mapping Servo
  callbacks to `EngineEvent`
- `crates/paladin-engine/src/input.rs`: `EngineInput` and conversion to
  Servo `InputEvent`
- `crates/paladin-engine/tests/headless.rs`: start engine with a
  `SoftwareRenderingContext`, load a `data:text/html` page, assert
  `TitleChanged` and `LoadStateChanged(Complete)` arrive

**Dependencies:** Phase 2.

**Done when:** The headless test passes on macOS locally and on both CI
runners. This is the first slow CI run; the README records expected build
times and the macOS and Windows prerequisites.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: Single browser window
**Goal:** `cargo run` opens a window with the logo, tabs, and a toolbar, and
renders web pages.

**Components:**
- `crates/paladin/src/main.rs`: CLI (`--profile`, URLs), profile and
  settings load, engine creation, event loop start
- `crates/paladin/src/app.rs`: `App` as winit `ApplicationHandler`, dispatch
  loop steps 1 to 5 from the Architecture section
- `crates/paladin/src/window.rs`: `BrowserWindow` with `TabHost` and
  `EguiGlow`
- `crates/paladin/src/chrome/`: tab strip and toolbar
- `crates/paladin/src/input.rs`: winit to `EngineInput`, toolbar offset,
  HiDPI handling
- `crates/paladin/src/icon.rs`: window icon and macOS dock icon

**Dependencies:** Phase 3.

**Done when:** On macOS, `cargo run` shows the Paladin icon, a URL typed in
the address bar loads and renders, back/forward/reload work, tabs open and
close, resizing and HiDPI changes keep the page aligned under the toolbar.
Manual checklist in `README.md` is written and passes.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: Multiple windows, settings window, keyboard shortcuts
**Goal:** The rest of the Phase 1 chrome, driven by the keymap.

**Components:**
- `crates/paladin/src/app.rs`: window registry keyed by winit window id and
  core `WindowId`; `NewWindow`, `CloseWindow`, quit when last window closes
- `crates/paladin/src/settings_window.rs`: egui-only window editing
  `Settings`, saving on change
- `crates/paladin/src/input.rs`: key chord extraction and `Keymap` lookup
  before forwarding keys to the engine
- `crates/paladin/src/chrome/`: window menu entries for new window and
  settings

**Dependencies:** Phase 4.

**Done when:** Two windows render independently; settings edits persist across
restarts; every `Command` is reachable by its default chord; closing the last
window exits cleanly with no Servo shutdown errors. Manual checklist extended
and passing on macOS.
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: Packaging, CI artifacts, Windows verification
**Goal:** Installable builds on both platforms and the docs that make them
reproducible.

**Components:**
- `crates/paladin/Cargo.toml` `[package.metadata.packager]` with identifier
  `org.paladin.software`, icons, product name
- `.github/workflows/ci.yml`: release build and `cargo packager` step
  uploading `.app`/`.dmg` and NSIS `.exe` artifacts
- `README.md`: Windows prerequisites (Visual Studio components, CMake, LLVM,
  Ninja), packaging commands, known limitations (no media playback)
- `CHANGELOG.md`, `ROADMAP.md`, `CLAUDE.md` brought current

**Dependencies:** Phase 5.

**Done when:** CI uploads both artifacts; the `.app` runs on Thomas's Mac
from the bundle; Thomas runs the Windows installer and the manual checklist on
his Windows machine and reports it passing. Definition of Done for Phase 1 is
met.
<!-- END_PHASE_6 -->

## Additional Considerations

**Servo version pinning.** The `servo` crate is pinned to an exact version.
winit, egui, egui-winit, egui_glow, glow, and surfman are pinned to the exact
versions in that Servo release's shell, because they share a GL context and
raw window handles. Bumping Servo means bumping all of them together; that is
a single documented step in `CLAUDE.md`.

**Compile time.** Servo plus SpiderMonkey is a long compile. Local clean
builds take 15 to 30 minutes on a recent Mac; the first CI run on each OS is
expected to exceed 30 minutes even with caching. `sccache` and `rust-cache`
keep subsequent runs shorter. Nothing in Phase 1 may be gated on CI speed.

**Engine failure handling.** `TabCrashed` replaces the tab's content with an
egui-rendered error panel and a reload button; the tab stays in the model so
its URL is not lost. Servo's `notify_error` from the `ServoDelegate` is logged
and shown in the status area. If the engine fails to start at all, the app
exits with a message naming the profile directory and the error.

**Phase 2 readiness, deliberately not built now.** Hotkeys: `Keymap` already
exists; Phase 2 adds `keymap.toml` persistence and an egui editor. Profiles:
all state is already under `profiles/<name>/`; Phase 2 adds `profiles.toml`,
a launcher that spawns one Paladin process per profile, color-coded chrome,
and the sharing/isolated resolution rule in `paladin-core`. Tab multi-select
and moves: the tab strip emits `Command`s and tabs are model objects; moving
a tab to another window or profile is a close plus a create with the same URL
and history replay, which Servo requires anyway.

**Extensions risk.** Servo has no WebExtension support and no scheduled work
toward it. `ROADMAP.md` lists extensions as Phase 3 with the explicit note
that it depends on upstream Servo and is the most likely reason to revisit
the engine choice. The crate isolation in this design is what limits the cost
of that revisit.
