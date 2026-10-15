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
