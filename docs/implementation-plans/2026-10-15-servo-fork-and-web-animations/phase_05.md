# Servo Fork and Web Animations Implementation Plan

**Goal:** Script animations change rendered style: `animate()` moves
`getComputedStyle` values each frame, `fill: forwards` holds the end value,
`cancel()` reverts, and bsky.app gets past its splash screen.

**Architecture:** Each tick, after an animation's finished state is
updated, the manager asks it to push its effect into stylo: the
`KeyframeEffect` converts its keyframes into `ScriptKeyframe`s (one locked
declaration block per keyframe, locked by the document's author lock), the
animation computes the iteration progress with the Phase 2 timing
functions, writes it into the `DocumentAnimationSet` under the target's
`AnimationSetKey` (starting the stylo animation on first contact or after
the keyframes changed), and dirties the target node when the sampled
progress moved. When the manager drops an animation (idle, or finished
without a fill) it removes the stylo entry and dirties the target so the
style reverts. The layout gates in `servo_dangerous_style_element.rs` need
no change: `has_css_animations` already asks stylo, and Phase 4 made it
count script animations.

**Tech Stack:** Servo 0.7.0 script crate on `paladin/0.7.0`, stylo fork
from Phase 4 (`style::animation::{ScriptAnimation, ScriptKeyframe,
ScriptAnimationId}`), Paladin's `web_animations` page harness.

**Scope:** 6 phases from the design plan; this file is Phase 5 of 6.

**Codebase verified:** 2026-10-15; builds on the Phase 2 to 4 code.

---

## Facts from verification that shape this phase

- `Document::shared_style_locks()` returns `&SharedRwLocks { author:
  SharedRwLock, ua_or_user: SharedRwLock }`
  (`components/script/event_loop/script_thread.rs:250`); layout passes
  `.author` to stylo's `get_animation_declarations`, so keyframe blocks are
  wrapped with `shared_style_locks().author.wrap(block)` inside
  `servo_arc::Arc` (`servo_arc` is a dependency of the script crate).
- `Node::to_opaque()` (`components/script/dom/node/node.rs:576`) gives the
  `OpaqueNode` that layout uses for `AnimationSetKey::new_for_non_pseudo`.
- `Node::dirty(&self, no_gc: &NoGC, damage: NodeDamage)` requests a
  restyle; `NodeDamage::Style` is what the CSS animation path uses.
- `AnimationManager::sets()` returns a clone of the shared
  `DocumentAnimationSet` (an `Arc<RwLock<..>>`), the same object layout
  reads during reflow and `getComputedStyle` queries, so script writes are
  visible to the next restyle without any message.
- `servo_dangerous_style_element.rs` implements `has_css_animations` as
  `context.animations.has_active_animations(&key)`, and Phase 4 made
  `ElementAnimationSet::has_active_animation` true when script animations
  exist. `may_have_animations` is already `true`. No change there.
- `DomRoot::downcast::<U>(root)` and `Castable::downcast::<U>(&self)`
  exist for `AnimationEffect` → `KeyframeEffect`.
- `PropertyDeclarationBlock::declaration_importance_iter()` yields
  `(&PropertyDeclaration, Importance)`; `push(declaration, importance)`.
- `AnimationComposition` is `style::values::computed::AnimationComposition`
  (`values/computed/mod.rs:48`), the same type stylo's `animation.rs`
  aliases from the longhand.
- `DomRoot<KeyframeEffect>` has no `Deref` to `AnimationEffect`
  (`dom_struct` generates none); methods of the parent are reached with
  `effect.upcast::<AnimationEffect>()`.
- `Node::is_connected()` tells whether the target is in the document;
  `Node::dirty` is a no-op for a disconnected node, so sampling stops for
  detached targets rather than re-adding a stylo entry every tick.
- `updateTiming()` does not bump the `KeyframeEffect` generation: the
  only timing value stylo holds is `iteration_duration`, used for easing
  precision alone, so a stale value is harmless.
- `animation_manager.rs` refers to the DOM `Animation` as `WebAnimation`
  (Phase 2 aliased it because stylo's `Animation` is imported there).

Working in the fork as in Phase 2. Phase 4 removed `~/.cargo/config.toml`
when it pinned the stylo dist branch; recreate it with the single `servo`
path entry from `phase_02.md`, "Working in the fork". stylo comes from its
published dist branch, pinned in Phase 4: Servo's `components/script`
depends on `stylo = "0.22"`, which the Paladin lock resolves to the stylo
dist commit.

---

<!-- START_SUBCOMPONENT_A (tasks 1-3) -->
<!-- START_TASK_1 -->
### Task 1: `KeyframeEffect` converts itself into stylo's input

**Files:**
- Modify: `~/vibes/servo/components/script/dom/animations/keyframeeffect.rs`

**Step 1: Imports**

Add:

```rust
use servo_arc::Arc as ServoArc;
use style::animation::{ScriptAnimation, ScriptAnimationId, ScriptKeyframe};
use style::values::computed::AnimationComposition;
```

Remove the `#[allow(dead_code)]` attribute (and its comment) from
`KeyframeEffect::target()`: this phase uses it.

**Step 2: A generation counter**

Add a field to `KeyframeEffect`:

```rust
    /// Bumped whenever the keyframes, composite or target change, so the
    /// animation re-pushes the effect into stylo.
    generation: Cell<u64>,
```

initialised with `generation: Cell::new(0)` in `new_inherited`. Add to
`impl KeyframeEffect`:

```rust
    /// Changes when the keyframes, composite or target change.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn bump_generation(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }
```

Call `self.bump_generation();` at the end of `SetTarget`, `SetComposite`,
and in `SetKeyframes` after the keyframes are stored.

**Step 3: The conversion**

Add to `impl KeyframeEffect`:

```rust
    /// This effect's keyframes in the form stylo samples: computed offsets,
    /// computed easing, per-keyframe composite, and one declaration block
    /// per keyframe locked by the document's author lock (the style
    /// traversal reads them with its author guard).
    pub(crate) fn to_script_animation(
        &self,
        id: ScriptAnimationId,
        document: &Document,
        iteration_duration_seconds: f64,
    ) -> ScriptAnimation {
        let keyframes = self.keyframes.borrow();
        let offsets = compute_missing_keyframe_offsets(
            &keyframes
                .iter()
                .map(|keyframe| keyframe.offset)
                .collect::<Vec<_>>(),
        );
        let lock = &document.shared_style_locks().author;
        let script_keyframes = keyframes
            .iter()
            .zip(offsets)
            .map(|(keyframe, offset)| {
                let mut block = PropertyDeclarationBlock::new();
                for declaration in &keyframe.declarations {
                    for (property, importance) in declaration.block.declaration_importance_iter() {
                        block.push(property.clone(), importance);
                    }
                }
                ScriptKeyframe {
                    offset,
                    easing: keyframe.easing.to_computed_value_without_context(),
                    composite: composite_or_auto_to_stylo(keyframe.composite),
                    declarations: ServoArc::new(lock.wrap(block)),
                }
            })
            .collect();
        ScriptAnimation::new(
            id,
            script_keyframes,
            composite_to_stylo(self.composite.get()),
            iteration_duration_seconds,
        )
    }
```

and two free functions near the other helpers:

```rust
fn composite_to_stylo(composite: CompositeOperation) -> AnimationComposition {
    match composite {
        CompositeOperation::Replace => AnimationComposition::Replace,
        CompositeOperation::Add => AnimationComposition::Add,
        CompositeOperation::Accumulate => AnimationComposition::Accumulate,
    }
}

fn composite_or_auto_to_stylo(composite: CompositeOperationOrAuto) -> Option<AnimationComposition> {
    match composite {
        CompositeOperationOrAuto::Auto => None,
        CompositeOperationOrAuto::Replace => Some(AnimationComposition::Replace),
        CompositeOperationOrAuto::Add => Some(AnimationComposition::Add),
        CompositeOperationOrAuto::Accumulate => Some(AnimationComposition::Accumulate),
    }
}
```

**Step 4: Compile**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo check -p servo-script 2>&1 | grep -E "^error" -A 6 | head -40
```

Expected: clean, or a `dead_code` warning for `to_script_animation` until
Task 2. If `style::animation::ScriptAnimation` is not found, the lock
still points at the old stylo dist commit: redo Phase 4 Task 3 Step 3.

**Step 5: Commit with Task 2.**
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: `Animation` pushes its progress into stylo each tick

**Files:**
- Modify: `~/vibes/servo/components/script/dom/animations/animation.rs`
- Modify: `~/vibes/servo/components/script/dom/document/animation_manager.rs`
- Modify: `~/vibes/servo/components/script/dom/document/document.rs`
  (`update_animations_and_send_events`)

**Step 1: `animation.rs` imports and fields**

Add imports:

```rust
use js::context::NoGC;
use style::animation::{AnimationSetKey, ScriptAnimationId};

use crate::dom::animations::keyframeeffect::KeyframeEffect;
use crate::dom::element::Element;
use crate::dom::node::{Node, NodeDamage};
```

Add fields after `sequence_number`:

```rust
    /// The element whose stylo animation set currently holds this
    /// animation's effect, if any.
    sampled_target: MutNullableDom<Element>,

    /// The `KeyframeEffect::generation()` the stylo entry was built from.
    sampled_generation: Cell<u64>,

    /// The progress last written into the stylo entry.
    sampled_progress: Cell<Option<f64>>,
```

initialised in `new_inherited` with `sampled_target: Default::default(),
sampled_generation: Cell::new(0), sampled_progress: Cell::new(None),`.

**Step 2: The two sampling methods**

Add to `impl Animation`:

```rust
    /// This animation's id in stylo's animation sets.
    fn script_animation_id(&self) -> ScriptAnimationId {
        ScriptAnimationId(self.sequence_number)
    }

    /// The effect target, when the effect is a `KeyframeEffect` with an
    /// element target.
    pub(crate) fn effect_target(&self) -> Option<DomRoot<Element>> {
        self.associated_effect
            .get()
            .and_then(|effect| DomRoot::downcast::<KeyframeEffect>(effect))
            .and_then(|effect| effect.target())
    }

    /// Writes the effect's current iteration progress into stylo's
    /// animation set for the target, starting the stylo animation when the
    /// set does not hold it yet or the keyframes changed, and dirties the
    /// target when the sampled value moved. Called by the manager after
    /// every tick while the animation is tracked.
    pub(crate) fn update_effect_sampling(&self, document: &Document, no_gc: &NoGC) {
        let effect = self
            .associated_effect
            .get()
            .and_then(|effect| DomRoot::downcast::<KeyframeEffect>(effect));
        let target = effect.as_ref().and_then(|effect| effect.target());
        let (Some(effect), Some(target)) = (effect, target) else {
            self.stop_sampling(document, no_gc);
            return;
        };
        // A detached target is never restyled; stylo would drop the entry
        // and we would re-add it every tick. Resume when it is back.
        if !target.upcast::<Node>().is_connected() {
            self.stop_sampling(document, no_gc);
            return;
        }

        // A new target or new keyframes: drop the old stylo entry first.
        let generation = effect.generation();
        if let Some(previous) = self.sampled_target.get() {
            if *previous != *target || self.sampled_generation.get() != generation {
                self.stop_sampling(document, no_gc);
            }
        }

        let timing = effect.upcast::<AnimationEffect>().timing();
        let progress = timing::compute(&timing, self.current_time(), self.playback_rate.get()).progress;
        let node = target.upcast::<Node>();
        let key = AnimationSetKey::new_for_non_pseudo(node.to_opaque());
        let id = self.script_animation_id();
        let sets = document.animation_manager().sets();
        let mut changed = self.sampled_progress.get() != progress;
        if !sets.set_script_animation_progress(&key, id, progress) {
            // Not in the set yet (or stylo dropped it, e.g. the node left
            // the document): push the keyframes; they resolve on restyle.
            sets.start_script_animation(
                &key,
                effect.to_script_animation(id, document, timing.iteration_duration / 1000.0),
            );
            sets.set_script_animation_progress(&key, id, progress);
            changed = true;
        }
        self.sampled_target.set(Some(&target));
        self.sampled_generation.set(generation);
        self.sampled_progress.set(progress);
        if changed {
            node.dirty(no_gc, NodeDamage::Style);
        }
    }

    /// Removes this animation from stylo's set for its last target and
    /// restyles the target so its style reverts.
    pub(crate) fn stop_sampling(&self, document: &Document, no_gc: &NoGC) {
        let Some(target) = self.sampled_target.get() else {
            return;
        };
        self.sampled_target.set(None);
        self.sampled_progress.set(None);
        let node = target.upcast::<Node>();
        let key = AnimationSetKey::new_for_non_pseudo(node.to_opaque());
        document
            .animation_manager()
            .sets()
            .cancel_script_animation(&key, self.script_animation_id());
        node.dirty(no_gc, NodeDamage::Style);
    }
```

**Step 3: The manager drives it**

In `animation_manager.rs` change `update_script_animations` to take the
document instead of the window and call the two methods:

```rust
    pub(crate) fn update_script_animations(
        &self,
        cx: &mut JSContext,
        document: &Document,
        timeline_time: Option<f64>,
    ) {
        let animations: Vec<DomRoot<WebAnimation>> = self
            .script_animations
            .borrow()
            .iter()
            .map(|animation| DomRoot::from_ref(&**animation))
            .collect();
        let mut done = Vec::new();
        for animation in &animations {
            if animation.update_for_timeline_tick(cx, timeline_time) {
                animation.update_effect_sampling(document, cx.no_gc());
            } else {
                animation.stop_sampling(document, cx.no_gc());
                done.push(DomRoot::clone(animation));
            }
        }
        self.script_animations
            .borrow_mut()
            .retain(|animation| !done.iter().any(|finished| **finished == **animation));

        let running_css_animations = self
            .sets
            .sets
            .read()
            .values()
            .any(|set| set.needs_animation_ticks());
        self.update_running_animations_presence(
            document.window(),
            running_css_animations || self.has_running_script_animations(),
        );
    }
```

Add `use crate::dom::document::Document;` to its imports. In
`document.rs` change the call to
`self.animation_manager.update_script_animations(cx, self, timeline_time);`.

**Step 4: Compile and commit**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo check -p servo-script 2>&1 | grep -E "^error" -A 6 | head -40
cargo fmt -p servo-script
git add components/script/dom/animations/keyframeeffect.rs components/script/dom/animations/animation.rs components/script/dom/document/animation_manager.rs components/script/dom/document/document.rs
git commit -m "script: sample script animations through stylo

Each frame an Animation pushes its effect's iteration progress into the
document's DocumentAnimationSet under the target's AnimationSetKey, adding
the keyframes on first contact, and dirties the target so the restyle
applies the sampled values at the Animations cascade origin. Dropping the
animation (idle, or finished without a fill) removes the entry and restyles
the target."
```

Expected: a clean check and commit.
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: Visual pages, Bluesky, publish and pin

**Files:**
- Create: `crates/paladin-engine/tests/pages/opacity-fill-forwards.html`
- Create: `crates/paladin-engine/tests/pages/opacity-mid-animation.html`
- Create: `crates/paladin-engine/tests/pages/cancel-reverts.html`
- Create: `crates/paladin-engine/tests/pages/splash.html`
- Modify: `crates/paladin-engine/tests/web_animations.rs` (`PAGES`)
- Modify: `Cargo.lock`, `CHANGELOG.md`, `docs/upstream-issues.md`

**Step 1: The pages**

`crates/paladin-engine/tests/pages/opacity-fill-forwards.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>opacity-fill-forwards</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('did not settle within 5s'), 5000);
const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
const opacity = () => parseFloat(getComputedStyle(document.getElementById('box')).opacity);

(async () => {
  const box = document.getElementById('box');
  if (opacity() !== 1) return fail('initial opacity is ' + opacity());
  const anim = box.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 100, fill: 'forwards' });
  await anim.finished;
  await frame();
  if (opacity() !== 0) return fail('opacity after finish with fill forwards is ' + opacity());
  // The value holds: no further ticks are needed.
  await new Promise((resolve) => setTimeout(resolve, 100));
  if (opacity() !== 0) return fail('opacity did not hold: ' + opacity());
  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

`crates/paladin-engine/tests/pages/opacity-mid-animation.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>opacity-mid-animation</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('did not settle within 5s'), 5000);
const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
const opacity = () => parseFloat(getComputedStyle(document.getElementById('box')).opacity);

(async () => {
  const box = document.getElementById('box');
  // Paused at a known time, the sampled value is deterministic.
  const anim = box.animate([{ opacity: 1 }, { opacity: 0 }], 2000);
  anim.pause();
  anim.currentTime = 500;
  await anim.ready;
  await frame();
  if (Math.abs(opacity() - 0.75) > 0.01) return fail('opacity at 500ms of 2000ms is ' + opacity());
  // A keyframe easing applies between its keyframe and the next.
  anim.effect.setKeyframes([{ opacity: 1, easing: 'steps(1, jump-end)' }, { opacity: 0 }]);
  await frame();
  await frame();
  if (opacity() !== 1) return fail('opacity with step easing is ' + opacity());
  // Three keyframes with a middle one missing the property.
  anim.effect.setKeyframes([{ opacity: 0 }, { width: '60px' }, { opacity: 1 }]);
  await frame();
  await frame();
  if (Math.abs(opacity() - 0.25) > 0.01) return fail('opacity across a gap is ' + opacity());
  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

`crates/paladin-engine/tests/pages/cancel-reverts.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>cancel-reverts</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('did not settle within 5s'), 5000);
const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
const opacity = () => parseFloat(getComputedStyle(document.getElementById('box')).opacity);

(async () => {
  const box = document.getElementById('box');
  const anim = box.animate([{ opacity: 0.2 }, { opacity: 0.2 }], { duration: 10000 });
  await anim.ready;
  await frame();
  if (opacity() !== 0.2) return fail('opacity while running is ' + opacity());
  anim.cancel();
  await frame();
  await frame();
  if (opacity() !== 1) return fail('opacity after cancel is ' + opacity());
  // A finished animation without a fill also reverts.
  const short = box.animate([{ opacity: 0.4 }, { opacity: 0.4 }], 50);
  await short.finished;
  await frame();
  await frame();
  if (opacity() !== 1) return fail('opacity after an unfilled finish is ' + opacity());
  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

`crates/paladin-engine/tests/pages/splash.html` (the shape of bsky.app's
splash: opacity and transform with a cubic-bezier, `fill: forwards`,
`onfinish` removes the splash and reveals the app):

```html
<!doctype html>
<meta charset="utf-8">
<title>splash</title>
<style>
  #splash { position: fixed; inset: 0; background: #1185fe; display: flex;
            align-items: center; justify-content: center; color: white; }
  #feed { display: none; }
</style>
<div id="splash">splash</div>
<div id="feed">feed</div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('splash did not finish within 5s'), 5000);
const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));

