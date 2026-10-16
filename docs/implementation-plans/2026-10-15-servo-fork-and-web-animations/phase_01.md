# Servo Fork and Web Animations Implementation Plan

**Goal:** Paladin builds from public `thomasmost/servo` and `thomasmost/stylo`
forks with zero patches, locally and in CI, and the fork maintenance
procedure is written down.

**Architecture:** Each fork has a dev branch (`paladin/<version>`) based on
the upstream release tag and an append-only dist branch
(`dist/paladin-<version>`) holding only the files a dependent build needs.
Paladin's root `Cargo.toml` redirects every crate from the two repositories
to the dist branches with `[patch.crates-io]`; the existing exact version
pins keep working because the fork manifests keep the upstream versions.

**Tech Stack:** cargo 1.97.1 (`[patch.crates-io]`, git sources), git
plumbing (`ls-tree`, `update-index`, `commit-tree`) for the dist publisher,
`gh` for the forks, GitHub Actions with `Swatinem/rust-cache`.

**Scope:** 6 phases from the design plan
(`docs/design-plans/2026-10-15-servo-fork-and-web-animations.md`); this file
is Phase 1 of 6.

**Codebase verified:** 2026-10-15 (Paladin at `628dfdf` on `vnext`; Servo
0.7.0 and stylo 0.22.0 sources in the cargo registry; servo/servo tag
`v0.7.0` = `aac43a3f31a259f04a574f5ec4e959c943ad7cc7`; servo/stylo tag
`v0.22.0` = `34d25f741765a0bdb894f8271852e633299352d8`).

---

## Corrections to the design, found during verification

Read these before executing; the tasks below already apply them.

1. **The stylo patch set is 11 crates, not 7.** In the stylo repository
   `style/Cargo.toml` pulls `selectors 0.41.0`, `servo_arc 0.5.0`,
   `to_shmem 0.6.0` and `to_shmem_derive 0.2.0` through workspace `path`
   entries. When `stylo` comes from git those four come from the same git
   source, while Servo's crates request them from crates.io, so without
   patching them too the graph holds two copies of each and compilation
   fails with "multiple different versions of crate". Paladin's `Cargo.lock`
   already carries exactly those versions, so patching them to the stylo
   fork is a no-op for the code.
2. **Every Servo crate in the lock comes from servo/servo.** All 60
   (`servo` plus 59 `servo-*`, including `servo-media-ohos` and
   `servo-tracing`) live under `components/`. `servo_arc` is the one
   `servo*`-named crate that comes from the stylo repository instead.
3. **Dist branches are append-only.** Cargo fetches only
   `refs/heads/<branch>`; when a locked sha is no longer reachable from the
   branch it falls back to fetching every branch and tag of the fork (about
   2 GB for Servo) and then fails. Re-creating the orphan branch on each
   publish would break every older Paladin commit. The publish script
   therefore commits the new pruned tree on top of the previous dist commit.
4. **`pyproject.toml`, `uv.lock` and `python/` are not needed by any build
   script.** `components/script_bindings/build.rs` runs
   `codegen/run.py` with everything it needs inside the crate directory.
   Those files are dropped from the dist branch, which makes `UV_NO_SYNC`
   irrelevant for the dist source; `UV_NO_SYNC = "1"` still matters for the
   local path-patch loop, where the build script runs inside a full checkout
   that does contain `pyproject.toml`.
5. **Patching `servo` alone would be enough** (the sibling crates are path
   dependencies of `components/servo`), but a patch entry per crate is
   harmless, keeps the mapping explicit, and protects against a future
   direct dependency on a `servo-*` crate resolving to crates.io. All 60
   are listed.

## Prerequisites

- `gh` authenticated as `thomasmost` (verified), SSH access to GitHub.
- About 3 GB of disk for the Servo clone and the usual Paladin target dir.
- Python 3.11+ and `uv` on PATH (both present; `uv 0.12.1` at
  `/opt/homebrew/bin/uv`).

---

<!-- START_SUBCOMPONENT_A (tasks 1-4) -->
<!-- START_TASK_1 -->
### Task 1: Create the two GitHub forks

**Files:** none in this repository.

**Step 1: Fork both repositories without cloning**

