# Paladin

A desktop web browser for macOS and Windows. Servo renders pages, egui draws
the chrome, winit owns the windows. Everything that touches Servo lives in one
crate so the engine could be replaced without rewriting the model or the UI.

_Last updated: 2026-10-15 (forks)._

## Architecture

Cargo workspace (Rust 1.97.1 pinned in `rust-toolchain.toml`, edition 2024).

| Crate                   | What it is |
| ----------------------- | ---------- |
| `crates/paladin-core`   | `paladin-core` — the Servo-free model: `model` (`Browser`/`Window`/`Tab`), `command` (`Command`, `dispatch`, `EngineAction`), `event` (`EngineEvent`, `apply`), `keymap` (`Keymap`, `KeyChord`, `CommandKind`), `settings` (TOML), `profile` (paths). Depends on neither servo, egui, nor winit. |
| `crates/paladin-engine` | `paladin-engine` — the only crate that imports `servo`. `engine.rs` (`Engine`, one per process), `host.rs` (`GlSurface`, a window's GL context; `TabHost`, the tabs of one window painted offscreen), `input.rs` (`EngineInput`), `delegate.rs` (Servo callbacks → `EngineEvent`). Re-exports `keyboard_types` and `PhysicalSize`. |
| `crates/paladin`        | `paladin` — the binary: `app.rs` (winit `App`), `window.rs` (`BrowserWindow`, egui chrome over the tab host), `settings_window.rs`, `chrome.rs` (pure chrome drawing that returns `Command`s), `routing.rs` (pure, unit-tested pointer/key routing and page-area geometry), `input.rs` (winit → `EngineInput`, `route_key`), `keyutils.rs` (winit keys → `keyboard-types`, copied from servoshell, MPL-2.0 header must stay), `icon.rs`, and the cargo-packager metadata in its `Cargo.toml`. |
| `xtask`                 | `cargo xtask icons` (renders `assets/logo.svg`, which frames the `logo.png` artwork, at 1024 px and area-averages the smaller icns/ico/png sizes from it) and `cargo xtask angle-dlls` (Windows ANGLE DLLs next to the binaries). |

Data flow: the chrome and the keymap emit `Command`s; `paladin_core::dispatch`
updates the model and returns `EngineAction`s, which the app forwards to the
window's `TabHost` or handles itself (open and close windows, settings, quit).
Servo callbacks become `EngineEvent`s that `paladin_core::apply` folds into the
model. Both functions are pure and unit-tested; core ids never meet Servo ids
outside `paladin-engine`.

Local setup is in `README.md`.

## Commands

Run from the repo root:

- `cargo run -- [--profile <name>] [<url>...]` — run the browser. The
  workspace sets `default-members = ["crates/paladin"]`, so plain `cargo
  build`/`cargo test` touch only the binary; pass `--workspace` for the rest.
- `cargo test --workspace` — unit tests plus the headless engine test (slow
  the first time: it builds Servo).
- `cargo clippy --workspace --all-targets -- -D warnings` — lint.
- `cargo fmt --all` — format; CI runs `cargo fmt --all --check`.
- `cargo xtask icons` — regenerate `assets/icons/` from `assets/logo.svg`
  and the `assets/logo.png` it frames.
- `cargo xtask angle-dlls [debug|release]` — Windows: copy `libEGL.dll` and
  `libGLESv2.dll` next to the executables after a build.
- Packaging (see `README.md`): `cargo build --release -p paladin`, then on
  Windows `cargo xtask angle-dlls release` (the installer's DLL resources come
  from this step), then `cargo packager --release -p paladin`, which re-runs
  the release build (its `before-packaging-command`) and produces the
  `.app`/`.dmg` (macOS) or NSIS installer (Windows) in `target/release/`.

## Conventions

- **Servo stays in `paladin-engine`.** The model and the UI only see
  `Command`, `EngineAction`, `EngineEvent`, `EngineInput`, and core ids.
- **Servo and stylo come from Paladin's forks.** `thomasmost/servo` and
  `thomasmost/stylo` each have a dev branch (`paladin/0.7.0`,
  `paladin/0.22.0`) based on the upstream release tag that holds Paladin's
  patch commits, and an append-only dist branch (`dist/paladin-0.7.0`,
  `dist/paladin-0.22.0`) that `etc/paladin/publish-dist.sh` in each fork
  builds from the dev branch with only the files a build needs. The root
  `Cargo.toml` patches every crate of both repositories to the dist
  branches; `Cargo.lock` pins the dist commit. Patches are one commit per
  logical change in Servo's style (MPL header, `cargo fmt`, no
  Paladin-specific names) so rebases stay mechanical. To publish a change:
  commit on the dev branch, push it, run
  `etc/paladin/publish-dist.sh dist/paladin-<version>` (prints the dist
  sha), then in Paladin `cargo update -p servo --precise <sha>` (moves all
  60 Servo crates) or `cargo update -p stylo --precise <sha>` (all 11 stylo
  crates). Never rewrite a published dist branch: cargo fetches only that
  branch, and a locked sha that disappears from it breaks every older
  checkout.
