# Changelog

All notable changes to Paladin are documented in this file. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Paladin has no
releases yet; `Unreleased` holds everything on `main`.

## [Unreleased]

### Changed

- The model, the egui chrome, the settings window, and the winit
  application now come from the [osmena](https://github.com/thomasmost/osmena)
  crates, shared with Nautilus (Paladin's Chromium sibling). `paladin-core`
  is gone; `paladin-engine` implements osmena's `Engine` and `TabHost`
  traits; the binary is the brand, Servo's content-process preamble, and
  the command line. No user-visible change.

### Added

- A chrome colour per profile: Settings has a row of swatches (Slate, Ocean,
  Forest, Plum, Rose, Amber, Graphite; `chrome_color` in `settings.toml`).
  The tab strip takes the colour, the toolbar a pale wash of it, so windows
  of different profiles tell apart at a glance.
- Tabs are redrawn: taller, with more padding, rounded tops, the close
  control inside the tab, a translucent fill that strengthens on hover and
  selection, and the active tab in the toolbar's colour so it reads as
  attached to the page.
- Tab selection and tab moves. Ctrl-click (Cmd-click on macOS) adds a tab
  to the selection or removes it, Shift-click selects the range from the
  active tab; selected tabs are tinted. Right-click a tab for a menu that
  acts on the selection (or on that tab): Close tab(s), Close other tabs,
  Move to new window, Move to window (the other open windows), Open in
  profile (the other profiles on this machine, each started as its own
  `paladin` process). Drag selected tabs onto another Paladin window to
  move them there, or onto the desktop to open a new window with them. A
  moved tab is recreated at its URL in the destination (a Servo webview
  cannot leave its window, `docs/upstream-issues.md` S8), so the page
  reloads and loses its history and form state. Design:
  `docs/design-plans/2026-10-17-tab-selection-and-moves.md`.
- Each tab's script thread (DOM, JavaScript, layout) runs in its own
  content process, Servo's multiprocess mode: the browser starts a second
  `paladin` process per script thread (with a hidden `--content-process`
  argument) and talks to it over IPC. A panic or a native crash in a page
  now shows that tab's error panel and leaves the rest of the browser
  running. The processes are not sandboxed (Servo's sandbox exists for
  Linux and macOS only). `--single-process` restores the old layout for
  debugging the engine. On Windows the browser puts itself and its content
  processes in a job object, so a browser that is killed or crashes
  natively takes them with it; on Unix a content process exits when its
  parent is gone. Servo itself only noticed a vanished content process
  when it next sent to it, so its tab stayed frozen; Paladin's Servo fork
  carries "constellation: report an exited content process as a crash of
  its event loop", which reports it at once.

- Web Animations API: `element.animate()`, `Animation` playback (`play`,
  `pause`, `cancel`, `finish`, `reverse`, `playbackRate`,
  `updatePlaybackRate`, `currentTime`, `startTime`, `playState`,
  `pending`, `ready`, `finished`, `onfinish`, `oncancel`),
  `KeyframeEffect` options, property-indexed keyframes and
  `getComputedTiming()`, and `getAnimations()` on elements and the
  document. Script animations are sampled by the style system each frame
  like CSS animations.
  Implemented as patches carried in Paladin's Servo and stylo forks.
- Cargo workspace with a Servo-free core crate, an engine crate, the `paladin`
  binary, and an `xtask` helper, with project conventions (README, CHANGELOG,
  CLAUDE.md, ROADMAP), the knight-on-pegasus logo with generated icons, and CI
  on macOS and Windows.
- Servo-free core model: windows and tabs, named commands with a pure
  `dispatch`, engine events with a pure `apply`, a keymap with macOS and
  Windows defaults, TOML settings, and per-OS profile directories. Profile
  names reject trailing dots and Windows device names (`CON`, `COM1`, and so
  on) on every platform, settings are saved atomically, and Back and Forward
  clear a crashed tab's error panel.
- Engine crate embedding Servo 0.7.0: one engine per process, a GL surface
  per window, a tab host that paints tabs offscreen, a headless test that
  loads a page without a window, and popups opened by `window.open` that keep
  their back and forward state and favicon when adopted.
- A browser window: tab strip with favicons, toolbar with back, forward,
  reload, and an address bar, Servo content rendered under the toolbar, a
  status overlay for link targets and engine errors, the logo as window and
  Dock icon, `--profile` and URL arguments, and a manual checklist in the
  README.
- Multiple windows, a settings window that persists the homepage, new-tab
  URL, and a restore option to the profile (the option is stored for a later
  phase and does not restore windows yet), keyboard shortcuts for every
  command with macOS and Windows defaults, and a window menu.
- Packaging with cargo-packager: `Paladin.app` and a `.dmg` on macOS, an NSIS
  installer on Windows, built and uploaded by CI.

### Changed

- The "☰" window menu and the tab context menu are native popup menus
  (`muda`) rather than egui menus, so they look and behave like the
  system's, show shortcuts the platform way, and work with screen readers.
  Their entries are built as data in `menus.rs` and still only issue
  `Command`s; everything else in the chrome stays egui.
- A new tab opened with "+" or the New tab shortcut puts the keyboard focus
  in the address bar with its text selected; tabs opened on a URL (popups)
  leave the focus on the page.
- Logging is set up by the engine when Servo starts (`RUST_LOG`, default
  `info`) instead of by the binary at launch, because Servo's crash
  reporting rides on its logger.

- Servo and stylo are built from Paladin's forks (`thomasmost/servo`
  `dist/paladin-0.7.0`, `thomasmost/stylo` `dist/paladin-0.22.0`) so the
  browser can carry its own engine patches.
- New logo artwork: the knight-on-pegasus illustration in `assets/logo.png`
  replaces the hand-drawn vector mark, framed by the same white rounded
  tile. All icon sizes are regenerated from it; the small ones are
  area-averaged from the 1024 px render rather than sampled, so they are
  smoother.

### Fixed

- A panic on one tab's script thread no longer ends the whole browser. Servo
  was started in its test-runner "hard fail" mode, which exits the process on
  any script panic without a crash report, and Paladin installed no panic
  hook, so the engine never heard of the panic. Now the crashed tab shows
  its error panel with the panic message (and `tab crashed: <reason>` on
  stderr) while the other tabs keep running. A native crash (an access
  violation) still takes the process down: Servo runs every tab in the
  one process on Windows and macOS.
- Windows: wikipedia.org (and any page that assigns `select.value`) no
  longer crashes the browser. Servo's `HTMLSelectElement.value` setter was
  compiled to read each option's next sibling in Windows release builds
  (servo/servo#48109); the option iterator is rewritten in Paladin's Servo
  fork. Release builds now carry line tables, and CI uploads the PDB or
  dSYM next to the installer so a crash in an installed build can be
  symbolized.
- Windows: the debug build of the workspace links again. The `script`
  crate's debug rlib had grown past 4 GB (rust-lang/rust#151184); it is
  now built with 16 codegen units, as in Servo's own workspace.
- Address bar: `localhost/path` and bare IPv4 addresses such as
  `192.168.1.1:8080` now parse, `localhostfoo.com` gets `https://`, and
  `javascript:` URLs are refused. Releasing one mouse button no longer ends
  a drag that another button still holds, and a popup tab the engine lost
  is closed instead of left blank.
- Settings window: the form is drawn on the panel fill and the framebuffer is
  cleared before each frame, so the background no longer shows undefined
  pixels (dark text on black in light mode, or stale pixels from a reused
  buffer). A crashed tab's error panel fills the page area the same way.
- Releasing a modifier such as Meta after a shortcut changed the active tab
  now goes to the tab that received its press, so no tab keeps a key down
  and the new tab never gets a lone release. A held key whose tab closed is
  dropped.
- Settings: the homepage and new-tab URL fields are stored normalised on
  commit, so `example.com` becomes `https://example.com/` as it does in the
  address bar. Text that is not a URL is kept as typed and marked "Not a
  URL".
- Windows release builds no longer open a console window next to the app;
  debug builds keep it for logs.
- Fatal startup errors (an invalid profile name, a profile directory or
  settings file that cannot be read, a bad URL argument, a failed engine or
  window start) are shown in a message box in Windows release builds, where
  stderr is invisible, and still go to stderr everywhere.
- `cargo packager --release -p paladin` builds the release binary before
  packaging, so it no longer bundles a stale `target/release/` binary.
- Servo's style threads get an 8 MiB stack, the size Servo's own builds
  use, so loading a page such as `wikipedia.org` no longer aborts the whole
  process with a stack overflow in a style thread.