(async () => {
  const splash = document.getElementById('splash');
  const feed = document.getElementById('feed');
  const anim = splash.animate(
    [{ opacity: 1, transform: 'scale(1)' }, { opacity: 0, transform: 'scale(1.1)' }],
    { duration: 300, easing: 'cubic-bezier(0.4, 0, 0.2, 1)', fill: 'forwards' });
  let sawProgress = false;
  anim.onfinish = () => {
    const style = getComputedStyle(splash);
    if (parseFloat(style.opacity) !== 0) return fail('splash opacity at finish is ' + style.opacity);
    if (!style.transform.includes('1.1')) return fail('splash transform at finish is ' + style.transform);
    splash.remove();
    feed.style.display = 'block';
    if (getComputedStyle(feed).display !== 'block') return fail('feed is not shown');
    if (!sawProgress) return fail('opacity never changed while animating');
    verdict('PASS');
  };
  for (let i = 0; i < 30; i++) {
    await frame();
    const value = parseFloat(getComputedStyle(splash).opacity);
    if (value > 0 && value < 1) { sawProgress = true; break; }
  }
})().catch((e) => fail(e.message));
</script>
```

**Step 2: Register the pages**

Append to `PAGES` in `crates/paladin-engine/tests/web_animations.rs`:

```rust
    "opacity-fill-forwards.html",
    "opacity-mid-animation.html",
    "cancel-reverts.html",
    "splash.html",
