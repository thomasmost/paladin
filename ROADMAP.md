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
limits the cost of that revisit. The upstream issues each phase depends on
are tracked in `docs/upstream-issues.md`.

## Next design plans

The Servo fork and Web Animations plan
(`docs/design-plans/2026-10-15-servo-fork-and-web-animations.md`) gave
Paladin a way to carry engine patches. Two design plans follow it, in
order, on the way to Phase 3:

1. **Isolated worlds in Servo.** Run scripts against the page DOM from a
   separate JavaScript global, either per-world DOM wrappers or a sandbox
   global with filtering wrappers; an embedder API to run a script in a
   named world of a frame, per-world `customElements`, `postMessage`
   between worlds, a host message port, and user scripts moved into an
   isolated world.
2. **WebExtension runtime in Paladin.** Unpacked extension loading, the
   Manifest V3 background service worker, the `chrome.*` subset Proton
   Pass uses, popup and options pages, `web_accessible_resources`.
   Acceptance: Proton Pass installs unpacked, signs in, autofills, and
   saves a login.
