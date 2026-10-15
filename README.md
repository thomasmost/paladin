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