```

**Step 3: Run against the path-patched fork**

```bash
cd /Users/thomas/vibes/paladin
cargo test -p paladin-engine --test web_animations 2>&1 | tail -15
```

Expected: `ok`. If `getComputedStyle` reports stale values one frame late,
the pages already wait two frames where it matters; if a page fails
because the value is right but off by rounding (e.g. `0.7499`), widen the
tolerance in that page to `0.02`, not more.

**Step 4: Manual acceptance on bsky.app**

```bash
cargo run -- https://bsky.app
```

Expected: the blue splash fades out and the Bluesky landing page or feed
renders and is scrollable; no `animate is not a function` errors in the
log. Also open `https://en.wikipedia.org/wiki/Main_Page` and a CSS
transition page to confirm nothing regressed, then run the 11-item manual
checklist in `README.md` (engine glue changed).

**Step 5: Publish and pin**

```bash
cd ~/vibes/servo && git push origin paladin/0.7.0 && etc/paladin/publish-dist.sh dist/paladin-0.7.0 | tee /tmp/servo-dist-sha
rm ~/.cargo/config.toml
cd /Users/thomas/vibes/paladin
git checkout Cargo.lock
cargo update -p servo --precise $(cat /tmp/servo-dist-sha) 2>&1 | tail -2
cargo test --workspace 2>&1 | grep -E '^test result|FAILED|panicked'
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo fmt --all --check && echo fmt-ok
```

Expected: all `ok`, `fmt-ok`.

**Step 6: Docs and commit**

`CHANGELOG.md`: change the Web Animations bullet's "Animations do not
change rendered style yet." to "Animations are sampled by the style system
each frame like CSS animations, so bsky.app renders past its splash
screen."

`docs/upstream-issues.md`, S13 fork entry: add `"script: sample script
animations through stylo"` and drop "Timing model only; sampling follows in
later patches."

```bash
git add Cargo.lock CHANGELOG.md docs/upstream-issues.md crates/paladin-engine/tests
git commit -m "feat(engine): script animations change rendered style"
git push origin vnext
```
<!-- END_TASK_3 -->
<!-- END_SUBCOMPONENT_A -->

## Phase 5 done when

- The twelve pages pass in Paladin's `web_animations` test against the
  published Servo and stylo dist commits.
- bsky.app clears its splash screen and renders in a manual check; the
  manual checklist passes.
- `cargo test --workspace`, clippy and fmt are clean.