```bash
gh repo fork servo/servo --clone=false
gh repo fork servo/stylo --clone=false
```

Expected: `✓ Created fork thomasmost/servo` and `✓ Created fork
thomasmost/stylo`. Forks copy all branches and tags, so `v0.7.0` and
`v0.22.0` exist on the forks.

**Step 2: Verify**

```bash
gh api repos/thomasmost/servo --jq '.fork, .default_branch'
gh api repos/thomasmost/stylo --jq '.fork, .default_branch'
gh api repos/thomasmost/servo/git/ref/tags/v0.7.0 --jq .object.sha
gh api repos/thomasmost/stylo/git/ref/tags/v0.22.0 --jq .object.sha
```

Expected: `true` / `main` for both; the tag shas are
`aac43a3f31a259f04a574f5ec4e959c943ad7cc7` and
`34d25f741765a0bdb894f8271852e633299352d8`.

GitHub disables workflow runs on forks by default; do not enable them (the
design keeps Paladin's CI as the only gate).

**Step 3: Nothing to commit.**
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: Servo dev branch and the dist publisher

**Files:**
- Create: `~/vibes/servo` (clone of `thomasmost/servo`)
- Create (in that clone): `etc/paladin/publish-dist.sh`

**Step 1: Clone the fork and add upstream**

A full clone (about 2 GB) keeps rebases and the publisher simple; it is a
one-time cost.

```bash
git clone git@github.com:thomasmost/servo.git ~/vibes/servo
cd ~/vibes/servo
git remote add upstream https://github.com/servo/servo.git
git checkout -b paladin/0.7.0 v0.7.0
git rev-parse HEAD
```

Expected: the last line prints `aac43a3f31a259f04a574f5ec4e959c943ad7cc7`.

**Step 2: Create the publisher**

Create `~/vibes/servo/etc/paladin/publish-dist.sh` with exactly this
content:

```bash
#!/usr/bin/env bash
# Publish the checked-out commit as a pruned "dist" commit that cargo can
# fetch cheaply. The dist branch holds only what a dependent build needs and
# its history is independent of the dev branch. Publishes append to the dist
# branch and never rewrite it: cargo locks the commit sha, and a sha that
# leaves the branch makes every older lockfile that references it
# unbuildable.
#
# Usage: etc/paladin/publish-dist.sh <dist-branch> [remote]
# Prints the new dist commit sha on stdout.
set -euo pipefail

dist_branch=${1:?usage: publish-dist.sh <dist-branch> [remote]}
remote=${2:-origin}

cd "$(git rev-parse --show-toplevel)"
if [ -n "$(git status --porcelain)" ]; then
    echo "publish-dist: the working tree is not clean" >&2
    exit 1
fi
dev_commit=$(git rev-parse HEAD)
dev_branch=$(git rev-parse --abbrev-ref HEAD)

# Everything a dependent build needs. Every path dependency reachable from
# components/servo must be present (all of components/); the other workspace
# members only have to parse. Dropped: tests/wpt, python/, pyproject.toml,
# uv.lock, docs, support, third_party, the mach tooling.
keep=(
    Cargo.toml
    Cargo.lock
    rust-toolchain.toml
    components
    ports/servoshell
    ffi
    tests/capi
    tests/unit
    resources
    etc/paladin
)

git fetch "$remote" "refs/heads/$dist_branch:refs/remotes/$remote/$dist_branch" 2>/dev/null || true
parent=$(git rev-parse --verify -q "refs/remotes/$remote/$dist_branch" || true)

index=$(mktemp)
trap 'rm -f "$index"' EXIT
export GIT_INDEX_FILE=$index
git read-tree --empty
git ls-tree -r --full-tree HEAD -- "${keep[@]}" | git update-index --index-info
tree=$(git write-tree)
unset GIT_INDEX_FILE

message="dist: $dev_branch at ${dev_commit:0:12}

Built by etc/paladin/publish-dist.sh from $dev_commit."
if [ -n "$parent" ]; then
    commit=$(git commit-tree "$tree" -p "$parent" -m "$message")
else
    commit=$(git commit-tree "$tree" -m "$message")
fi
git push "$remote" "$commit:refs/heads/$dist_branch"
echo "$commit"
```

```bash
chmod +x ~/vibes/servo/etc/paladin/publish-dist.sh
```

**Step 3: Dry-run the pruning without pushing**

```bash
cd ~/vibes/servo
export GIT_INDEX_FILE=$(mktemp)
git read-tree --empty
git ls-tree -r --full-tree HEAD -- Cargo.toml Cargo.lock rust-toolchain.toml components ports/servoshell ffi tests/capi tests/unit resources | git update-index --index-info
git ls-files | wc -l
git ls-files | grep -c '^tests/wpt/' || true
git ls-files | grep -c '^components/script_bindings/codegen/run.py$'
unset GIT_INDEX_FILE
```

Expected: a few thousand files, `0` wpt files, and `1` for `run.py`.

**Step 4: Commit and push the dev branch**

```bash
cd ~/vibes/servo
git add etc/paladin/publish-dist.sh
git commit -m "Add the dist branch publisher used by Paladin"
git push -u origin paladin/0.7.0
```

Expected: branch `paladin/0.7.0` exists on `thomasmost/servo` one commit
ahead of `v0.7.0`.
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: Publish the Servo dist branch

**Files:** none in this repository.

**Step 1: Publish**

```bash
cd ~/vibes/servo
etc/paladin/publish-dist.sh dist/paladin-0.7.0 | tee /tmp/servo-dist-sha
```

Expected: a push that creates `dist/paladin-0.7.0` and a 40-hex sha on
stdout. Keep that sha; Task 6 pins it.

**Step 2: Verify the branch contents on GitHub**

```bash
SHA=$(cat /tmp/servo-dist-sha)
gh api "repos/thomasmost/servo/git/commits/$SHA" --jq '.parents | length'
gh api "repos/thomasmost/servo/contents/Cargo.toml?ref=dist/paladin-0.7.0" --jq .size
gh api "repos/thomasmost/servo/contents/components/script_bindings/build.rs?ref=dist/paladin-0.7.0" --jq .size
gh api "repos/thomasmost/servo/contents/pyproject.toml?ref=dist/paladin-0.7.0" 2>&1 | head -1
```

Expected: `0` parents (first dist commit), non-zero sizes for the two kept
files, and a `Not Found` error for `pyproject.toml`.

**Step 3: Make the fork's default branch the dist branch**

This keeps cargo's all-branches fallback and casual clones small.

```bash
gh repo edit thomasmost/servo --default-branch dist/paladin-0.7.0
```

Expected: no error.
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: Stylo dev branch, publisher, and dist branch

**Files:**
- Create: `~/vibes/stylo` (clone of `thomasmost/stylo`)
- Create (in that clone): `etc/paladin/publish-dist.sh`

**Step 1: Clone and branch**

```bash
git clone git@github.com:thomasmost/stylo.git ~/vibes/stylo
cd ~/vibes/stylo
git remote add upstream https://github.com/servo/stylo.git
git checkout -b paladin/0.22.0 v0.22.0
git rev-parse HEAD
```

Expected: `34d25f741765a0bdb894f8271852e633299352d8`.

**Step 2: Create the publisher**

Copy the script from Task 2 to `~/vibes/stylo/etc/paladin/publish-dist.sh`
and replace the `keep=( ... )` block (and its comment) with:

```bash
# Everything a dependent build needs: the workspace manifest and all eleven
# crates (style/Cargo.toml has path dependencies on selectors, servo_arc,
# to_shmem and to_shmem_derive). README.md is the `readme` of style/.
# Dropped: .github, the sync scripts.
keep=(
    Cargo.toml
    README.md
    rustfmt.toml
    malloc_size_of
    selectors
    servo_arc
    style
    style_derive
    style_traits
    stylo_atoms
    stylo_dom
    stylo_static_prefs
    to_shmem
    to_shmem_derive
    etc/paladin
)
```

```bash
chmod +x ~/vibes/stylo/etc/paladin/publish-dist.sh
cd ~/vibes/stylo
git add etc/paladin/publish-dist.sh
git commit -m "Add the dist branch publisher used by Paladin"
git push -u origin paladin/0.22.0
```

**Step 3: Publish and verify**

```bash
cd ~/vibes/stylo
etc/paladin/publish-dist.sh dist/paladin-0.22.0 | tee /tmp/stylo-dist-sha
SHA=$(cat /tmp/stylo-dist-sha)
gh api "repos/thomasmost/stylo/contents/style/Cargo.toml?ref=dist/paladin-0.22.0" --jq .size
gh api "repos/thomasmost/stylo/contents/selectors/Cargo.toml?ref=dist/paladin-0.22.0" --jq .size
gh repo edit thomasmost/stylo --default-branch dist/paladin-0.22.0
```

Expected: two non-zero sizes; no errors.
<!-- END_TASK_4 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_SUBCOMPONENT_B (tasks 5-7) -->
<!-- START_TASK_5 -->
### Task 5: Build-time setting for the local path-patch loop

**Files:**
- Modify: `.cargo/config.toml` (append to the `[env]` table, after line 10)

**Step 1: Append the setting**

Add to the end of `.cargo/config.toml` (inside the existing `[env]` table):

```toml
# servo-script-bindings' build script runs its WebIDL codegen with
# `uv run --frozen python` when uv is installed. Inside a full Servo checkout
# (the local path-patch loop, see CLAUDE.md) that would sync Servo's Python
# environment into the checkout first; the codegen needs only the standard
# library, so skip the sync. Harmless for the dist branches, which carry no
# pyproject.toml (uv prints a one-line "no effect" warning).
UV_NO_SYNC = "1"
```

**Step 2: Verify the file parses**

```bash
cargo metadata --format-version 1 --no-deps > /dev/null && echo ok
```

Expected: `ok`.

**Step 3: Commit** (together with Task 6, see there).
<!-- END_TASK_5 -->

<!-- START_TASK_6 -->
### Task 6: Point every Servo and stylo crate at the dist branches

**Files:**
- Modify: `Cargo.toml` (root; append a `[patch.crates-io]` table after
  line 29)
- Modify: `Cargo.lock` (regenerated by cargo)

**Step 1: Append the patch table**

Append to the root `Cargo.toml`:

```toml
# Servo and stylo build from Paladin's forks so the browser can carry its own
# patches. The dist branches hold only the files a build needs; the dev
# branches (paladin/<version>) hold the patch commits. The fork manifests
# keep the upstream versions, so the exact pins above and in
# crates/paladin-engine/Cargo.toml still apply. Move all crates of one repo
# at once with `cargo update -p servo --precise <sha>` or
# `cargo update -p stylo --precise <sha>`. See CLAUDE.md for the bump and
# local-loop procedures.
[patch.crates-io]
servo = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-allocator = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-background-hang-monitor = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-background-hang-monitor-api = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-base = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-canvas = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-canvas-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-config = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-config-macro = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-constellation = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-constellation-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-default-resources = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-deny-public-fields = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-devtools = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-devtools-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-dom-struct = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-embedder-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-fonts = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-fonts-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-geometry = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-hyper-serde = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-jstraceable-derive = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-layout = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-layout-api = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-malloc-size-of = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-audio = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-derive = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-dummy = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-ohos = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-player = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-streams = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-thread = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-media-webrtc = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-metrics = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-net = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-net-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-paint = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-paint-api = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-pixels = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-profile = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-profile-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-script = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-script-bindings = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-script-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-script-webgpu = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-storage = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-storage-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-timers = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-tracing = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-url = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-wakelock = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-webgl = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-webgpu = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-webgpu-traits = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-webvtt = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-webxr = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-webxr-api = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
servo-xpath = { git = "https://github.com/thomasmost/servo.git", branch = "dist/paladin-0.7.0" }
# stylo's style/ crate has path dependencies on selectors, servo_arc, to_shmem
# and to_shmem_derive, so those must come from the same source or the graph
# holds two copies of each.
stylo = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
stylo_atoms = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
stylo_derive = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
stylo_dom = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
stylo_malloc_size_of = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
stylo_static_prefs = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
stylo_traits = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
selectors = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
servo_arc = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
to_shmem = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
to_shmem_derive = { git = "https://github.com/thomasmost/stylo.git", branch = "dist/paladin-0.22.0" }
```

**Step 2: Re-resolve the patched packages and measure the fetch**

```bash
cargo fetch 2>&1 | tail -5
du -sh ~/.cargo/git/db/servo-* ~/.cargo/git/db/stylo-* ~/.cargo/git/checkouts/servo-* ~/.cargo/git/checkouts/stylo-*
```

Expected: cargo fetches the two git sources once each; the Servo db and
checkout are tens of MB, not gigabytes (the design's reason for the dist
branch). Write the four numbers down for the CI budget comment in Task 8.

**Step 3: Verify the lock now points at the dist commits**

```bash
grep -c 'git+https://github.com/thomasmost/servo.git?branch=dist/paladin-0.7.0#' Cargo.lock
grep -c 'git+https://github.com/thomasmost/stylo.git?branch=dist/paladin-0.22.0#' Cargo.lock
grep -A1 '^name = "servo"$' Cargo.lock | grep -o '#[0-9a-f]*'
grep -A1 '^name = "stylo"$' Cargo.lock | grep -o '#[0-9a-f]*'
grep -c 'name = "servo' Cargo.lock
cat /tmp/servo-dist-sha /tmp/stylo-dist-sha
```

Expected: `60`, `11`, the two shas printed by the publishers, and still 61
`servo*` packages (no duplicates). If the lock sha differs from the
published one (someone published again in between), pin it:
`cargo update -p servo --precise $(cat /tmp/servo-dist-sha)`.

**Step 4: Build everything**

```bash
cargo build --workspace --all-targets 2>&1 | tail -3
```

Expected: `Finished` with no errors. This rebuilds all Servo and stylo
crates (new source), about the same time as the first-ever build. A
`warning: --no-sync has no effect when used outside of a project` line from
uv is expected.

**Step 5: Commit Tasks 5 and 6**

```bash
cargo fmt --all
git add .cargo/config.toml Cargo.toml Cargo.lock
git commit -m "build: take Servo and stylo from Paladin's forks' dist branches"
```
<!-- END_TASK_6 -->

<!-- START_TASK_7 -->
### Task 7: Verify the workspace against the forks

**Files:** none.

**Step 1: Tests, clippy, fmt**

```bash
cargo test --workspace 2>&1 | grep -E '^test result|FAILED|panicked' 
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo fmt --all --check && echo fmt-ok
cargo xtask icons && git diff --exit-code -- assets/icons && echo icons-ok
```

Expected: every `test result:` line says `ok` (including the headless engine
test in `paladin-engine`), clippy ends with `Finished`, `fmt-ok`,
`icons-ok`.

**Step 2: Manual checklist**

Run `cargo run` and go through the 11 items of the manual checklist in
`README.md` (lines 72-128). Nothing in the chrome changed; this proves the
fork-sourced engine behaves as before.

Expected: all 11 items pass.

**Step 3: Nothing to commit** (the tree is unchanged by this task).
<!-- END_TASK_7 -->
<!-- END_SUBCOMPONENT_B -->

<!-- START_SUBCOMPONENT_C (tasks 8-9) -->
<!-- START_TASK_8 -->
### Task 8: Documentation of the fork procedures

**Files:**
- Modify: `CLAUDE.md` (lines 7, 54-63; new bullets after line 63)
- Modify: `AGENTS.md` (copy of `CLAUDE.md`)
- Modify: `README.md` (lines 36-62 "Local development")
- Modify: `docs/upstream-issues.md` (lines 10-23 and before line 280)
- Modify: `CHANGELOG.md` (under `## [Unreleased]` / `### Changed`, line 37)

**Step 1: `CLAUDE.md`**

Replace line 7 with:

```markdown
_Last updated: 2026-10-15 (forks)._
```

Replace the "Version pins move together" bullet (lines 54-63) with these
three bullets:

```markdown
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
```

Also add `UV_NO_SYNC` to the style-thread bullet: after the sentence ending
"set it by hand if cargo runs from outside the repo." (line 71) append:

```markdown
  The same file sets `UV_NO_SYNC=1` so Servo's WebIDL codegen never syncs
  a Python environment into a path-patched checkout.
```

Then:

```bash
cp CLAUDE.md AGENTS.md
diff CLAUDE.md AGENTS.md && echo identical
```

**Step 2: `README.md`**

In the "Local development" section, after the prerequisites list (line 44)
and before the build steps, add:

```markdown
Servo and stylo are built from Paladin's forks (`thomasmost/servo`,
`thomasmost/stylo`): the root `Cargo.toml` patches their crates to small
`dist/*` branches that cargo fetches like any git dependency, so no extra
setup is needed. `CLAUDE.md` describes how to work on the forks.
```

**Step 3: `docs/upstream-issues.md`**

In "How to maintain this file", after line 18 ("...now unblocked.") add a
bullet:

```markdown
- An issue fixed by a patch in Paladin's Servo or stylo fork moves to the
  "Carried in the fork" section below with the patch commit's subject; on
  every Servo bump re-check whether upstream has merged it and, if so, drop
  the patch during the rebase and move the entry to "Resolved".
```

Before `## Resolved` (line 280) add:

```markdown
## Carried in the fork

Patches on `thomasmost/servo` (`paladin/0.7.0`) and `thomasmost/stylo`
(`paladin/0.22.0`), each listed against the upstream issue it addresses.

None yet.

```

**Step 4: `CHANGELOG.md`**

Under `## [Unreleased]` / `### Changed` add as the first bullet:

```markdown
- Servo and stylo are built from Paladin's forks (`thomasmost/servo`
  `dist/paladin-0.7.0`, `thomasmost/stylo` `dist/paladin-0.22.0`) so the
  browser can carry its own engine patches. No patches yet; the engine is
  byte-for-byte upstream 0.7.0 / 0.22.0.
```

**Step 5: Commit**

```bash
git add CLAUDE.md AGENTS.md README.md docs/upstream-issues.md CHANGELOG.md
git commit -m "docs: describe the Servo and stylo forks and the bump procedure"
```
<!-- END_TASK_8 -->

<!-- START_TASK_9 -->
### Task 9: CI run and cache budget

**Files:**
- Modify (only if needed): `.github/workflows/ci.yml` (lines 58-72, the
  cache budget comment)

**Step 1: Push and watch**

`vnext` has no CI on push (CI triggers on `main` and pull requests). Open a
pull request from `vnext` to `main` so the `check` job runs on both hosts:

```bash
git push origin vnext
gh pr create --base main --head vnext --title "Servo fork bootstrap" --body "Phase 1 of docs/design-plans/2026-10-15-servo-fork-and-web-animations.md: build from the forks with zero patches." 
gh pr checks --watch
```

Expected: `check (macos-15)` and `check (windows-2025)` pass. The Windows
LLVM install step failed on infrastructure in the most recent `main` run;
if it fails again with the same `KyleMayes/install-llvm-action` error,
re-run the job once with `gh run rerun <id> --failed` before treating it
as a Paladin problem.

**Step 2: Measure the caches**

```bash
gh cache list --limit 20 --json key,sizeInBytes --jq '.[] | "\(.sizeInBytes/1048576|floor) MB \(.key)"'
```

Expected: the new `check-*` caches are within a few hundred MB of the
previous ones (1873 MB macOS, 2815 MB Windows measured on 2026-10-15) and
the total stays under 10 GB after old keys are evicted.

**Step 3: Update the budget comment if the numbers moved**

If the measured sizes differ from the comment in `.github/workflows/ci.yml`
lines 58-63 by more than a few hundred MB, update the numbers and the date
in that comment, then:

```bash
git add .github/workflows/ci.yml
git commit -m "ci: record the cache sizes measured with the fork sources"
git push origin vnext
```

**Step 4: Hand the pull request to the user.** They close or merge it once
Phase 1 is verified (an open pull request would re-run the three-hour CI
on every later push to `vnext`). Phases 2 to 5 push to `vnext` without
CI; Phase 6 opens a new pull request for the final verification.
<!-- END_TASK_9 -->
<!-- END_SUBCOMPONENT_C -->

## Phase 1 done when

- `cargo build --workspace`, `cargo test --workspace`, clippy and fmt are
  clean with `Cargo.lock` pointing every Servo and stylo crate at the dist
  branches (Task 7).
- CI is green on macOS and Windows and the cache total is under budget
  (Task 9).
- The manual checklist passes unchanged (Task 7).
- `CLAUDE.md`/`AGENTS.md`, `README.md`, `docs/upstream-issues.md` and
  `CHANGELOG.md` describe the forks (Task 8).
