# Servo Fork and Web Animations Design

## Summary

Paladin renders pages with Servo, and Servo 0.7.0 lacks the scripting half
of the Web Animations API. Sites that call `element.animate()` without
feature detection stall; bsky.app is the motivating case, stuck on its
splash screen. This plan first gives Paladin a way to carry its own patches
to Servo and to Servo's styling engine, stylo, and then uses that mechanism
to implement the missing animation behaviour.

For the patch mechanism, Paladin maintains public forks of Servo and stylo,
each with a development branch holding the patches on top of a release tag
and a stripped-down "dist" branch that cargo can fetch cheaply. Paladin's
root `Cargo.toml` redirects every Servo and stylo crate to the dist
branches, so the existing exact version pins keep working, and each monthly
Servo release is handled by rebasing the patches onto the new tag and
republishing. For the animation work the split follows the spec: the timing
model (play, pause, cancel, finish, reverse, promises, events,
`getAnimations`) goes in Servo's script crate, and sampling goes in stylo,
where script supplies keyframes, timing, and a per-tick local time and
stylo folds the result into the normal cascade next to CSS animations. Six
phases take it from a zero-patch fork bootstrap to visual acceptance.
Acceptance is checked by local test pages that report through the page
title in Paladin's headless engine test, plus a manual check of bsky.app.

## Definition of Done

1. **Fork infrastructure.** A public `thomasmost/servo` fork with a patch
   branch based at tag `v0.7.0` and a pruned branch that cargo fetches
   cheaply. Paladin's root `Cargo.toml` patches every Servo crate to it, CI
   stays green on macOS and Windows within the cache budget, and `CLAUDE.md`
   documents the monthly rebase-and-bump procedure. The same model is ready
   for a stylo fork if the animation design needs one.

2. **Web Animations.** The spec playback core implemented in the fork:
   `play`, `pause`, `cancel`, `finish`, `reverse`, `currentTime`,
   `startTime`, `playbackRate`, `playState`, `ready`, `finished`,
   `onfinish`, `oncancel`, `id`; `KeyframeEffect` timing, target, composite,
   and property-indexed keyframes; `Element` and `Document` `getAnimations`
   for script animations. Script animations are sampled by the restyle each
   tick like CSS animations. Paladin enables the pref. Acceptance: a set of
   local HTML test pages run through Paladin's headless engine test, and
   bsky.app gets past its splash screen and renders the feed.

3. **Roadmap.** A roadmap section naming the next two design plans: isolated
   worlds in Servo, then the WebExtension runtime in Paladin with Proton Pass
   as acceptance (unpacked install, login, autofill, save).

**Out of scope:** `CSSAnimation` and `CSSTransition` objects, `commitStyles`,
`persist`, `replaceState`, scroll timelines, pseudo-element targets, upstream
PRs, and anything in plans 2 and 3.

## Glossary

- **Servo**: The Rust browser engine Paladin uses to render pages. It is the
  code being forked and patched here.
- **stylo**: Servo's CSS styling engine, shared with Firefox. It computes
  styles and samples CSS animations, so it needs its own fork for this work.
- **WPT**: Web Platform Tests, the cross-browser conformance suite. Servo
  keeps it in-tree, which is much of why the repository is about 2 GB.
- **Web Animations API**: The browser API for creating and controlling
  animations from script (`element.animate()`, `Animation`,
  `KeyframeEffect`, `getAnimations`).
- **Timing model**: The part of the Web Animations spec that defines how
  time, playback state, playback rate, and finishing work, independent of
  how styles are applied.
- **Sampling**: Computing an animation's property values at a given moment
  and feeding them into style resolution. Here stylo does it during restyle.
- **Cascade**: The CSS process of resolving competing declarations into one
  computed style per element. Script animations enter it at the Animations
  origin.
- **Restyle**: Recomputing an element's styles after something changes.
  Each tick with a running animation restyles its target.
- **Local time**: An effect's time within its own run, derived from the
  animation's current time. Script pushes it into stylo each tick.
- **Fill mode**: A timing option such as `forwards` that controls whether an
  effect still applies before it starts or after it ends.
