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
  Windows defaults, TOML settings, and per-OS profile directories.
