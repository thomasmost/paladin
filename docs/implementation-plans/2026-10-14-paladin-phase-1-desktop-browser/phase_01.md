# Paladin Phase 1 Desktop Browser Implementation Plan

**Goal:** A desktop browser for macOS and Windows that renders pages with Servo under an egui chrome, with multiple windows and tabs, a settings window, a logo, installable packages, and CI on both platforms.

**Architecture:** Three-crate Cargo workspace plus an `xtask` helper. `paladin-core` holds the Servo-free model, named commands, keymap, settings, and profile paths. `paladin-engine` is the only crate that imports Servo and exposes `Engine`, `GlSurface`, and `TabHost`. The `paladin` binary runs the winit event loop and draws the chrome with egui, blitting Servo's offscreen framebuffer under the toolbar. The UI only emits `Command`s; the pure `dispatch` and `apply` functions in core turn commands and engine events into state changes plus `EngineAction`s.

**Tech Stack:** Rust 1.97.1 (edition 2024), servo 0.7.0, winit 0.30.13, egui / egui-winit / egui_glow 0.34.3, glow 0.17.0, surfman 0.14.0, keyboard-types 0.8.3, directories 6, toml 1.1, clap 4, resvg 0.48.1, icns 0.5, ico 0.6, cargo-packager 0.11.8, GitHub Actions on `macos-15` and `windows-2025`.

**Scope:** 6 phases from original design (this file is phase 1 of 6)

**Codebase verified:** 2026-10-14. The repository holds only `docs/design-plans/2026-10-14-paladin-phase-1-desktop-browser.md` and one commit on `main`; there is no code, no `Cargo.toml`, and no GitHub remote. Every Rust file in this plan was compiled, linted with `cargo clippy --all-targets -- -D warnings`, and formatted with the pinned toolchain before being written here. The engine crate was type-checked, linted, built, and its headless test run against the published `servo` 0.7.0 crate on the development Mac.

---

## Conventions for every task in every phase

- Run every command from the repository root, `/Users/thomas/vibes/paladin`.
- Commit to the current branch (`main`) with Conventional Commits. No feature branches.
- "Create" means the file does not exist yet; write exactly the content shown. "Replace" means overwrite the whole file with the content shown. Code blocks are complete files unless a step says otherwise.
- Before every commit run `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`. All three must be clean. A step that expects a failure says so explicitly.
- The first `cargo` command after Task 1 installs Rust 1.97.1 through rustup (the machine has 1.98 stable active; `rust-toolchain.toml` pins 1.97.1). That is expected.
- The development Mac already has: rustup, Homebrew `python3` 3.14 ahead of Apple's 3.9 on `PATH`, Xcode Command Line Tools (which provide `libclang`), `pkg-config`, `curl`, and `gh` logged in as `thomasmost`. Nothing else needs installing in Phase 1.

---

## Phase 1: Workspace, conventions, logo, CI skeleton

**Goal:** A buildable, documented, continuously integrated repository with the brand assets in place, before any Servo code exists.

**Done when:** `cargo build --workspace` and `cargo test --workspace` succeed on macOS; `cargo xtask icons` regenerates byte-identical icon files; the GitHub repository `thomasmost/paladin` exists and CI is green on both runners.