- **Pref**: A Servo preference flag. Web Animations sits behind
  `dom_web_animations_enabled`, which Paladin turns on.
- **WebIDL**: The interface description language for web APIs. Servo
  generates its JavaScript bindings from `.webidl` files at build time.
- **Dev branch**: A fork branch (`paladin/<version>`) holding the patch
  commits, one logical change each, based on an upstream release tag.
- **Dist branch**: A generated orphan branch (`dist/paladin-<version>`)
  holding only the files cargo needs, so a git fetch avoids Servo's history
  and tree.
- **Orphan branch**: A git branch with no parent history. The dist script
  rewrites it from scratch on each publish.
- **`[patch.crates-io]`**: A Cargo manifest section that replaces a
  crates.io dependency with another source without changing the version
  requirements of the crates that depend on it.
- **Bump procedure**: The routine for moving to a new Servo release: rebase
  the patches onto the new tag, republish dist, repoint the patch block,
  update pins, rebuild, run the manual checklist.
- **Isolated worlds**: A browser mechanism, used by Chromium extensions,
  that runs scripts against the same page DOM with separate JavaScript
  globals so page scripts cannot see or tamper with them. A later plan.
- **SpiderMonkey compartment**: A memory and security boundary inside the
  SpiderMonkey JavaScript engine Servo uses. A sandbox global in its own
  compartment is one candidate route to isolated worlds.
- **Reflector**: Servo's single JavaScript wrapper object for a DOM object.
  Per-world wrappers would turn it into a per-world table.
- **WebExtension, Manifest V3**: The cross-browser extension format and its
  current manifest version, which uses a background service worker instead
  of a persistent page.
- **Content script**: Extension code injected into pages, normally run in an
  isolated world.
- **Proton Pass**: The password manager extension chosen as the acceptance
  target for the later WebExtension runtime plan.
- **Headless engine test**: Paladin's test that runs Servo without a window.
  The animation pages load through it and report via `document.title`.

## Architecture

### Fork infrastructure

Paladin builds against two public forks under `thomasmost`, both on the
same model:

| Fork | Base tag | Dev branch | Dist branch |
| ---- | -------- | ---------- | ----------- |
| `thomasmost/servo` | `v0.7.0` (commit `aac43a3f`) | `paladin/0.7.0` | `dist/paladin-0.7.0` |
| `thomasmost/stylo` | `v0.22.0` (commit `34d25f74`) | `paladin/0.22.0` | `dist/paladin-0.22.0` |

The crates.io releases are exact snapshots of those tags, with crate names
(`servo-script`, `stylo`, ...) living in the repo manifests, so a git source
satisfies the same `=0.7.0` and `=0.22.0` requirements as long as the forks
never touch `[workspace.package] version`.

**Dev branch.** Patches are one commit per logical change, in Servo's style
(MPL header, tidy-clean, no Paladin-specific names), so the monthly rebase
onto the next tag stays mechanical and a later upstream PR remains possible.
No upstream PRs are opened as part of this plan, and GitHub Actions stays
disabled on both forks; Paladin's CI is the gate.

**Dist branch.** Cargo fetches a git dependency's full history and checks
out its whole tree, and Servo's repository is about 2 GB with WPT in-tree.
Each fork therefore carries a script, `etc/paladin/publish-dist.sh`, that
rewrites an orphan dist branch from the dev branch head containing only
what cargo needs: the root `Cargo.toml` and `Cargo.lock`, `components/`,
`ports/servoshell`, `ffi/`, `tests/capi`, `tests/unit`, and `resources/`
for Servo (every workspace member must exist because cargo loads the whole
workspace), and the equivalent crate set for stylo. WPT, docs, `python/`,
`pyproject.toml` and `uv.lock` are dropped. The dist commit message records
the dev commit it was built from.

