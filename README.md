# Paladin

A desktop web browser for macOS and Windows, written in Rust. Pages render
through Servo, the experimental Rust browser engine; the window chrome (tab
strip, address bar, settings) is drawn with egui. There is no HTML in the
chrome. The model and the chrome are
[osmena](https://github.com/thomasmost/osmena), the shell Paladin shares
with [Nautilus](https://github.com/thomasmost/nautilus), its Chromium
sibling; this repository holds what is Servo's.

## Features

- **Servo under the hood.** Pages render through Servo, the Rust browser
  engine, not Chromium or WebKit. The chrome is drawn with egui; menus are
  the system's own popup menus.
- **Crashes stay in their tab.** Each tab's script thread runs in its own
  content process. A page that takes Servo down shows an error panel in that
  tab with the reason; every other tab keeps working.
- **Multi-select tabs.** Cmd/Ctrl-click and Shift-click select several
  tabs, then drag them together onto another window or onto the desktop
  for a new one, or right-click to close them all, move them to a window,
  or open them in another profile.
- **Profiles.** Each profile has its own settings, cookies, and local
  storage; `--profile <name>` picks one, and a tab can be handed to another
  profile from its context menu.
- **Your colour.** Settings offers a row of swatches that tint the tab strip
  and toolbar per profile.
- **Sleek by default.** No system title bar: the tab strip is the title
  bar, with the window controls at its end. "Use the system title bar and
  borders" in Settings brings the frame back.
- **Tab folders and bookmark groups.** Fold a selection of tabs behind
  one chip in the strip, or bookmark it as a named group to reopen from
  the menu.
- **A right-click menu.** Open a link in a new tab or window, copy a
  link or image address, cut, copy, paste, or select all; what Servo
  cannot do yet (save, print, inspect) is left out.
- **Search from the address bar.** Text that is not a URL is searched,
  with Google unless Settings names another engine.
- **Keyboard first.** Every control is a command with Chromium's shortcut
  (Cmd for Ctrl on macOS), chords work even while the address bar has
  focus, and Settings remaps them the way a game's controls screen does:
  a primary and a secondary slot per action, click and press.
- **Web Animations.** Script-driven CSS animations are sampled by Paladin's
  forks of Servo and stylo, so pages such as bsky.app render past their
  splash screens.
- **Engine-agnostic design.** Only one crate imports Servo, and it
  implements osmena's `Engine` trait; the model and the UI are osmena's,
  shared with Nautilus and unit-tested without an engine.

## Stack

| Layer     | Choice                                                            |
| --------- | ----------------------------------------------------------------- |
| Language  | Rust 1.97.1 (pinned in `rust-toolchain.toml`), edition 2024       |
| Engine    | Servo 0.7.0 (the `servo` crate), confined to `crates/paladin-engine` |
| Shell     | [osmena](https://github.com/thomasmost/osmena): the model, the egui 0.34 chrome on winit 0.30, the settings window, pinned by git revision |
| Graphics  | surfman GL contexts (Servo's), ANGLE on Windows; the chrome shares Servo's context |
| Packaging | cargo-packager: `.app` and `.dmg` on macOS, NSIS installer on Windows |
| CI        | GitHub Actions on macOS and Windows runners                       |

## Workspace layout

```
crates/
  paladin-engine/  Servo glue: osmena's Engine and TabHost over Servo, GL surfaces, delegate -> events
  paladin/         The binary: the Brand, Servo's content-process preamble, the command line, packaging metadata
xtask/             `cargo xtask icons` (osmena's icon task), `cargo xtask angle-dlls`
assets/
  logo.png         The knight-on-pegasus artwork
  logo.svg         Frames logo.png in the rounded tile the icons use
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

Servo and stylo are built from Paladin's forks (`thomasmost/servo`,
`thomasmost/stylo`): the root `Cargo.toml` patches their crates to small
`dist/*` branches that cargo fetches like any git dependency, so no extra
setup is needed. `CLAUDE.md` describes how to work on the forks.

The model, chrome, and winit application come from the osmena crates,
pinned by git revision in the workspace `Cargo.toml`. To change them
alongside Paladin, check osmena out next to this repository and patch the
source to it in `~/.cargo/config.toml` (osmena's README has the table);
then bump the `rev`s here once the change is pushed.

1. **Build and run:** `cargo run` opens a window at the homepage. Pass URLs
   to open them as tabs, `--profile <name>` to use another profile, or
   `--single-process` to keep every tab in the browser process.
2. **Windows only, before running:** `cargo xtask angle-dlls` copies
   `libEGL.dll` and `libGLESv2.dll` next to the executables; without them
   the binary does not start.
3. **Icons:** replace `assets/logo.png` (the artwork) or edit
   `assets/logo.svg` (the tile around it), run `cargo xtask icons`, and
   commit the regenerated files in `assets/icons/`. CI fails if they are
   stale.

> The first build downloads a prebuilt SpiderMonkey and compiles the rest of
> Servo: about 5 minutes on an Apple Silicon Mac, longer on Windows and on CI
> runners. Incremental builds of Paladin's own crates take seconds.

Profiles live in `~/Library/Application Support/Paladin/profiles/<name>/` on
macOS and `%APPDATA%\Paladin\profiles\<name>\` on Windows, holding
`settings.toml` (homepage, new-tab URL, chrome colour) and Servo's `servo/`
directory (cookies, local storage).

## Tests

- `cargo test --workspace` — the engine's unit tests plus its two test
  binaries, `headless` and `web_animations`, which start Servo without a
  window (slow the first time). The model, chrome, and routing tests live
  in osmena.
- `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all --check` — what CI runs.

## Manual checklist

Run on macOS with `cargo run` after any change to the chrome, input handling,
or engine glue. Each item must pass.

1. **Launch.** A window opens with one tab at the homepage and the page
   renders. There is no system title bar: on Windows the tab strip ends
   in the ☰ menu and the minimize, maximize, and close controls, and the
   taskbar shows the Paladin logo with the title Paladin; on macOS the
   traffic lights sit at the strip's left end and the Dock shows the
   logo. Dragging the strip's empty space moves the window,
   double-clicking it maximizes (zooms), and on Windows the pointer
   becomes a resize arrow at every edge and corner.
2. **Address bar.** Type `example.com` and press Enter: the page loads, the
   tab title becomes "Example Domain", and the address bar shows
   `https://example.com/`. Typing `localhost:8080` targets `http://`.
3. **History.** Navigate to a second page, then use Back and Forward: the
   buttons enable and disable with the history and the pages change. Reload
   is always present and reloads the current page; while a page loads, a
   disabled Stop button appears beside it whose tooltip explains that Servo
   cannot stop a load.
4. **Tabs.** "+" or Cmd+T (Ctrl+T on Windows) opens a new tab, activates
   it, and puts the keyboard focus in the address bar with its text
   selected, so typing a URL and pressing Enter loads it in the new tab;
   clicking a tab title switches tabs and the address bar follows; "×" and
   middle-click close a tab and the neighbour to the right becomes active;
   closing the last tab closes the window and the process exits.
   **Selection.** With three tabs open, Cmd-click (Ctrl-click on Windows)
   the first: it gets a tinted fill while the third stays active; Shift-click
   the first from the third selects all three; a plain click leaves only
   the clicked tab selected. Right-click a selected tab: the menu offers
   "Close 3 tabs", "Close other tabs" (disabled when all are selected),
   "Move 3 tabs to new window" (same), "Move 3 tabs to window" (disabled
   with one window), and "Open 3 tabs in profile" (disabled with no other
   profile directory). Right-click an unselected tab: it becomes the active
   and only selected tab and the menu says "Close tab". Open a second window
   with Cmd+N, select two tabs in the first, and drag one of them onto the
   second window: both appear at the end of its strip (reloaded, with the
   previously active one active) and leave the first. Drag a tab onto the
   desktop: it opens in a new window. Dropping within the same window does
   nothing. "Open tab in profile" starts a second Paladin for that profile
   with the tab's URL and closes the tab here. Both this menu and the "☰"
   menu are the system's own popup menus (native look, Escape closes them,
   shortcuts shown the platform way), not egui.
5. **Crashed tab.** Each tab's script thread (DOM, JavaScript, layout)
   runs in a content process, a second `paladin` process. A page whose
   script thread panics or crashes natively (a Servo bug, not something a
   page can do on purpose) shows the tab's error panel with the reason, the
   other tabs keep working, and the terminal shows `tab crashed: <reason>`;
   Back, Forward, and Reload clear the panel. `--single-process` runs every
   tab in the browser process instead, for debugging the engine; a crash
   then ends the browser.
6. **Resize and HiDPI.** Resize the window: the page stays aligned under the
   toolbar with no gap or stretching. Drag the window to a display with a
   different scale factor (or change the display scaling): text stays sharp
   and clicks land where expected.
7. **Input.** Scrolling with the wheel or trackpad over the page scrolls it;
   clicking a text field on a page and typing enters text; typing in the
   address bar does not reach the page. Hovering a link shows its URL in the
   status overlay at the bottom left without the page moving. Drag-select
   text in the page, release the button over the toolbar, then move the
   pointer back over the page: the selection does not keep growing. Press
   in the address bar, release over the page, then move over the page: the
   address bar's selection does not keep growing either.
   **Page menu.** Right-click a link: a native menu at the pointer offers
   Open link in new tab, Open link in new window, and Copy link address;
   "Open link in new tab" adds a tab right after this one without
   leaving it. Right-click a text field: Undo and Redo (disabled), Cut,
   Copy, Paste, Delete (disabled), Select all. Right-click empty page:
   Back, Forward, Reload with their shortcuts.
   **Search.** Type `rust programming language` in the address bar and
   press Enter: a Google search for it loads.
8. **Exit.** Closing the window exits with no Servo error output on the
   terminal. Quitting from the Dock menu does the same.
9. **Windows and menu.** Cmd+N (Ctrl+N on Windows), or New window chosen
   with the mouse from the "☰" menu, opens a second window; each window's
   tabs, address bar, and page are independent, and closing one leaves the
   other working. Clicking on the page outside an open menu closes the menu
   without the click reaching the page.
10. **Shortcuts.** With Cmd on macOS or Ctrl on Windows: T new tab, W close
   tab (also Ctrl+F4 on Windows), Shift+T reopens the last closed tab,
   Shift+W close window, N new window, L focus the address bar with its
   text selected (also Alt+D on Windows), R reload (also F5 on Windows),
   period Stop (only logs a warning: Servo 0.7.0 cannot
   stop a load), Shift+] and Shift+[ on macOS or Tab and
   Shift+Tab and PgDn/PgUp everywhere next and previous tab (wrapping),
   1 to 8 the tab at that position and 9 the last tab, comma opens
   settings, Q quits. Back and forward are Cmd+[ and Cmd+] on macOS and
   Alt+Left and Alt+Right on Windows; the homepage is Cmd+Shift+H or
   Alt+Home; F11 (Ctrl+Cmd+F on macOS) shows the page alone and again
   brings the chrome back. A chord held down fires once, and none of its
   key strokes reach the page. Shortcuts also work while the address bar
   has focus. Escape and Cmd+Left/Right are not shortcuts and do reach the
   page.
11. **Settings.** Settings (Cmd+, or chosen with the mouse from the menu)
    opens a window with Homepage, New tab URL, a row of chrome colour
    swatches, the "Use the system title bar and borders" checkbox, the
    restore checkbox, and the Keyboard shortcuts table. Edits are saved when a field loses
    focus or the window closes; relaunching opens the new homepage, and
    `settings.toml` in the profile directory holds it. Click a swatch: every
    browser window's tab strip takes that colour at once, the toolbar a pale
    wash of it, and `chrome_color = "<name>"` lands in `settings.toml`; the
    outlined swatch is the current one. With the settings window focused,
    Cmd+W closes it and Cmd+Q quits.
    **Title bar.** Tick "Use the system title bar and borders": every open
    window gets the system frame at once on Windows (on macOS, the next
    window opened); untick it and the strip is the title bar again.
    **Shortcuts.** Click the primary slot of New tab: it reads "Press
    keys…"; press Ctrl+Y (Cmd+Y): the slot shows it, `keymap.toml` in the
    profile directory holds `new-tab = ["Ctrl+KeyY"]` (Meta on macOS), and
    Ctrl+Y opens a tab while Ctrl+T no longer does. Give Reload's secondary
    slot Ctrl+Y: New tab loses it and a note under the table says so.
    Right-click a slot to clear it; "Reset to defaults" puts everything
    back and the file lists the defaults.
12. **Folders and bookmarks.** Open three tabs. Right-click the second:
    the menu has "Close all selected (1)" (disabled with one selected),
    "Move tab to folder…", and "Bookmark all selected (1)…". Pick "Move
    tab to folder…": a prompt over the page offers "Folder A"; Enter.
    The strip shows a chip "▸ Folder A (1)" in the tab's place; click
    it: a list drops down with the tab, and clicking the tab there
    activates it. Right-click the chip: Expand, Rename…, Ungroup, Close
    folder (1 tab). Expand: the tab appears after the chip, which reads
    "▾ Folder A". Ctrl-click the other two tabs, right-click one, "Move
    2 tabs to folder…", pick Folder A from the existing-folder choice:
    the folder holds three tabs, together. "Bookmark all selected…" on a
    selection asks for a group name; type Work, Enter: the profile has
    `bookmarks.toml` with the pages, and ☰ → Bookmarks → Work lists
    them with "Open all (n)", which opens each in a new tab. Folders are
    gone after a relaunch; bookmarks stay.
13. **Clean exit.** Closing the last window, Cmd+Q from a browser window or
    from the settings window, and Quit from the Dock menu (macOS) all exit
    with code 0 and no Servo error output.
14. **Web Animations.** Open `https://bsky.app`: the blue splash screen fades
    out and the landing page or feed renders and scrolls. (Script animations
    are sampled by Paladin's Servo and stylo forks; see
    `docs/upstream-issues.md`, "Carried in the fork".)

## Packaging

`cargo install cargo-packager --version 0.11.8 --locked` once, then:

```bash
cargo build --release -p paladin
cargo xtask angle-dlls release   # Windows only
cargo packager --release -p paladin
```

`cargo packager` also runs `cargo build --release -p paladin` itself (its
`before-packaging-command`), so the bundles always match the current source.
On Windows the explicit build must still come first: the ANGLE DLL step reads
the build output, and the installer picks the DLLs up from `target/release/`.

Output lands in `target/release/`: `Paladin.app` and `Paladin_<version>_aarch64.dmg`
on macOS, `paladin_<version>_x64-setup.exe` (NSIS, per-user install) on
Windows. The bundles are unsigned. CI builds both on every push to `main` and
uploads them as workflow artifacts named `paladin-macos-15` and
`paladin-windows-2025`, together with the symbol file for that build
(`paladin.pdb` on Windows, `paladin.dSYM` on macOS; the release profile keeps
line tables). Release builds on Windows are GUI-subsystem apps with no console
window, so they print no logs; debug builds keep the console. A release
build still writes its log to a redirected stderr, and a crash can be
caught with `cdb -g -G paladin.exe <url>` (set `_NO_DEBUG_HEAP=1` first) and
symbolized with the PDB of the same build.

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

Upstream causes and the features they block are tracked in
`docs/upstream-issues.md`.

- No audio or video playback (Servo's `media-gstreamer` feature is off).
- Textareas sized by their `rows` attribute are shorter than in other
  browsers (Servo uses rows × font size, not rows × line height), so one-row
  fields such as Google's search box cut off descenders.
- The page menu has no Save, Print, View source, or Inspect (Servo 0.7.0
  has no API for them), and Undo, Redo, and Delete in a field are
  disabled.
- Stop is disabled while a page loads (Servo 0.7.0 has no API for it).
- Without the system title bar, Windows 11's snap-layouts flyout on the
  maximize control is not available (snap by dragging still is), and the
  frameless window has not been tried on macOS.
- No macOS application menu: winit's default menu is turned off so that
  Cmd+Q reaches Paladin's own shortcut handling, which also removes Cmd+H
  (hide) and Cmd+M (minimize). A custom menu is planned.
- Restore previous windows on launch is stored in `settings.toml` but not yet
  acted on.
- No bookmarks, history UI, downloads UI, extensions, sync, or auto-update.
- The Windows installer does not bundle the Microsoft Visual C++
  Redistributable that MSVC builds need. Install the x64 redistributable
  first from https://aka.ms/vs/17/release/vc_redist.x64.exe, or the app may
  fail to start with a missing `vcruntime140.dll` error.

## Todo

- [ ] Phase 2: remappable hotkeys, profiles, moving tabs between windows
- [ ] Media playback (the `media-gstreamer` feature is off; no audio or video)
- [ ] IME composition for page text fields: forward Servo's input-method control to winit's `set_ime_allowed`; egui-winit only enables IME for its own fields
- [ ] Restore previous windows on launch (the setting is stored but not acted on)