<!-- START_SUBCOMPONENT_A (tasks 1-2) -->
<!-- START_TASK_1 -->
### Task 1: Cargo workspace with four empty crates

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`
- Create: `clippy.toml`
- Create: `.gitignore`
- Create: `.cargo/config.toml`
- Create: `crates/paladin-core/Cargo.toml`
- Create: `crates/paladin-core/src/lib.rs`
- Create: `crates/paladin-engine/Cargo.toml`
- Create: `crates/paladin-engine/src/lib.rs`
- Create: `crates/paladin/Cargo.toml`
- Create: `crates/paladin/src/main.rs`
- Create: `xtask/Cargo.toml`
- Create: `xtask/src/main.rs`

**Step 1: Create the workspace manifest and tool configuration**

`Cargo.toml`:

```toml
[workspace]
members = ["crates/paladin-core", "crates/paladin-engine", "crates/paladin", "xtask"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.88"
publish = false

[workspace.dependencies]
# Pinned to the versions Servo 0.7.0's own shell uses. They share a GL
# context and raw window handles with Servo, so bump them together with
# `servo` (see CLAUDE.md).
winit = "=0.30.13"
egui = "=0.34.3"
egui-winit = { version = "=0.34.3", default-features = false }
egui_glow = { version = "=0.34.3", features = ["winit"] }
glow = "=0.17.0"
surfman = "=0.14.0"
raw-window-handle = "0.6"
dpi = "0.1"
euclid = "0.22"
keyboard-types = { version = "0.8.3", features = ["serde"] }
url = { version = "2.5", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
log = "0.4"
```

`rust-toolchain.toml`:

```toml
[toolchain]
# The stable channel Servo 0.7.0 pins. Bump together with the servo crate.
channel = "1.97.1"
components = ["clippy", "rustfmt"]
```

`rustfmt.toml`:

```toml
style_edition = "2024"
```

`clippy.toml`:

```toml
msrv = "1.88"
```

`.gitignore`:

```text
/target/
/dist/
*.log
.env
.DS_Store
```

`.cargo/config.toml` (this is what makes `cargo xtask <task>` work):

```toml
[alias]
xtask = "run --package xtask --"
```

**Step 2: Create the core crate**

`crates/paladin-core/Cargo.toml` (the dependencies are used from Phase 2; declaring them now is harmless):

```toml
[package]
name = "paladin-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
description = "Servo-free browser model: windows, tabs, commands, keymap, settings, profiles"

[dependencies]
directories = "6.0.0"
keyboard-types = { workspace = true }
serde = { workspace = true }
toml = "1.1"
url = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

`crates/paladin-core/src/lib.rs`:

```rust
//! Servo-free model of the browser. Filled in during Phase 2.
```

**Step 3: Create the engine crate (no Servo yet)**

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
```

`crates/paladin-engine/src/lib.rs`:

```rust
//! Servo glue. Filled in during Phase 3.
```

**Step 4: Create the binary crate**

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
```

`crates/paladin/src/main.rs`:

```rust
//! Paladin: a desktop browser on the Servo engine with an egui chrome.
//! The window, chrome, and engine wiring arrive in Phases 4 and 5.

fn main() {
    println!("Paladin {}", env!("CARGO_PKG_VERSION"));
}
```

**Step 5: Create the xtask crate**

Both tasks are complete now. `icons` needs `assets/logo.svg`, which Task 3 adds; `angle-dlls` is for Windows after Phase 3.

`xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
description = "Workspace helper tasks: `cargo xtask icons`"

[dependencies]
# default-features = false drops text and system-font support, so the
# output never depends on fonts installed on the machine. The logo must
# therefore contain no <text> elements.
resvg = { version = "0.48.1", default-features = false }
icns = "0.5.0"
ico = "0.6.0"
```

`xtask/src/main.rs`:

```rust
//! Workspace helper tasks, run as `cargo xtask <task>` (alias in
//! `.cargo/config.toml`).
//!
//! `icons`: rasterize `assets/logo.svg` into `assets/icons/`:
//! `paladin-<size>.png` for each size in `PNG_SIZES`, `paladin.png`
//! (1024 px), `paladin.icns` (macOS), and `paladin.ico` (Windows). Output is
//! deterministic for a given `Cargo.lock`, so the generated files are
//! committed and CI checks they are up to date.
//!
//! `angle-dlls [profile]`: Windows only. Copy `libEGL.dll` and
//! `libGLESv2.dll`, which the `servo` crate's `no-wgl` feature builds into
//! `target/<profile>/build/mozangle-*/out/`, next to the executables in
//! `target/<profile>/` and `target/<profile>/deps/`. The binary links
//! `libEGL.dll` at load time, and surfman loads it by name, so without this
//! step `paladin.exe` and the engine's test binary do not start.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use resvg::tiny_skia;
use resvg::usvg;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PNG_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];
const ICO_SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];
/// (pixel size, icns slot): the set Apple's `iconutil` produces for a
/// standard `.iconset`. The `_2x` slots are the Retina versions.
const ICNS_TYPES: [(u32, icns::IconType); 10] = [
    (16, icns::IconType::RGBA32_16x16),
    (32, icns::IconType::RGBA32_16x16_2x),
    (32, icns::IconType::RGBA32_32x32),
    (64, icns::IconType::RGBA32_32x32_2x),
    (128, icns::IconType::RGBA32_128x128),
    (256, icns::IconType::RGBA32_128x128_2x),
    (256, icns::IconType::RGBA32_256x256),
    (512, icns::IconType::RGBA32_256x256_2x),
    (512, icns::IconType::RGBA32_512x512),
    (1024, icns::IconType::RGBA32_512x512_2x),
];

fn main() {
    let task = std::env::args().nth(1);
    let result = match task.as_deref() {
        Some("icons") => icons(&project_root()),
        Some("angle-dlls") => {
            let profile = std::env::args()
                .nth(2)
                .unwrap_or_else(|| "debug".to_string());
            angle_dlls(&project_root(), &profile)
        }
        _ => {
            eprintln!("usage: cargo xtask icons | cargo xtask angle-dlls [debug|release]");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

fn icons(root: &Path) -> Result<()> {
    let svg_path = root.join("assets").join("logo.svg");
    let svg_data =
        std::fs::read(&svg_path).map_err(|e| format!("reading {}: {e}", svg_path.display()))?;
    let tree = usvg::Tree::from_data(&svg_data, &usvg::Options::default())?;

    let out = root.join("assets").join("icons");
    std::fs::create_dir_all(&out)?;

    let mut pngs: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for size in PNG_SIZES.iter().chain(ICO_SIZES.iter()).copied() {
        if let std::collections::btree_map::Entry::Vacant(entry) = pngs.entry(size) {
            entry.insert(render_png(&tree, size)?);
        }
    }
    for size in PNG_SIZES {
        std::fs::write(out.join(format!("paladin-{size}.png")), &pngs[&size])?;
    }
    std::fs::write(out.join("paladin.png"), &pngs[&1024])?;

    let mut family = icns::IconFamily::new();
    for (size, icon_type) in ICNS_TYPES {
        let image = icns::Image::read_png(Cursor::new(&pngs[&size]))?;
        family.add_icon_with_type(&image, icon_type)?;
    }
    let mut icns_bytes = Vec::new();
    family.write(&mut icns_bytes)?;
    std::fs::write(out.join("paladin.icns"), icns_bytes)?;

    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in ICO_SIZES {
        let image = ico::IconImage::read_png(Cursor::new(&pngs[&size]))?;
        dir.add_entry(ico::IconDirEntry::encode_as_png(&image)?);
    }
    let mut ico_bytes = Vec::new();
    dir.write(&mut ico_bytes)?;
    std::fs::write(out.join("paladin.ico"), ico_bytes)?;

    println!("wrote icons to {}", out.display());
    Ok(())
}

fn angle_dlls(root: &Path, profile: &str) -> Result<()> {
    const DLLS: [&str; 2] = ["libEGL.dll", "libGLESv2.dll"];
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let profile_dir = target_dir.join(profile);
    let build_dir = profile_dir.join("build");

    // There may be several mozangle-<hash> directories from older builds;
    // take the most recently modified copy of each DLL.
    let mut newest: BTreeMap<&str, (std::time::SystemTime, PathBuf)> = BTreeMap::new();
    for entry in std::fs::read_dir(&build_dir)
        .map_err(|e| format!("reading {}: {e}", build_dir.display()))?
    {
        let dir = entry?.path();
        let is_mozangle = dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("mozangle-"));
        if !is_mozangle {
            continue;
        }
        for name in DLLS {
            let dll = dir.join("out").join(name);
            let Ok(modified) = dll.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            let replace = newest.get(name).is_none_or(|(when, _)| modified > *when);
            if replace {
                newest.insert(name, (modified, dll));
            }
        }
    }
    if newest.len() != DLLS.len() {
        return Err(format!(
            "ANGLE DLLs not found under {}; build the workspace first (Windows only)",
            build_dir.display()
        )
        .into());
    }
    for (name, (_, dll)) in newest {
        for dest_dir in [profile_dir.clone(), profile_dir.join("deps")] {
            std::fs::create_dir_all(&dest_dir)?;
            let dest = dest_dir.join(name);
            std::fs::copy(&dll, &dest)
                .map_err(|e| format!("copying {} to {}: {e}", dll.display(), dest.display()))?;
            println!("copied {name} to {}", dest_dir.display());
        }
    }
    Ok(())
}

fn render_png(tree: &usvg::Tree, size: u32) -> Result<Vec<u8>> {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("icon size must be non-zero")?;
    let scale_x = size as f32 / tree.size().width();
    let scale_y = size as f32 / tree.size().height();
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.encode_png()?)
}
```

**Step 6: Verify operationally**

Run: `cargo build --workspace`
Expected: rustup installs 1.97.1 on first use, then `Finished` with no warnings. The first build fetches the xtask dependencies (resvg, icns, ico).

Run: `cargo test --workspace`
Expected: `test result: ok. 0 passed` for each crate (there are no tests yet) and no errors.

Run: `cargo run -p paladin`
Expected output: `Paladin 0.1.0`

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: no output from fmt, `Finished` from clippy, exit 0.

Run: `cargo xtask`
Expected: prints `usage: cargo xtask icons | cargo xtask angle-dlls [debug|release]` and exits 2 (no logo yet, so do not run `cargo xtask icons` until Task 3).

**Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml clippy.toml .gitignore .cargo crates xtask
git commit -m "chore: scaffold cargo workspace with core, engine, binary, and xtask crates"
```
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: Project documents in the geniami conventions

**Files:**
- Create: `README.md`
- Create: `CHANGELOG.md`
- Create: `CLAUDE.md`
- Create: `AGENTS.md`
- Create: `ROADMAP.md`

These follow the conventions of `../geniami`: a stack table and layout tree in the README, Keep a Changelog with an `Unreleased` section, a `CLAUDE.md` with an italic freshness line and a three-part Definition of Done, numbered phases in `ROADMAP.md` with ` (in progress)` on the current one, and `AGENTS.md` identical to `CLAUDE.md`. The README describes the finished Phase 1 project and opens with a status note saying so; Phase 4 removes the note, and later phases append sections rather than rewriting the file.

**Step 1: Create the files**

`README.md`:

````markdown
# Paladin

A desktop web browser for macOS and Windows, written in Rust. Pages render
through Servo, the experimental Rust browser engine; the window chrome (tab
strip, address bar, settings) is drawn with egui. There is no HTML in the
chrome.

> Phase 1 is in progress. This README describes the finished Phase 1; the
> browser window arrives in Phase 4 of the implementation plan, and until
> then `cargo run` only prints the version.

## Stack

| Layer     | Choice                                                            |
| --------- | ----------------------------------------------------------------- |
| Language  | Rust 1.97.1 (pinned in `rust-toolchain.toml`), edition 2024       |
| Engine    | Servo 0.7.0 (the `servo` crate), confined to `crates/paladin-engine` |
| Chrome    | egui 0.34 on winit 0.30, rendered through egui_glow               |
| Graphics  | surfman GL contexts (Servo's), ANGLE on Windows                   |
| Packaging | cargo-packager: `.app` and `.dmg` on macOS, NSIS installer on Windows |
| CI        | GitHub Actions on macOS and Windows runners                       |

## Workspace layout

```
crates/
  paladin-core/    Servo-free model: windows, tabs, commands, keymap, settings, profiles
  paladin-engine/  Servo glue: engine, GL surfaces, tab host, delegate -> events
  paladin/         The binary: winit app, egui chrome, dispatch loop, packaging metadata
xtask/             `cargo xtask icons` (logo.svg -> icns/ico/png), `cargo xtask angle-dlls`
assets/
  logo.svg         The knight-on-pegasus mark
  icons/           Generated from logo.svg; committed
docs/
  design-plans/          Approved designs
  implementation-plans/  Task-level plans derived from designs
```

## Local development

Prerequisites: a Rust toolchain via `rustup` (the pinned version installs on
first `cargo` run), Python 3.11 or newer on `PATH`, and a C/C++ toolchain:
Xcode Command Line Tools on macOS, or Visual Studio Build Tools with the
"Desktop development with C++" workload on Windows. Windows also needs LLVM
(for `libclang`) with `LIBCLANG_PATH` set to its `bin` directory. SpiderMonkey
is downloaded prebuilt during the build, so no CMake, MozTools, or GStreamer
is required.

1. **Build and run:** `cargo run` opens a window at the homepage. Pass URLs
   to open them as tabs, or `--profile <name>` to use another profile.
2. **Windows only, before running:** `cargo xtask angle-dlls` copies
   `libEGL.dll` and `libGLESv2.dll` next to the executables; without them
   the binary does not start.
3. **Icons:** edit `assets/logo.svg`, run `cargo xtask icons`, and commit the
   regenerated files in `assets/icons/`. CI fails if they are stale.

> The first build downloads a prebuilt SpiderMonkey and compiles the rest of
> Servo: about 5 minutes on an Apple Silicon Mac, longer on Windows and on CI
> runners. Incremental builds of Paladin's own crates take seconds.

Profiles live in `~/Library/Application Support/Paladin/profiles/<name>/` on
macOS and `%APPDATA%\Paladin\profiles\<name>\` on Windows, holding
`settings.toml` and Servo's `servo/` directory (cookies, local storage).

## Tests

- `cargo test --workspace` — unit tests in `paladin-core` and `paladin`, plus
  the headless engine test in `paladin-engine`, which starts Servo without a
  window and loads a page (slow the first time).
- `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all --check` — what CI runs.

## Todo

- [ ] Phase 2: remappable hotkeys, profiles, moving tabs between windows
- [ ] Media playback (the `media-gstreamer` feature is off; no audio or video)
- [ ] A search engine for address-bar text that is not a URL
````

`CHANGELOG.md`:

```markdown
# Changelog

All notable changes to Paladin are documented in this file. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Paladin has no
releases yet; `Unreleased` holds everything on `main`.

## [Unreleased]

### Added

- Cargo workspace with a Servo-free core crate, an engine crate, the `paladin`
  binary, and an `xtask` helper, with project conventions (README, CHANGELOG,
  CLAUDE.md, ROADMAP), the knight-on-pegasus logo with generated icons, and CI
  on macOS and Windows.
```

`CLAUDE.md`:

```markdown
# Paladin

A desktop web browser for macOS and Windows. Servo renders pages, egui draws
the chrome, winit owns the windows. Everything that touches Servo lives in one
crate so the engine could be replaced without rewriting the model or the UI.

_Last updated: 2026-10-14._

## Architecture

Cargo workspace (Rust 1.97.1 pinned in `rust-toolchain.toml`, edition 2024).

| Crate                   | What it is |
| ----------------------- | ---------- |
| `crates/paladin-core`   | `paladin-core` — the Servo-free model: `Browser`/`Window`/`Tab`, `Command` and `dispatch`, `EngineEvent` and `apply`, `Keymap`, `Settings` (TOML), `Profile` paths. Depends on neither servo, egui, nor winit. |
| `crates/paladin-engine` | `paladin-engine` — the only crate that imports `servo`. `Engine` (one per process), `GlSurface` (a window's GL context), `TabHost` (the tabs of one window, painted offscreen), `DelegateState` (Servo callbacks → `EngineEvent`). |
| `crates/paladin`        | `paladin` — the binary: winit `App`, `BrowserWindow` (egui chrome over the tab host), `SettingsWindow`, winit-to-engine input mapping, icons, cargo-packager metadata. |
| `xtask`                 | `cargo xtask icons` (logo → icns/ico/png) and `cargo xtask angle-dlls` (Windows ANGLE DLLs next to the binaries). |

Data flow: the chrome and the keymap emit `Command`s; `paladin_core::dispatch`
updates the model and returns `EngineAction`s, which the app forwards to the
window's `TabHost` or handles itself (open and close windows, settings, quit).
Servo callbacks become `EngineEvent`s that `paladin_core::apply` folds into the
model. Both functions are pure and unit-tested; core ids never meet Servo ids
outside `paladin-engine`.

Local setup is in `README.md`.

## Commands

Run from the repo root:

- `cargo run -- [--profile <name>] [<url>...]` — run the browser.
- `cargo test --workspace` — unit tests plus the headless engine test (slow
  the first time: it builds Servo).
- `cargo clippy --workspace --all-targets -- -D warnings` — lint.
- `cargo fmt --all` — format; CI runs `cargo fmt --all --check`.
- `cargo xtask icons` — regenerate `assets/icons/` from `assets/logo.svg`.
- `cargo xtask angle-dlls [debug|release]` — Windows: copy `libEGL.dll` and
  `libGLESv2.dll` next to the executables after a build.
- `cargo build --release && cargo packager --release -p paladin` — build the
  `.app`/`.dmg` (macOS) or NSIS installer (Windows) into `target/release/`.

## Conventions

- **Servo stays in `paladin-engine`.** The model and the UI only see
  `Command`, `EngineAction`, `EngineEvent`, `EngineInput`, and core ids.
- **Version pins move together.** `servo`, `winit`, `egui`, `egui-winit`,
  `egui_glow`, `glow`, and `surfman` are pinned to exact versions in the
  workspace `Cargo.toml` because they share a GL context and raw window
  handles. To bump Servo: set `servo = "=<new>"` in
  `crates/paladin-engine/Cargo.toml`, set the other pins to the versions in
  that release's root `Cargo.toml` and `ports/servoshell/Cargo.toml`, set
  `rust-toolchain.toml` to its channel, run `cargo update`, then pin any
  pre-release crates to the versions in Servo's own `Cargo.lock` (0.7.0 needs
  `cargo update -p pkcs1 --precise 0.8.0-rc.4`), rebuild, and run the manual
  checklist in `README.md`.
- **One `Engine` per process.** Servo's options are process-global, so the
  engine test binary holds a single test function.
- **Every user action is a `Command`.** New chrome controls and shortcuts add
  a `Command` (and a `CommandKind` if it needs a hotkey), never a direct
  engine call.
- **Generated icons are committed.** Edit `assets/logo.svg`, run
  `cargo xtask icons`, commit the result. Never hand-edit `assets/icons/`.
- **Format before every commit:** `cargo fmt --all`. CI fails on formatting
  or clippy warnings.
- **`AGENTS.md` mirrors this file.** Keep them identical.

## Definition of Done

A task is complete only when:

1. **All tests pass** — `cargo test --workspace` is green, and the manual
   checklist in `README.md` passes on macOS for any change to the chrome,
   input handling, or engine glue.
2. **All checks pass** — `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, and
   `cargo build --workspace` are clean, and `cargo xtask icons` leaves
   `assets/icons/` unchanged.
3. **Docs are current** — `CHANGELOG.md` has an entry under `Unreleased` for
   any user-visible change, and this file's freshness stamp is updated when
   the architecture or commands change.

## Workflow

- Don't create a new branch unless specifically asked. Commit to the current branch.
- Commit messages follow Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`, `ci:`).
- Design plans live in `docs/design-plans/`; implementation plans in
  `docs/implementation-plans/`; the phase roadmap is `ROADMAP.md`.
```

`ROADMAP.md`:

```markdown
# Roadmap

Numbered phases. Each phase works on macOS and Windows before the next begins.

## Phase 1 — Desktop browser (in progress)

A Servo-rendered browser with an egui chrome: multiple windows and tabs, an
address bar, back, forward, and reload, a settings window, the Paladin logo,
an installable `.app` and Windows installer, and CI on both platforms. Every
user action is a named command so hotkeys can be remapped later without a
rewrite, and all per-user state lives under one profile directory.

Design: `docs/design-plans/2026-10-14-paladin-phase-1-desktop-browser.md`.

## Phase 2 — Hotkeys, profiles, tab moves

Remappable hotkeys persisted in `keymap.toml`, edited in the settings window.
Named profiles with one Paladin process per profile (Servo's cookies and
storage are per process), a `profiles.toml` registry, a launcher, and
color-coded chrome. Moving tabs between windows or profiles, implemented as a
close plus a create with the same URL, because a Servo `WebView` cannot leave
the window it was created in.

## Phase 3 — Extensions

Chrome or Firefox extensions. This depends on upstream Servo, which has no
WebExtension support and no scheduled work toward it. It is the most likely
reason to revisit the engine choice; the crate isolation from Phase 1 is what
limits the cost of that revisit.
```

**Step 2: Mirror CLAUDE.md into AGENTS.md**

Run: `cp CLAUDE.md AGENTS.md && diff CLAUDE.md AGENTS.md && echo identical`
Expected: `identical`

**Step 3: Commit, including this implementation plan**

The plan files under `docs/implementation-plans/` are untracked so far; the conventions say plans live in the repository, so they go in with the documents.

```bash
git add README.md CHANGELOG.md CLAUDE.md AGENTS.md ROADMAP.md docs/implementation-plans
git commit -m "docs: add README, CHANGELOG, CLAUDE.md, AGENTS.md, ROADMAP, and the Phase 1 implementation plan"
```
<!-- END_TASK_2 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_TASK_3 -->
### Task 3: Logo and generated icons

**Files:**
- Create: `assets/logo.svg`
- Create (generated): `assets/icons/paladin-16.png`, `paladin-32.png`, `paladin-64.png`, `paladin-128.png`, `paladin-256.png`, `paladin-512.png`, `paladin-1024.png`, `paladin.png`, `paladin.icns`, `paladin.ico`

The logo is a black and white silhouette: a knight with a lance on a winged horse charging from bottom left to top right, on a white rounded square with a black border. It uses only paths (no `<text>`), so rasterizing never depends on installed fonts. It is a first version that Thomas may refine later; the generated icons are regenerated from it with one command.

**Step 1: Create the logo**

`assets/logo.svg`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!-- Paladin logo: a knight with a lance on a pegasus, charging from bottom
     left to top right. Black and white only; no text elements so icon
     generation never depends on installed fonts. -->
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <rect x="24" y="24" width="976" height="976" rx="200" ry="200" fill="#fff" stroke="#000" stroke-width="28"/>
  <g fill="#000" stroke="none">
    <!-- wing: three feather tiers sweeping up and back from the withers -->
    <path d="M520 520 C480 370 380 280 170 230 C290 290 380 350 430 440 C370 400 280 370 140 370 C260 400 350 450 410 510 C360 500 290 505 220 530 C320 535 400 565 470 610 Z"/>
    <!-- tail streaming down and back -->
    <path d="M270 650 C190 670 150 740 135 840 C180 760 230 715 310 700 Z"/>
    <!-- hind legs stretched out behind -->
    <path d="M330 700 L215 825 L190 800 L290 660 Z"/>
    <path d="M385 735 L320 890 L288 880 L335 715 Z"/>
    <!-- body (tilted along the charge), neck, head, ear -->
    <ellipse cx="470" cy="620" rx="245" ry="100" transform="rotate(-30 470 620)"/>
    <path d="M600 520 L700 330 L780 365 L690 565 Z"/>
    <path d="M690 330 L805 255 L870 300 L830 355 L765 385 Z"/>
    <path d="M722 318 L742 262 L782 302 Z"/>
    <!-- front legs tucked and reaching forward -->
    <path d="M625 650 L745 590 L768 622 L665 700 Z"/>
    <path d="M565 690 L645 770 L675 748 L615 660 Z"/>
    <!-- rider: a white halo first so he reads against the horse -->
    <g fill="#fff" stroke="#fff" stroke-width="28" stroke-linejoin="round">
      <path d="M520 405 L590 388 L612 495 L520 505 Z"/>
      <circle cx="578" cy="345" r="38"/>
      <path d="M525 505 L572 505 L600 605 L562 620 Z"/>
    </g>
    <g fill="#000">
      <path d="M520 405 L590 388 L612 495 L520 505 Z"/>
      <circle cx="578" cy="345" r="38"/>
      <path d="M525 505 L572 505 L600 605 L562 620 Z"/>
    </g>
    <!-- lance over the horse's head, white outlined, with a pennant -->
    <path d="M400 560 L905 148" fill="none" stroke="#fff" stroke-width="44" stroke-linecap="round"/>
    <path d="M400 560 L905 148" fill="none" stroke="#000" stroke-width="20" stroke-linecap="round"/>
    <path d="M868 178 L952 150 L894 228 Z"/>
  </g>
</svg>
```

**Step 2: Generate the icons**

Run: `cargo xtask icons`
Expected: `wrote icons to /Users/thomas/vibes/paladin/assets/icons` and ten files in `assets/icons/`. Open `assets/icons/paladin-256.png` (for example with `open assets/icons/paladin-256.png`) and confirm it shows the winged horse, rider, and lance inside a rounded square.

**Step 3: Verify the output is deterministic**

Run:

```bash
md5 -q assets/icons/* > /tmp/paladin-icons-1.txt
cargo xtask icons > /dev/null
md5 -q assets/icons/* > /tmp/paladin-icons-2.txt
cmp /tmp/paladin-icons-1.txt /tmp/paladin-icons-2.txt && echo deterministic
```

Expected: `deterministic`. (CI relies on this: it regenerates the icons and fails if `git diff` is not empty.)

**Step 4: Commit**

```bash
git add assets
git commit -m "feat: add Paladin logo and generated app icons"
```
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: GitHub repository and CI on macOS and Windows

**Files:**
- Create: `.github/workflows/ci.yml`

The workflow already includes the Python and LLVM steps that the Servo build will need from Phase 3, so that phase only has to raise the timeout and add the Windows DLL step. Icons are only diffed on the Apple Silicon runner because tiny-skia does not promise bit-identical rasterization across CPU architectures.

**Step 1: Create the workflow**

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
    timeout-minutes: 60
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

**Step 2: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: build, lint, and test on macOS and Windows runners"
```

**Step 3: Create the GitHub repository and push**

Thomas chose a private repository under his account. Run:

```bash
gh repo create thomasmost/paladin --private --source=. --remote=origin --push
```

Expected: the command prints the repository URL `https://github.com/thomasmost/paladin` and pushes `main`.

Run: `git remote -v`
Expected: `origin  https://github.com/thomasmost/paladin.git (fetch)` and `(push)` (or the SSH form, depending on `gh` configuration).

**Step 4: Verify CI is green on both runners**

Run: `gh run watch --exit-status`
(If several runs are listed, pick the one for the latest commit.) Expected: both matrix jobs, `macos-15` and `windows-2025`, finish with success. If a job fails, read its log with `gh run view --log-failed`, fix, commit with a `ci:` prefix, push, and watch again. Phase 1 is not done until both jobs are green.

Run: `gh run list --limit 1`
Expected: a line showing `completed  success` for the CI workflow on `main`.
<!-- END_TASK_4 -->

---

## Phase 1 verification

- `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all --check` are all clean locally.
- `cargo xtask icons && git status --porcelain assets/icons` prints nothing (the icons are up to date).
- `gh run list --limit 1` shows the latest CI run on `main` succeeded on both runners.
- `git log --oneline` shows the four Conventional Commits from this phase on top of the design-plan commit.
