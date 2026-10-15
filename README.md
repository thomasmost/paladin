# Paladin

A desktop web browser for macOS and Windows, written in Rust. Pages render
through Servo, the experimental Rust browser engine; the window chrome (tab
strip, address bar, settings) is drawn with egui. There is no HTML in the
chrome.

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

## Todo

- [ ] Phase 2: remappable hotkeys, profiles, moving tabs between windows
- [ ] Media playback (the `media-gstreamer` feature is off; no audio or video)
- [ ] A search engine for address-bar text that is not a URL
- [ ] IME composition for page text fields: egui-winit disables IME while no chrome field has focus