**Paladin wiring.** The root `Cargo.toml` gains a `[patch.crates-io]`
block: the 60 Servo crates in `Cargo.lock` (`servo` plus 59 `servo-*`,
including `servo-media-ohos`, which is locked on every host) and the 7
stylo crates at 0.22.0 (`stylo`, `stylo_atoms`, `stylo_derive`,
`stylo_dom`, `stylo_malloc_size_of`, `stylo_static_prefs`,
`stylo_traits`), each `{ git = "...", branch = "dist/..." }`.
`crates/paladin-engine/Cargo.toml` keeps `servo = "=0.7.0"`. `Cargo.lock`
pins the dist commit; `cargo update -p servo --precise <sha>` moves all
sixty at once. `.cargo/config.toml` adds `UV_NO_SYNC = "1"` so that
`servo-script-bindings`' build script, which prefers `uv run python`, never
installs Servo's Python environment into the cargo checkout.

**Local loop.** Development happens in `~/vibes/servo` and `~/vibes/stylo`
checkouts of the dev branches. A `[patch.crates-io]` entry in
`~/.cargo/config.toml` points the crate under edit at its path there
(config patches override manifest patches for the same package), so a
change rebuilds one crate rather than sixty. Publishing is: push the dev
branch, run the dist script, update Paladin's lock sha.

**Bump procedure** (replaces the Servo bump steps in `CLAUDE.md`): branch
`paladin/<new>` from the new tag, rebase the patch commits onto it, drop
any upstream has merged, re-publish dist, repoint the patch block, bump
the engine pin, run `cargo update` and the pre-release pins, rebuild, run
the manual checklist. `docs/upstream-issues.md` gains a "Carried in the
fork" section listing each patch against the upstream issue it addresses.

### Web Animations

The spec's own seam splits the work: the **timing model** lives in Servo's
script crate, **sampling** lives in stylo.

**Script crate** (`components/script/dom/animations/`):

- `Animation` (`animation.rs`) gains the spec state: start time, hold
  time, playback rate, pending playback rate, pending play or pause task,
  `ready` and `finished` promises, replace state, id. It implements play
  with auto-rewind, pause, cancel, finish, reverse, update playback rate,
  the current time getter and setter, play state, and the update-finished-
  state procedure. It is registered with the document's `AnimationManager`
  while it has an effect with a target.
- `KeyframeEffect` (`keyframeeffect.rs`) applies the options it is given
  (duration, delay, end delay, easing, fill, iterations, iteration start,
  direction, composite), computes missing offsets, parses keyframe easing,
  and accepts property-indexed keyframes (`BasePropertyIndexedKeyframe`
  members restored in the IDL). `target` and `composite` are exposed.
- `AnimationEffect` (`animationeffect.rs`) gets the timing calculations
  (active duration, end time, local time, transformed progress, current
  iteration) so `getComputedTiming` returns real values; these are pure
  functions with unit tests.
- `AnimationPlaybackEvent` (new `animationplaybackevent.rs` and
  `AnimationPlaybackEvent.webidl`): `currentTime` and `timelineTime`.
  `onfinish`, `oncancel`, and `onremove` are event handler attributes on
  `Animation`.
- `Element::Animate` (`dom/element/element.rs`) finishes the procedure:
  sets `id` and plays. `Element.getAnimations()` and
  `Document.getAnimations()` return the manager's script animations for
  the element (and its subtree when asked) or the document, in spec order.
- `AnimationManager` (`dom/document/animation_manager.rs`) owns the list of
  live script animations, roots their target nodes, counts them toward
  `running_animation_count`, and on every
  `Document::update_animations_and_send_events` tick asks each one for its
  effect's local time, writes that into stylo's `DocumentAnimationSet`,
  and marks the target dirty.

**IDL** (`components/script_bindings/webidls/`): `Animation.webidl`,
`KeyframeEffect.webidl`, and `Animatable.webidl` have their commented-out
members restored; `AnimationPlayState` and `AnimationReplaceState` enums
are added; `Document.webidl` gets `getAnimations`. `Bindings.conf` lists
the members that need `cx`. Everything stays behind
`dom_web_animations_enabled`; Paladin turns it on in
`crates/paladin-engine/src/engine.rs` where it builds `Preferences`.

**Stylo** (`style/animation.rs`): `ElementAnimationSet` gains
`script_animations: Vec<ScriptAnimation>` beside `animations` and
`transitions`. The contract with Servo is:

