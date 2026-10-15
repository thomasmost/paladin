# Changelog

All notable changes to Paladin are documented in this file. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Paladin has no
releases yet; `Unreleased` holds everything on `main`.

## [Unreleased]

### Added

- Web Animations: `element.animate()` now plays its animation, and
  `Animation` implements the timing model (`play`, `pause`, `cancel`,
  `finish`, `reverse`, `playbackRate`, `updatePlaybackRate`, `currentTime`,
  `startTime`, `playState`, `pending`, `ready`, `finished`, `onfinish`,
  `oncancel`). Animations do not change rendered style yet. Carried as a
  patch in Paladin's Servo fork.
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

- Servo and stylo are built from Paladin's forks (`thomasmost/servo`
  `dist/paladin-0.7.0`, `thomasmost/stylo` `dist/paladin-0.22.0`) so the
  browser can carry its own engine patches. No patches yet; the engine is
  byte-for-byte upstream 0.7.0 / 0.22.0.
- New logo artwork: the knight-on-pegasus illustration in `assets/logo.png`
  replaces the hand-drawn vector mark, framed by the same white rounded
  tile. All icon sizes are regenerated from it; the small ones are
  area-averaged from the 1024 px render rather than sampled, so they are
  smoother.

### Fixed

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
