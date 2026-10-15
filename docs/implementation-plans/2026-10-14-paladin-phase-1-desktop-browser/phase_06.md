# Paladin Phase 1 Desktop Browser Implementation Plan

**Goal:** A desktop browser for macOS and Windows that renders pages with Servo under an egui chrome, with multiple windows and tabs, a settings window, a logo, installable packages, and CI on both platforms.

**Architecture:** Three-crate Cargo workspace plus an `xtask` helper. `paladin-core` holds the Servo-free model, named commands, keymap, settings, and profile paths. `paladin-engine` is the only crate that imports Servo and exposes `Engine`, `GlSurface`, and `TabHost`. The `paladin` binary runs the winit event loop and draws the chrome with egui, blitting Servo's offscreen framebuffer under the toolbar. The UI only emits `Command`s; the pure `dispatch` and `apply` functions in core turn commands and engine events into state changes plus `EngineAction`s.

**Tech Stack:** Rust 1.97.1 (edition 2024), servo 0.7.0, winit 0.30.13, egui / egui-winit / egui_glow 0.34.3, glow 0.17.0, surfman 0.14.0, keyboard-types 0.8.3, directories 6, toml 1.1, clap 4, resvg 0.48.1, icns 0.5, ico 0.6, cargo-packager 0.11.8, GitHub Actions on `macos-15` and `windows-2025`.

**Scope:** 6 phases from original design (this file is phase 6 of 6)

**Codebase verified:** 2026-10-14. After Phase 5 the browser is complete; `crates/paladin/Cargo.toml` has no packaging metadata and no `build.rs`. Facts verified in cargo-packager 0.11.8's source: it reads `[package.metadata.packager]` from every workspace package, picks binaries up from Cargo automatically, does not build (so the release build must exist), resolves `icons` and `resources` relative to the current directory, copies a listed `.icns` as the macOS icon, downloads NSIS on Windows and `create-dmg` on macOS on first use, and installs plain `resources` at the package root (`$INSTDIR` for NSIS). It does not embed an icon in the Windows executable, which is why `build.rs` uses `winresource`. The `.app` is unsigned (no `signing-identity`), which is fine for a local run and for Thomas's own machines.

---

## Conventions for every task in every phase

- Run every command from the repository root, `/Users/thomas/vibes/paladin`.
- Commit to the current branch (`main`) with Conventional Commits. No feature branches.
- "Create" means the file does not exist yet; write exactly the content shown. "Replace" means overwrite the whole file with the content shown. Code blocks are complete files unless a step says otherwise.
- Before every commit run `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`. All three must be clean.

---

## Phase 6: Packaging, CI artifacts, Windows verification

**Goal:** Installable builds on both platforms and the docs that make them reproducible.

**Done when:** CI uploads both artifacts; the `.app` runs on Thomas's Mac from the bundle; Thomas runs the Windows installer and the manual checklist on his Windows machine and reports it passing. The Definition of Done for Phase 1 is met.

<!-- START_TASK_1 -->
### Task 1: Packaging metadata, Windows icon resource, and a local `.app`

**Files:**
- Replace: `crates/paladin/Cargo.toml`
- Create: `crates/paladin/build.rs`

**Step 1: Replace the manifest**

The only changes from Phase 4 are the `[target.'cfg(windows)'.build-dependencies]` table and the `[package.metadata.packager]` tables at the end. If Phase 4 or 5 changed this manifest locally (a dependency added while fixing a checklist item), run `git diff` after replacing it and carry those changes forward before committing.

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

[target.'cfg(windows)'.build-dependencies]
winresource = "0.1"

# `cargo packager --release -p paladin`, run from the workspace root (paths
# below resolve against the current directory). Build first:
# `cargo build --release -p paladin`, plus `cargo xtask angle-dlls release`
# on Windows.
[package.metadata.packager]
product-name = "Paladin"
identifier = "org.paladin.software"
icons = ["assets/icons/paladin.icns", "assets/icons/paladin.ico", "assets/icons/paladin-256.png"]
# Formats that do not apply to the host are skipped with a warning.
formats = ["app", "dmg", "nsis"]
# Windows: the ANGLE DLLs that `cargo xtask angle-dlls release` copied next
# to the binary, installed beside paladin.exe. The glob matches nothing on
# macOS.
resources = ["target/release/*.dll"]

[package.metadata.packager.macos]
minimum-system-version = "12.0"