```rust
// style/animation.rs (new, public)
pub struct ScriptAnimationId(pub u64);

pub struct ScriptKeyframe {
    pub offset: f64,                      // computed, 0..=1
    pub easing: TimingFunction,
    pub composite: CompositeOperation,
    pub declarations: Arc<Locked<PropertyDeclarationBlock>>,
}

pub struct ScriptAnimationTiming {
    pub iteration_duration: f64,          // ms, resolved
    pub iteration_start: f64,
    pub iteration_count: f64,             // may be infinite
    pub direction: AnimationDirection,
    pub fill_mode: AnimationFillMode,
    pub easing: TimingFunction,
}

pub struct ScriptAnimation {
    pub id: ScriptAnimationId,
    pub keyframes: Vec<ScriptKeyframe>,
    pub timing: ScriptAnimationTiming,
    /// Set by script each tick; `None` means the effect is not in effect
    /// and the fill mode decides whether anything applies.
    pub local_time: Option<f64>,
    // computed steps are materialised on the next restyle, like
    // `maybe_start_animations`, because resolving keyframe declarations
    // needs the element's cascade.
}

impl ElementAnimationSet {
    pub fn start_script_animation(&mut self, animation: ScriptAnimation);
    pub fn set_script_animation_local_time(&mut self, id: ScriptAnimationId, local_time: Option<f64>);
    pub fn cancel_script_animation(&mut self, id: ScriptAnimationId);
}
```

Script animations are exempt from `is_cancelled_in_new_style` and from
the `animation-name` diff in `update_animations_for_new_style`; they
materialise their computed keyframes in `process_animations_for_style`
(`style/matching.rs`) the first time the element is restyled after they
are added, and are sampled through the existing
`get_value_map_for_active_animations` path into the cascade at the
Animations origin, after CSS animations and before transitions. A script
animation is removed only when Servo cancels it.

**Data flow per tick.** `update_animations_and_send_events` advances the
`DocumentTimeline`; each registered `Animation` recomputes its current
time, runs update-finished-state, resolves pending tasks, and reports its
effect's local time; the manager writes local times into the
`DocumentAnimationSet` (the same `Arc<RwLock>` the reflow request already
carries), dirties the target nodes, and queues finish and cancel events on
its pending-events list; the reflow samples. `fill: forwards` leaves the
entry in place at its end value after finish; cancel removes it.

## Existing Patterns

- **Exact version pins that move together.** The workspace already pins
  servo, winit, egui, glow and surfman to exact versions
  (`Cargo.toml`, `CLAUDE.md`). The patch block extends that: the fork is
  another exactly-pinned source, moved by one lock update.
- **Build-time settings in `.cargo/config.toml`.** `SERVO_STYLE_THREAD_STACK_SIZE_KB`
  lives there today; `UV_NO_SYNC` joins it with the same comment style.
- **Upstream tracking in `docs/upstream-issues.md`.** S13 already
  describes the animation gap; the new "Carried in the fork" section is
  where resolved-by-patch entries go, and the file's instruction to
  re-check `S` entries on each bump now also covers rebasing.
- **One engine per test binary.** `crates/paladin-engine/tests/headless.rs`
  holds one test because Servo's options are process-global. The animation
  pages get their own binary, `tests/web_animations.rs`, with one test
  function that loads each page in turn.
- **Pages report through the title.** The headless test already waits on
  `EngineEvent::TitleChanged`; test pages set `document.title` to `PASS`
  or `FAIL: <reason>` so no new engine API is needed to read results.
- **Servo's own animation plumbing.** Script animations reuse
  `AnimationManager` for rooting, dirtying, liveness and event dispatch,
  and stylo's `ElementAnimationSet` for sampling, rather than adding a
  parallel pipeline. Materialising computed keyframes in the restyle
  mirrors `maybe_start_animations`; this is also the direction the
  upstream contributor's stub (`start_animation_from_script`) took.
- **Pref-gated features.** The IDL stays behind `dom_web_animations_enabled`
  as upstream has it; Paladin opts in from `Engine`, matching how it
  already passes `Preferences`.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Fork bootstrap
**Goal:** Paladin builds from the forks with zero patches, locally and in
CI, and the maintenance procedure is written down.

