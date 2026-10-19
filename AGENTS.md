# Paladin

A desktop web browser for macOS and Windows. Servo renders pages; the
model, the egui chrome, and the winit application come from
[osmena](https://github.com/thomasmost/osmena), the shell Paladin shares
with Nautilus (Chromium). This repository holds only what is Servo's: the
engine crate, the forks, and the binary that brands it.

_Last updated: 2026-10-19 (moved onto osmena)._

## Architecture

Cargo workspace (Rust 1.97.1 pinned in `rust-toolchain.toml`, edition 2024).

| Crate                   | What it is |
| ----------------------- | ---------- |
| `crates/paladin-engine` | `paladin-engine` — the only crate that imports `servo`. Implements `osmena_engine::Engine` (`engine.rs`: `Engine`, one per process, the waker bridge, `INFO`, `run_content_process`), `osmena_engine::Surface` (`host.rs`: `GlSurface`, Servo's GL context on a native window, which the chrome shares) and `osmena_engine::TabHost` (`host.rs`: the tabs of one window painted offscreen and handed to the shell as a `PagePaint::Blit`). `delegate.rs` turns Servo's callbacks into `EngineEvent`s; `input.rs` turns `EngineInput` into Servo's events. |
| `crates/paladin`        | `paladin` — the binary: `brand.rs` (the `Brand`), `main.rs` (the `RUST_LOG` default, the content-process dispatch, clap, and `osmena_shell::run`), `build.rs` (the Windows icon resource), and the cargo-packager metadata in its `Cargo.toml`. |
| `xtask`                 | `cargo xtask icons` (through `osmena-xtask`: renders `assets/logo.svg`, which frames the `logo.png` artwork) and `cargo xtask angle-dlls` (Windows ANGLE DLLs next to the binaries). |

The osmena crates (`osmena-core`, `osmena-engine`, `osmena-shell`,
`osmena-xtask`) are git dependencies pinned by revision in the workspace
`Cargo.toml`; they version together, so move every `rev` at once. Their
docs describe the model, the chrome, the commands and events, and the
conventions that apply to all of it; this file covers Servo's side.

Data flow: osmena's chrome and keymap emit `Command`s; `dispatch` updates
the model and returns `EngineAction`s, which the shell forwards to this
crate's `TabHost`. Servo callbacks become `EngineEvent`s that `apply` folds
into the model. Core ids never meet Servo ids outside `paladin-engine`.

Local setup is in `README.md`.

## Commands

Run from the repo root:

- `cargo run -- [--profile <name>] [--single-process] [<url>...]` — run
  the browser (`--single-process`: every tab in the browser process). The
  workspace sets `default-members = ["crates/paladin"]`, so plain `cargo
  build`/`cargo test` touch only the binary; pass `--workspace` for the rest.
- `cargo test --workspace` — unit tests plus the two engine test binaries,
  `headless` and `web_animations` (slow the first time: it builds Servo).
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
  The release profile keeps line tables (`debug = "line-tables-only"`), so
  the build also leaves `paladin.pdb` (Windows) or `paladin.dSYM` (macOS)
  there; CI uploads it with the installer and it stays out of the bundle.

## Conventions

- **Servo stays in `paladin-engine`.** The binary never names a Servo
  type; the shell sees only osmena's types.
- **Shared behaviour changes in osmena.** A change to the model, the
  chrome, routing, menus, or the settings window is an osmena change:
  patch the osmena checkout in `~/.cargo/config.toml` (osmena's README),
  make it there with its tests, push, and bump the `rev`s here. Only what
  Servo needs lives in this repository.
- **Engine facts live in `INFO`.** `paladin_engine::INFO` names the
  engine and its version (bump it with the `servo` pin), its data
  directory (`servo/`), that it cannot stop a load, and its wheel
  distance; product strings live in `brand.rs`. Nothing else in this
  repository embeds a name.
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
- **Fork pitfalls.** Servo's own workspace resolves stylo from crates.io,
  so `cargo check` inside `~/vibes/servo` never sees the stylo fork's
  patches; compile-check Servo patches from Paladin with the local loop
  (`cargo check -p paladin-engine`). Plain `cargo fmt` rewrites hundreds of
  untouched places in both forks (Servo relies on the unstable options
  `./mach fmt` passes, listed in `python/servo/testing_commands.py`; stylo
  was formatted by an older rustfmt): in Servo pass those options after
  `cargo fmt -p <crate> --`, and in either fork commit only the hunks you
  wrote. `cargo update -p <servo|stylo> --precise` can also move unrelated
  lock edges (it once rewrote windows-sys edges); keep the `Cargo.lock` diff
  to the fork crates, by hand if needed, and validate with
  `cargo metadata --locked`.
- **Version pins move together, and osmena holds some of them.** osmena
  pins `winit`, `egui`, `egui-winit`, `egui_glow`, and `glow` to the
  versions Servo 0.7.0's own shell uses; this workspace pins `glow` and
  `surfman`. They share a GL context and raw window handles with Servo.
  To bump Servo: in `~/vibes/servo`, `git fetch upstream --tags`,
  `git checkout -b paladin/<new> paladin/<old>`, then `git rebase --onto
  v<new> v<old>` to replay the patch commits onto the new tag (drop any
  patch upstream has merged; `paladin/<old>` itself is not rewritten). Do
  the same steps in `~/vibes/stylo` for the stylo version that release's
  root `Cargo.toml` pins. Publish both dist branches with
  `etc/paladin/publish-dist.sh` and make each the fork's default branch
  (`gh repo edit thomasmost/<repo> --default-branch dist/paladin-<new>`);
  in Paladin set `servo = "=<new>"` in `crates/paladin-engine/Cargo.toml`
  and `INFO.version`, change the `branch = ...` values in the root
  `[patch.crates-io]` table (and the stylo crate list if the release's
  stylo workspace changed), set `glow` and `surfman` to the versions in
  that release's root `Cargo.toml`, set `rust-toolchain.toml` to its
  channel, run `cargo update`, then pin any pre-release crates to the
  versions in Servo's own `Cargo.lock` (0.7.0 needs
  `cargo update -p pkcs1 --precise 0.8.0-rc.4`). If the release moved
  winit, egui, or glow, bump them in osmena first (its pins are exact)
  and move the `rev`s here. Rebuild, run the manual checklist in
  `README.md`, and re-check the "Carried in the fork" entries in
  `docs/upstream-issues.md`. The first bump also squashes the review
  fix-up commits of the Web Animations series into one commit per logical
  change before republishing (the list is in that entry).
- **One `Engine` per process.** Servo's options are process-global, so each
  engine test binary (`headless`, `web_animations`) holds a single test
  function that builds one engine.
- **Web Animations pages report through the title.** Each page in
  `crates/paladin-engine/tests/pages/` sets `document.title` to `PASS` or
  `FAIL: <reason>` exactly once, with an error listener and, if it waits for
  frames or events, a timeout that fails it (the harness gives up after
  120 s). Pages are not discovered: add a new one to `PAGES` in
  `tests/web_animations.rs`, or it never runs.
- **The style-thread stack size is a build-time setting.** `.cargo/config.toml`
  exports `SERVO_STYLE_THREAD_STACK_SIZE_KB=8192` to every cargo invocation
  run from inside the repo; stylo reads it with `option_env!`, and its 512 KB
  default overflowed on wikipedia.org. Servo's own builds use the same value.
  Keep it when bumping Servo, and set it by hand if cargo runs from outside
  the repo. The same file sets `UV_NO_SYNC=1` so Servo's WebIDL codegen never
  syncs a Python environment into a path-patched checkout.
- **The debug `script` crate builds with 16 codegen units.** The root
  `Cargo.toml` sets `[profile.dev.package.servo-script] codegen-units = 16`
  because the default 256 push its debug rlib past 4 GB on Windows and the
  link fails (`docs/upstream-issues.md` T3). Keep it on every Servo bump
  until the rustc issue is fixed.
- **Tabs run in content processes.** `Engine::new(.., multiprocess: true)`
  sets Servo's `multiprocess` option; Servo then starts the current
  executable with `--content-process <token>` for each script thread, and
  `main` routes that argument to `paladin_engine::run_content_process`
  before touching profiles, logging, or windows. The processes are
  unsandboxed (`sandbox: false`; Servo's sandbox is Linux and macOS only).
  A content process that dies makes the constellation's next message to
  it fail, which Servo treats like a panic: crash page plus
  `notify_crashed`. The engine test binaries pass `false`: they do not
  handle the argument. `--single-process` is the debugging escape hatch.
- **The engine owns the logger and the panic hook.** `Engine::new` installs
  Servo's logger (`Servo::setup_logging`: `env_logger` plus a forwarder to
  the constellation) and a panic hook that logs the panic message at error
  level; an error logged on a panicking thread is how the constellation
  learns a script thread died, closes that tab's pipelines, reports
  `notify_crashed` (an `EngineEvent::TabCrashed`), and loads a crash page.
  `Opts::hard_fail` stays `false` (Servo's default `true` exits the process
  instead). Nothing else may initialise a logger: `log::set_boxed_logger`
  is once per process and the engine's call would panic. `main` only
  defaults `RUST_LOG` to `info`; osmena's shell installs no logger.
- **Packaging metadata** is `[package.metadata.packager]` in
  `crates/paladin/Cargo.toml`. cargo-packager resolves its paths against that
  crate's directory (hence `../../`). It deliberately lists no `formats`
  (0.11.8 fails on formats the host can't build); the per-host default is
  app+dmg on macOS, NSIS on Windows. The `target/release/*.dll` resources glob
  assumes the default target dir and no `--target`.
- **Generated icons are committed.** Replace `assets/logo.png` (the
  artwork) or edit `assets/logo.svg` (the tile that frames it; no `<text>`,
  osmena's xtask builds resvg without font support), run `cargo xtask
  icons`, commit the result. Never hand-edit `assets/icons/`.
- **Format before every commit:** `cargo fmt --all`. CI fails on formatting
  or clippy warnings.
- **`AGENTS.md` mirrors this file.** Keep them identical.

## Definition of Done

A task is complete only when:

1. **All tests pass** — `cargo test --workspace` is green, and the manual
   checklist in `README.md` passes on macOS for any change to the engine
   glue or to the osmena revision.
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