[package.metadata.packager.nsis]
installer-icon = "assets/icons/paladin.ico"
install-mode = "currentUser"
```

**Step 2: Create the build script**

`crates/paladin/build.rs`:

```rust
//! Embeds the application icon into `paladin.exe` on Windows so Explorer,
//! the taskbar, and the installer's shortcuts show the logo. cargo-packager
//! does not do this itself. No-op elsewhere.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/paladin.ico");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icons/paladin.ico");
        resource
            .compile()
            .expect("embedding the Windows icon resource");
    }
}
```

**Step 3: Verify the workspace still builds and lints**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: `Finished`, no warnings (on macOS the build script only emits the `rerun-if-changed` line).

**Step 4: Install cargo-packager and build the release binary**

Run: `cargo install cargo-packager --version 0.11.8 --locked`
Expected: compiles and installs `cargo-packager` into `~/.cargo/bin` (a few minutes).

Run: `cargo build --release -p paladin`
Expected: a release build of Servo and Paladin; expect 10 to 20 minutes the first time. `Finished`.

**Step 5: Package**

Run: `cargo packager --release -p paladin`
Expected: on first use it downloads the `create-dmg` script, then prints the paths of `target/release/Paladin.app` and `target/release/Paladin_0.1.0_aarch64.dmg`. The `nsis` format is skipped with a warning on macOS.

Run: `ls target/release/Paladin.app/Contents/Resources/ target/release/*.dmg`
Expected: `paladin.icns` among the resources and the `.dmg` file.

Run: `open target/release/Paladin.app`
Expected: Paladin launches from the bundle with its icon in the Dock and the homepage rendering. Quit it with Cmd+Q.

**Step 6: Commit**

```bash
git add crates/paladin/Cargo.toml crates/paladin/build.rs Cargo.lock
git commit -m "feat(app): cargo-packager metadata and Windows icon resource"
```
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: CI packages both platforms and uploads the artifacts

**Files:**
- Replace: `.github/workflows/ci.yml`

A `package` job runs after `check` on both runners: release build, the Windows DLL copy, `cargo packager`, and an artifact upload of the `.dmg` and the NSIS `-setup.exe`. The `.app` is not uploaded on its own because zipped artifacts drop executable permissions; the `.dmg` contains it.

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

  package:
    name: package ${{ matrix.os }}
    needs: check
    runs-on: ${{ matrix.os }}
    timeout-minutes: 180
    strategy:
      fail-fast: false
      matrix:
        os: [macos-15, windows-2025]
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: 1.97.1
      - uses: actions/setup-python@v6
        with:
          python-version: "3.12"
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

      # rust-cache restores ~/.cargo/bin, so this is a no-op after the first run.
      - run: cargo install cargo-packager --version 0.11.8 --locked

      - run: cargo build --release -p paladin
      - name: Copy ANGLE DLLs next to the binary (Windows)
        if: runner.os == 'Windows'
        run: cargo xtask angle-dlls release

      # macOS: Paladin.app and a .dmg. Windows: an NSIS installer. The
      # installer and the .dmg are uploaded; a bare .app loses its
      # executable bits in a zipped artifact, and the .dmg contains it.
      - run: cargo packager --release -p paladin

      - uses: actions/upload-artifact@v7
        with:
          name: paladin-${{ matrix.os }}
          path: |
            target/release/*.dmg
            target/release/*-setup.exe
          if-no-files-found: error
```

**Step 2: Commit and push**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: package the macOS .dmg and Windows installer and upload them"
git push
```

**Step 3: Verify the artifacts**

Run: `gh run watch --exit-status`
Expected: `check` and `package` succeed on both runners (the release builds are slow; allow up to an hour each on a cold cache).

Run: `gh run view --json databaseId,conclusion,name --jq '.'` then `gh run download <databaseId> --dir /tmp/paladin-artifacts && ls -R /tmp/paladin-artifacts`
Expected: `paladin-macos-15/Paladin_0.1.0_aarch64.dmg` and `paladin-windows-2025/paladin_0.1.0_x64-setup.exe`.

If the macOS `package` job fails inside `create-dmg` (hosted runners occasionally fail `hdiutil detach`), re-run the job once with `gh run rerun <databaseId> --failed`; if it fails the same way twice, report it rather than looping.
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: Bring the documents current

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `CLAUDE.md` and `AGENTS.md`

**Step 1: Add packaging and Windows details to the README**

In `README.md`, insert this section between `## Manual checklist` and `## Todo` (the block is fenced with four backticks here only because it contains a three-backtick fence of its own; paste the content without the outer fence):

````markdown
## Packaging

`cargo install cargo-packager --version 0.11.8 --locked` once, then:

```bash
cargo build --release -p paladin
cargo xtask angle-dlls release   # Windows only
cargo packager --release -p paladin
```

Output lands in `target/release/`: `Paladin.app` and `Paladin_<version>_aarch64.dmg`
on macOS, `paladin_<version>_x64-setup.exe` (NSIS, per-user install) on
Windows. The bundles are unsigned. CI builds both on every push to `main` and
uploads them as workflow artifacts named `paladin-macos-15` and
`paladin-windows-2025`.

### Windows prerequisites

- Visual Studio 2022 Build Tools with the "Desktop development with C++"
  workload: MSVC v143, a Windows 10/11 SDK (10.0.19041 or newer), and C++ ATL.
- LLVM (any recent release from https://releases.llvm.org/) with the
  environment variable `LIBCLANG_PATH` set to its `bin` directory, for
  example `C:\Program Files\LLVM\bin`.
- Python 3.11 or newer on `PATH`.
- Rust via https://win.rustup.rs/; the pinned toolchain installs on first use.

No CMake, Ninja, MozTools, or GStreamer: SpiderMonkey is downloaded prebuilt
and media is disabled. After `cargo build`, run `cargo xtask angle-dlls`
before `cargo run` so `libEGL.dll` and `libGLESv2.dll` sit next to the
executable.

### Known limitations

- No audio or video playback (Servo's `media-gstreamer` feature is off).
- No search engine: address-bar text that is not a URL is ignored.
- Stop is disabled while a page loads (Servo 0.7.0 has no API for it).
- No macOS application menu: winit's default menu is turned off so that
  Cmd+Q reaches Paladin's own shortcut handling, which also removes Cmd+H
  (hide) and Cmd+M (minimize). A custom menu is planned.
- No bookmarks, history UI, downloads UI, extensions, sync, or auto-update.
````

**Step 2: Record the change**

In `CHANGELOG.md`, under `## [Unreleased]` / `### Added`, add after the last bullet:

```markdown
- Packaging with cargo-packager: `Paladin.app` and a `.dmg` on macOS, an NSIS
  installer on Windows, built and uploaded by CI.
```

**Step 3: Refresh CLAUDE.md and mirror it**

In `CLAUDE.md`, change the freshness line to `_Last updated: <today's date, YYYY-MM-DD>._` (the Commands section already lists the packaging command, so nothing else changes). Then run `cp CLAUDE.md AGENTS.md`.

**Step 4: Commit and push**

```bash
git add README.md CHANGELOG.md CLAUDE.md AGENTS.md
git commit -m "docs: packaging, Windows prerequisites, and known limitations"
git push
```
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: Windows verification by Thomas, then close out Phase 1

**Files:**
- Modify: `ROADMAP.md`

This task needs Thomas at his Windows machine; the executor prepares the request and waits.

**Step 1: Ask Thomas to verify on Windows**

Tell Thomas the CI run id from Task 2 and ask him to, on the Windows machine:

1. Download the `paladin-windows-2025` artifact (`gh run download <databaseId> -n paladin-windows-2025`, or from the Actions page), unzip it, and run `paladin_0.1.0_x64-setup.exe`. Expected: a per-user install with a Start menu entry showing the Paladin icon.
2. Launch Paladin from the Start menu and run the manual checklist in `README.md` (items 1 to 11, using Ctrl in place of Cmd and Alt+Left/Right for back/forward; where an item mentions the Dock, use the window's close button and Alt+F4 instead).
3. Optionally, a development build: install the prerequisites from the README's "Windows prerequisites", clone the repository, run `cargo build`, `cargo xtask angle-dlls`, and `cargo run`.
4. Report which items pass and paste any error output.

**Step 2: Fix what Thomas reports**

If an item fails only on Windows, fix it, push, wait for green CI and new artifacts, and ask again. Likely areas: the ANGLE DLLs missing from the installer (check `resources` in `crates/paladin/Cargo.toml` and that the CI job ran `cargo xtask angle-dlls release` before packaging), HiDPI scaling on Windows (the `ScaleFactorChanged` path in `window.rs`), and the Alt+arrow shortcuts.

**Step 3: Mark Phase 1 complete**

Once Thomas reports the checklist passing on Windows, in `ROADMAP.md` change the heading `## Phase 1 — Desktop browser (in progress)` to `## Phase 1 — Desktop browser` and add this line after the paragraph that follows it, before the `Design:` line:

```markdown
Verified on macOS and Windows on <date Thomas reported, YYYY-MM-DD>.
```

```bash
git add ROADMAP.md
git commit -m "docs: mark Phase 1 verified on macOS and Windows"
git push
```
<!-- END_TASK_4 -->

---

## Phase 6 verification (Definition of Done for Phase 1)

- `cargo packager --release -p paladin` on the Mac produces `Paladin.app` and the `.dmg`, and the `.app` runs with the logo.
- The latest CI run on `main` uploads `paladin-macos-15` (the `.dmg`) and `paladin-windows-2025` (the installer).
- Thomas has run the Windows installer and reported the manual checklist passing on Windows.
- `README.md`, `CHANGELOG.md`, `CLAUDE.md`/`AGENTS.md`, and `ROADMAP.md` are current, and every commit on `main` follows Conventional Commits.