**Components:**
- `thomasmost/servo`: branch `paladin/0.7.0` at `v0.7.0`;
  `etc/paladin/publish-dist.sh`; branch `dist/paladin-0.7.0`.
- `thomasmost/stylo`: branch `paladin/0.22.0` at `v0.22.0`; the same
  script; branch `dist/paladin-0.22.0`.
- `Cargo.toml` (root): `[patch.crates-io]` with the 60 Servo and 7 stylo
  entries.
- `.cargo/config.toml`: `UV_NO_SYNC = "1"`.
- `Cargo.lock`: the Servo and stylo entries move to the git sources.
- `.github/workflows/ci.yml`: whatever the cache budget needs after
  measuring a run with the git sources.
- `CLAUDE.md` and `AGENTS.md`: the bump procedure rewritten for the forks,
  the local path-patch loop, freshness stamp.
- `docs/upstream-issues.md`: "Carried in the fork" section (empty).
- `README.md`: local setup mentions the forks.

**Dependencies:** none.

**Done when:** `cargo build --workspace`, `cargo test --workspace`, clippy
and fmt are clean against the dist branches; CI is green on both hosts and
the cache total stays under budget; the manual checklist passes unchanged.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Animation timing model
**Goal:** `Animation` behaves per spec in time without yet changing any
pixel.

**Components:**
- `components/script/dom/animations/animation.rs`: spec state and
  procedures; `ready` and `finished`; registration with the manager.
- `components/script/dom/animations/animationeffect.rs`: timing
  calculations behind `getComputedTiming`, with unit tests.
- `components/script/dom/animations/animationplaybackevent.rs` and
  `components/script_bindings/webidls/AnimationPlaybackEvent.webidl`.
- `components/script_bindings/webidls/Animation.webidl`: members restored,
  `AnimationPlayState` and `AnimationReplaceState`.
- `components/script_bindings/codegen/Bindings.conf`: `cx` entries.
- `components/script/dom/document/animation_manager.rs`: script animation
  list, per-tick update, liveness count, event queueing.
- `components/script/dom/element/element.rs`: `Animate` sets `id` and
  plays.
- `crates/paladin-engine/src/engine.rs`: pref enabled.
- `crates/paladin-engine/tests/web_animations.rs` and
  `crates/paladin-engine/tests/pages/`: the harness and its first pages
  (finish fires, `finished` resolves, cancel rejects, pause holds
  `currentTime`, `playbackRate` scales).

**Dependencies:** Phase 1.

**Done when:** the timing unit tests and the engine test pages pass;
`cargo test --workspace` is green.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: KeyframeEffect completeness
**Goal:** every input `animate()` accepts is parsed and retained.

**Components:**
- `components/script/dom/animations/keyframeeffect.rs`: options applied,
  computed offsets, keyframe easing parsed, property-indexed keyframes,
  `target` and `composite`.
- `components/script_bindings/webidls/KeyframeEffect.webidl`: members and
  `BasePropertyIndexedKeyframe` restored.
- Test pages: `getKeyframes` round-trips both keyframe forms;
  `getComputedTiming` reflects the options.

**Dependencies:** Phase 2.

**Done when:** the new pages pass alongside the Phase 2 pages.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: Stylo script animations
**Goal:** stylo can sample an animation whose keyframes and timing come
from script and whose local time is pushed in.

**Components:**
- `style/animation.rs`: `ScriptAnimation`, `ScriptKeyframe`,
  `ScriptAnimationTiming`, `ScriptAnimationId`, the three
  `ElementAnimationSet` methods, materialisation and sampling, exemption
  from `animation-name` cancellation; unit tests sampling at given local
  times and fill modes.
- `style/matching.rs`: materialisation hook in
  `process_animations_for_style`; `needs_animations_update` considers
  script animations.
- `style/stylesheets/keyframes_rule.rs` or a sibling: whatever helper
  builds computed keyframes from `ScriptKeyframe`s.

**Dependencies:** Phase 1 (the stylo fork exists). Independent of Phases
2 and 3.

**Done when:** stylo unit tests pass; Paladin still builds and behaves as
before with the new stylo patch and no caller.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: Wiring and visual acceptance
**Goal:** script animations change rendered style, and bsky.app loads.