- **Local loop for fork changes.** Clone the forks to `~/vibes/servo` and
  `~/vibes/stylo` (dev branches checked out) and put a `[patch.crates-io]`
  table in `~/.cargo/config.toml` pointing at the checkout; config patches
  override the manifest's for the same package. For Servo one entry is
  enough (`servo = { path = "/Users/<you>/vibes/servo/components/servo" }`;
  the other 59 are path dependencies of it and cargo warns that their
  manifest patches are unused). For stylo list all eleven
  (`stylo = { path = ".../stylo/style" }`, `stylo_atoms`
  (`stylo_atoms`), `stylo_derive` (`style_derive`), `stylo_dom`
  (`stylo_dom`), `stylo_malloc_size_of` (`malloc_size_of`),
  `stylo_static_prefs` (`stylo_static_prefs`), `stylo_traits`
  (`style_traits`), `selectors`, `servo_arc`, `to_shmem`,
  `to_shmem_derive`), or the graph holds two copies of the siblings. The
  first build from a path recompiles the whole repository; later edits
  rebuild only the changed crates. The lock then records path sources: run
  `git checkout Cargo.lock` before committing, and remove the config table
  (or comment it out) to go back to the dist branches.
- **Version pins move together.** `servo`, `winit`, `egui`, `egui-winit`,
  `egui_glow`, `glow`, and `surfman` are pinned to exact versions in the
  workspace `Cargo.toml` because they share a GL context and raw window
  handles. To bump Servo: in `~/vibes/servo`, `git fetch upstream --tags`,
  branch `paladin/<new>` from the new tag, `git rebase --onto v<new>
  v<old> paladin/<old>` the patch commits onto it (drop any upstream has
  merged), do the same in `~/vibes/stylo` for the stylo version that
  release's root `Cargo.toml` pins, publish both dist branches; in Paladin
  set `servo = "=<new>"` in `crates/paladin-engine/Cargo.toml`, change the
  `branch = ...` values in the root `[patch.crates-io]` table (and the
  stylo crate list if the release's stylo workspace changed), set the
  other pins to the versions in that release's root `Cargo.toml` and
  `ports/servoshell/Cargo.toml`, set `rust-toolchain.toml` to its channel,
  run `cargo update`, then pin any pre-release crates to the versions in
  Servo's own `Cargo.lock` (0.7.0 needs
  `cargo update -p pkcs1 --precise 0.8.0-rc.4`), rebuild, run the manual
  checklist in `README.md`, and re-check the "Carried in the fork" entries
  in `docs/upstream-issues.md`.
- **One `Engine` per process.** Servo's options are process-global, so the
  engine test binary holds a single test function.
- **The style-thread stack size is a build-time setting.** `.cargo/config.toml`
  exports `SERVO_STYLE_THREAD_STACK_SIZE_KB=8192` to every cargo invocation
  run from inside the repo; stylo reads it with `option_env!`, and its 512 KB
  default overflowed on wikipedia.org. Servo's own builds use the same value.
  Keep it when bumping Servo, and set it by hand if cargo runs from outside
  the repo. The same file sets `UV_NO_SYNC=1` so Servo's WebIDL codegen never
  syncs a Python environment into a path-patched checkout.
- **Every user action is a `Command`.** New chrome controls and shortcuts add
  a `Command` (and a `CommandKind` if it needs a hotkey), never a direct
  engine call.
- **Key routing has one rule.** It lives in `input::route_key` (Cmd/Ctrl
  chords fire even in the address bar; otherwise egui while focused, then
  bound chords on initial press, then the page). Browser windows apply it via
  `routing::KeyRouter`, which sends repeats and releases to the side that got
  the press. Change routing there, with a unit test, not in `window.rs`.
- **Packaging metadata** is `[package.metadata.packager]` in
  `crates/paladin/Cargo.toml`. cargo-packager resolves its paths against that
  crate's directory (hence `../../`). It deliberately lists no `formats`
  (0.11.8 fails on formats the host can't build); the per-host default is
  app+dmg on macOS, NSIS on Windows. The `target/release/*.dll` resources glob
  assumes the default target dir and no `--target`.
- **Generated icons are committed.** Replace `assets/logo.png` (the
  artwork) or edit `assets/logo.svg` (the tile that frames it; no `<text>`,
  the xtask builds resvg without font support), run `cargo xtask icons`,
  commit the result. Never hand-edit `assets/icons/`.
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
- `docs/upstream-issues.md` tracks the Servo, egui, winit, and toolchain
  issues Paladin depends on and the features each one blocks. Add an entry
  when a bug or feature turns out to be upstream; re-check every `S` entry on
  a Servo bump.