**Components:**
- `components/script/dom/document/animation_manager.rs`: writes
  `ScriptAnimation`s and local times into the `DocumentAnimationSet`,
  roots and dirties targets, removes on cancel.
- `components/script/dom/animations/keyframeeffect.rs`: conversion of its
  keyframes and timing into the stylo input types.
- `components/script/layout_dom/servo_dangerous_style_element.rs`: the
  `may_have_animations` and `has_css_animations` gates account for script
  animations.
- Test pages: opacity reaches its end value and holds with
  `fill: forwards`; cancel reverts; a Bluesky-shaped splash (opacity and
  transform, cubic-bezier easing, `onfinish`).

**Dependencies:** Phases 3 and 4.

**Done when:** the visual pages pass by reading `getComputedStyle` after
finish; bsky.app clears its splash screen and renders the feed in a manual
check.
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: getAnimations and polish
**Goal:** the remaining surface and the documentation.

**Components:**
- `components/script/dom/element/element.rs` and
  `components/script/dom/document/document.rs`: `getAnimations`.
- `components/script_bindings/webidls/Animatable.webidl` and
  `Document.webidl`: `getAnimations` declared.
- `README.md`: limitation removed, bsky.app added to the manual checklist.
- `docs/upstream-issues.md`: S13 moved to "Carried in the fork".
- `CHANGELOG.md`: Unreleased entry.
- Test pages: `getAnimations` ordering and subtree option.

**Dependencies:** Phase 5.

**Done when:** all pages pass, docs are current, the Definition of Done
holds.
<!-- END_PHASE_6 -->

## Additional Considerations

**Rebuild cost.** Cargo fingerprints git packages by commit, so every dist
commit rebuilds all sixty Servo crates (not mozjs, webrender or wgpu).
That is acceptable for publish steps; the local path-patch loop exists so
day-to-day edits do not pay it.

**Servo 0.8.0 lands mid-plan.** Upstream releases monthly and nothing
functional has landed in the animation code since 0.7.0, so the first
rebase is expected to be clean. It should be done as soon as 0.8.0 and the
matching stylo are on crates.io, between phases, as a rehearsal of the
procedure.

**Sandboxed iframes.** Upstream's user-script path panics in sandboxed
iframes (servo/servo#48774); the animation work does not touch it, but the
isolated-worlds plan will.

**Promise and task ordering.** Pending play and pause tasks resolve on the
first tick where the timeline has a value, which is how `startTime`
becomes resolved; `finished` is replaced on replay. Both need the
microtask checkpoint that `update_animations_and_send_events` already runs
after updating animations.

**Liveness.** Script animations count toward `running_animation_count`
while running or pending, so the tick loop keeps going for a page whose
only animation came from `animate()` and goes quiet when it finishes.

**Roadmap after this plan.** Two further design plans follow, in order:

1. *Isolated worlds in Servo.* Choose between per-world DOM wrappers
   (Chromium's model: the single `Reflector` becomes a per-world table,
   invasive across `script_bindings` and codegen) and a sandbox global in
   its own compartment with filtering wrappers (the direction the Servo
   maintainer is researching on servo/servo#13263; fits the current
   one-wrapper code but needs prototype resolution in the content world).
   Deliverables: an embedder API to run a script in a named world of a
   given frame, a per-world `customElements` registry, `postMessage`
   between worlds, a host message port, and user scripts moved into an
   isolated world.
2. *WebExtension runtime in Paladin.* Unpacked extension loading, the
   Manifest V3 background service worker, the `chrome.*` subset Proton
   Pass uses (`scripting` with `world`, `runtime` ports and messages,
   `tabs`, `alarms`, `storage.local` and `.session`, `webNavigation`,
   `webRequest` including blocking `onAuthRequired`, `offscreen`,
   `windows`, `action`, `commands`, `permissions`), popup and options
   pages, `web_accessible_resources`. Acceptance: Proton Pass installs
   unpacked, signs in, autofills, and saves a login. Known risk: Servo's
   service worker support is partial, so the background worker may need
   its own fork patch.
