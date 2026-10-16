# Servo Fork and Web Animations Implementation Plan

**Goal:** The remaining API surface (`Element.getAnimations()`,
`Document.getAnimations()`) and the documentation: README limitation
removed and bsky.app on the manual checklist, S13 moved to "Carried in the
fork", the changelog entry, and the roadmap naming the next two design
plans.

**Architecture:** The manager already holds every script animation that
left the idle state (Phase 2) and each `Animation` knows whether it is
relevant (Phase 2) and its effect target (Phase 5). `getAnimations()` is a
filter over that list, sorted by the creation sequence number, which is
the composite order for script animations (there are no `CSSAnimation`
objects to interleave).

**Tech Stack:** Servo 0.7.0 script crate on `paladin/0.7.0`; Paladin docs.

**Scope:** 6 phases from the design plan; this file is Phase 6 of 6.

**Codebase verified:** 2026-10-15; `Animatable.webidl` has
`getAnimations` commented out, `Document.webidl:228-231` holds the Web
Animations partial with only `timeline`, `ROADMAP.md` has Phases 1 to 3,
`README.md:176-178` is the animations limitation and `:72-128` the
11-item manual checklist, `docs/upstream-issues.md:208-233` is S13 with a
reference from the dependency tree at `:38-39`.

---

## Facts from verification that shape this phase

- The spec puts `getAnimations()` on the `DocumentOrShadowRoot` mixin.
  Servo implements that mixin's members separately on `Document` and
  `ShadowRoot`; shadow roots are out of scope here, so the method is
  declared in `Document.webidl`'s Web Animations partial, where `timeline`
  already is. (Deviation from the IDL text, not from behaviour on
  `document`.)
- Dictionary arguments arrive as `&GetAnimationsOptions { subtree: bool,
  pseudoElement: Option<DOMString> }` (generated in
  `AnimatableBinding.rs`). Pseudo-element targets are unsupported, so a
  non-null `pseudoElement` yields an empty list rather than a
  `SyntaxError` round trip through pseudo-element parsing.
- `Node::is_inclusive_ancestor_of(&self, child: &Node)` (`node.rs:1024`)
  gives the subtree test; `Document` upcasts to `Node`.
- No `cx` is needed: the method only reads DOM state.
- `animation_manager.rs` names the DOM `Animation` `WebAnimation` (Phase
  2 alias).
- The README, CHANGELOG and upstream-issues line numbers below are from
  the start of Phase 1; Phase 1's insertions shift them by a few lines.
  The steps name the sections, so find by heading.

Working in the fork as in Phase 2 (recreate `~/.cargo/config.toml` with
the single `servo` path entry; Phase 5 removed it).

---

<!-- START_SUBCOMPONENT_A (tasks 1-2) -->
<!-- START_TASK_1 -->
### Task 1: `getAnimations()` on `Element` and `Document`

**Files:**
- Modify: `~/vibes/servo/components/script_bindings/webidls/Animatable.webidl`
- Modify: `~/vibes/servo/components/script_bindings/webidls/Document.webidl:228-231`
- Modify: `~/vibes/servo/components/script/dom/document/animation_manager.rs`
- Modify: `~/vibes/servo/components/script/dom/element/element.rs`
- Modify: `~/vibes/servo/components/script/dom/document/document.rs`

**Step 1: IDL**

`Animatable.webidl`: replace the commented line with

```webidl
    [Pref="dom_web_animations_enabled"]
    sequence<Animation> getAnimations(optional GetAnimationsOptions options = {});
```

`Document.webidl`: extend the Web Animations partial to

```webidl
// https://drafts.csswg.org/web-animations-1/#extensions-to-the-document-interface
partial interface Document {
    readonly attribute DocumentTimeline timeline;
    // https://drafts.csswg.org/web-animations-1/#dom-documentorshadowroot-getanimations
    [Pref="dom_web_animations_enabled"]
    sequence<Animation> getAnimations();
};
```

**Step 2: The manager's query**

Add to `impl AnimationManager` in `animation_manager.rs` (after
`has_running_script_animations`), plus `use crate::dom::element::Element;`:

```rust
    /// The relevant script animations whose effect target `include`
    /// accepts, in composite order. Script animations have no owning
    /// element, so their composite order is their position in the global
    /// animation list, which the sequence number records.
    ///
    /// <https://drafts.csswg.org/web-animations-1/#relevant>
    pub(crate) fn relevant_script_animations(
        &self,
        include: &dyn Fn(&Element) -> bool,
    ) -> Vec<DomRoot<WebAnimation>> {
        let mut animations: Vec<DomRoot<WebAnimation>> = self
            .script_animations
            .borrow()
            .iter()
            .map(|animation| DomRoot::from_ref(&**animation))
            .filter(|animation| {
                animation.is_relevant() &&
                    animation
                        .effect_target()
                        .is_some_and(|target| include(&target))
            })
            .collect();
        animations.sort_by_key(|animation| animation.sequence_number());
        animations
    }
```

Remove the `#[allow(dead_code)]` attribute (and its comment) from
`Animation::sequence_number()` in `animation.rs`: it is used now.

**Step 3: `Element`**

In `element.rs`, next to `Animate`, add (import
`crate::dom::bindings::codegen::Bindings::AnimatableBinding::GetAnimationsOptions`):

```rust
    /// <https://drafts.csswg.org/web-animations-1/#dom-animatable-getanimations>
    fn GetAnimations(&self, options: &GetAnimationsOptions) -> Vec<DomRoot<Animation>> {
        // (`Animation` here is the DOM type already imported by element.rs.)
        // Pseudo-element targets are not supported, so no animation can
        // target one.
        if options.pseudoElement.is_some() {
            return Vec::new();
        }
        let node = self.upcast::<Node>();
        self.owner_document()
            .animation_manager()
            .relevant_script_animations(&|target| {
                if options.subtree {
                    node.is_inclusive_ancestor_of(target.upcast::<Node>())
                } else {
                    *target == *self
                }
            })
    }
```

**Step 4: `Document`**

In `document.rs`, in `impl DocumentMethods` next to `Timeline` (line
7188), add (import `crate::dom::animation::Animation` if `Animation` is
not yet imported there):

```rust
    /// <https://drafts.csswg.org/web-animations-1/#dom-documentorshadowroot-getanimations>
    fn GetAnimations(&self) -> Vec<DomRoot<Animation>> {
        let root = self.upcast::<Node>();
        self.animation_manager
            .relevant_script_animations(&|target| {
                root.is_inclusive_ancestor_of(target.upcast::<Node>())
            })
    }
```

**Step 5: Compile and commit**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo check -p servo-script 2>&1 | grep -E "^error" -A 6 | head -30
cargo fmt -p servo-script
git add components/script_bindings/webidls/Animatable.webidl components/script_bindings/webidls/Document.webidl components/script/dom/document/animation_manager.rs components/script/dom/element/element.rs components/script/dom/document/document.rs
git commit -m "script: implement Element.getAnimations() and Document.getAnimations()

Return the relevant script animations for the element (or its subtree with
the subtree option) or the whole document, in global animation list order
(https://drafts.csswg.org/web-animations-1/#dom-animatable-getanimations)."
```
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: Test page, publish, pin

**Files:**
- Create: `crates/paladin-engine/tests/pages/get-animations.html`
- Modify: `crates/paladin-engine/tests/web_animations.rs` (`PAGES`)
- Modify: `Cargo.lock`

**Step 1: The page**

`crates/paladin-engine/tests/pages/get-animations.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>get-animations</title>
<div id="box" style="width:50px;height:50px;background:teal"><span id="child">x</span></div>
<div id="other"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('did not settle within 5s'), 5000);
const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
const same = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);
const check = (condition, what) => { if (!condition) throw new Error(what); };

(async () => {
  const box = document.getElementById('box');
  const child = document.getElementById('child');
  const other = document.getElementById('other');
  check(box.getAnimations().length === 0, 'animations before animate()');
  check(document.getAnimations().length === 0, 'document animations before animate()');

  const a1 = box.animate([{ opacity: 1 }, { opacity: 0 }], 10000);
  const a2 = child.animate([{ opacity: 1 }, { opacity: 0 }], 10000);
  const a3 = box.animate([{ opacity: 1 }, { opacity: 0 }], 10000);
  const a4 = other.animate([{ opacity: 1 }, { opacity: 0 }], 10000);
  check(same(box.getAnimations(), [a1, a3]), 'box animations in creation order');
  check(same(box.getAnimations({ subtree: true }), [a1, a2, a3]), 'subtree animations');
  check(same(child.getAnimations(), [a2]), 'child animations');
  check(same(document.getAnimations(), [a1, a2, a3, a4]), 'document animations');
  check(box.getAnimations({ pseudoElement: '::before' }).length === 0, 'pseudo-element animations');

  a1.cancel();
  check(same(box.getAnimations(), [a3]), 'cancelled animation listed');
  a3.pause();
  check(same(box.getAnimations(), [a3]), 'paused animation missing');
  a3.finish();
  check(box.getAnimations().length === 0, 'finished animation without fill listed');
  a4.effect.updateTiming({ fill: 'forwards' });
  a4.finish();
  await frame();
  check(same(other.getAnimations(), [a4]), 'finished animation with fill missing');
  check(same(document.getAnimations(), [a2, a4]), 'document animations after changes');

  // Replaying a finished animation brings it back in its original order.
  a3.play();
  check(same(box.getAnimations(), [a3]), 'replayed animation missing');
  check(same(document.getAnimations(), [a2, a3, a4]), 'replayed animation out of order');
  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

**Step 2: Register, run, publish, pin**

Append `"get-animations.html",` to `PAGES`, then:

```bash
cd /Users/thomas/vibes/paladin
cargo test -p paladin-engine --test web_animations 2>&1 | tail -15
cd ~/vibes/servo && git push origin paladin/0.7.0 && etc/paladin/publish-dist.sh dist/paladin-0.7.0 | tee /tmp/servo-dist-sha
rm ~/.cargo/config.toml
cd /Users/thomas/vibes/paladin
git checkout Cargo.lock
cargo update -p servo --precise $(cat /tmp/servo-dist-sha) 2>&1 | tail -2
cargo test --workspace 2>&1 | grep -E '^test result|FAILED|panicked'
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo fmt --all --check && echo fmt-ok
```

Expected: thirteen pages pass; all `ok`; `fmt-ok`.

**Step 3: Commit** (docs follow in Task 3; keep the code commit separate)

```bash
git add Cargo.lock crates/paladin-engine/tests
git commit -m "feat(engine): getAnimations() on elements and the document"
```
<!-- END_TASK_2 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_SUBCOMPONENT_B (tasks 3-4) -->
<!-- START_TASK_3 -->
### Task 3: Documentation and roadmap

**Files:**
- Modify: `README.md` (lines 72-128 manual checklist, 176-178 limitation)
- Modify: `docs/upstream-issues.md` (lines 38-39, 208-233, the "Carried in
  the fork" section)
- Modify: `CHANGELOG.md` (the Web Animations bullet)
- Modify: `ROADMAP.md` (append a section)
- Modify: `CLAUDE.md` and `AGENTS.md` (freshness stamp)

**Step 1: `README.md`**

Delete the limitation bullet (lines 176-178, "Sites that use the Web
Animations API ...").

Append a 12th item to the manual checklist, after item 11:

```markdown
12. **Web Animations.** Open `https://bsky.app`: the blue splash screen fades
    out and the landing page or feed renders and scrolls. (Script animations
    are sampled by Paladin's Servo and stylo forks; see
    `docs/upstream-issues.md`, "Carried in the fork".)
```

**Step 2: `docs/upstream-issues.md`**

- Remove the `### S13: Web Animations API unimplemented` entry (lines
  208-233) from the `## Servo` section, and the now-unused `[36950]` and
  `[44174]` link definitions at the end of the file (the new entry below
  writes the issue numbers inline).
- In the dependency tree (lines 38-39) change the S13 reference to point
  at "Carried in the fork: Web Animations".
- Replace the two partial S13 entries under "Carried in the fork" with one
  final entry:

```markdown
- **Web Animations API (was S13).** Upstream servo/servo#36950 and #44174:
  Servo 0.7.0 ships the API behind `dom_web_animations_enabled` with the
  playback and sampling halves unimplemented. Carried as
  `thomasmost/servo` `paladin/0.7.0`: "script: add the Web Animations
  timing calculations", "script: add AnimationPlaybackEvent", "script:
  implement the Animation timing model", "script: finish
  Element.animate()", "script: complete KeyframeEffect construction",
  "script: sample script animations through stylo", "script: implement
  Element.getAnimations() and Document.getAnimations()"; and
  `thomasmost/stylo` `paladin/0.22.0`: "Add script-driven animations to
  ElementAnimationSet". Paladin enables the pref in `Engine`. Not covered:
  `CSSAnimation`/`CSSTransition` objects, `commitStyles`, `persist`,
  replace-state removal, pseudo-element targets, shadow-root
  `getAnimations`. On each Servo bump check whether upstream has landed
  any of #36950's follow-ups and drop the overlapping patches.
```

**Step 3: `CHANGELOG.md`**

Rewrite the Web Animations bullet under `### Added` as the final wording:

```markdown
- Web Animations API: `element.animate()`, `Animation` playback (`play`,
  `pause`, `cancel`, `finish`, `reverse`, `playbackRate`,
  `updatePlaybackRate`, `currentTime`, `startTime`, `playState`,
  `pending`, `ready`, `finished`, `onfinish`, `oncancel`),
  `KeyframeEffect` options, property-indexed keyframes and
  `getComputedTiming()`, and `getAnimations()` on elements and the
  document. Script animations are sampled by the style system each frame
  like CSS animations, so bsky.app renders past its splash screen.
  Implemented as patches carried in Paladin's Servo and stylo forks.
```

Keep the "Servo and stylo are built from Paladin's forks" bullet under
`### Changed` from Phase 1 but drop its last sentence ("No patches yet;
...").

**Step 4: `ROADMAP.md`**

Append after the Phase 3 section:

```markdown
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
```

**Step 5: `CLAUDE.md` / `AGENTS.md`**

Set line 7 to `_Last updated: 2026-10-15 (forks, Web Animations)._`,
then `cp CLAUDE.md AGENTS.md`.

**Step 6: Commit**

```bash
git add README.md docs/upstream-issues.md CHANGELOG.md ROADMAP.md CLAUDE.md AGENTS.md
git commit -m "docs: Web Animations shipped from the forks; roadmap for the next plans"
git push origin vnext
```
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: Definition of Done check

**Files:** none.

**Step 1: CI**

Open a pull request from `vnext` to `main` for the final verification (the
Phase 1 pull request was closed or merged by the user after Phase 1); if
one is already open, just watch it:

```bash
gh pr list --head vnext --base main --state open
gh pr create --base main --head vnext --title "Web Animations from the Servo and stylo forks" --body "Phases 2-6 of docs/design-plans/2026-10-15-servo-fork-and-web-animations.md."
gh pr checks --watch
gh cache list --limit 20 --json key,sizeInBytes --jq '.[] | "\(.sizeInBytes/1048576|floor) MB \(.key)"'
```

Expected: both `check` jobs green on the pull request; cache total under
10 GB. Leave the pull request for the user to merge.

**Step 2: Walk the design's Definition of Done**

1. Fork infrastructure: `thomasmost/servo` has `paladin/0.7.0` (eight
   commits above `v0.7.0`: the publisher plus seven patches) and
   `dist/paladin-0.7.0`; `thomasmost/stylo` has `paladin/0.22.0` (two
   commits) and `dist/paladin-0.22.0`; the root `Cargo.toml` patches all
   71 crates; CI green; `CLAUDE.md` documents the bump procedure.
2. Web Animations: the list in the design's DoD item 2 is covered by the
   thirteen pages and the bsky.app check (Phase 5, Task 3, Step 4).
3. Roadmap: `ROADMAP.md` names the two next design plans.

Report the result to the user, including anything that was deferred: the
Servo 0.8.0 rehearsal rebase (design, "Additional Considerations") was not
part of these phases and should be scheduled when 0.8.0 and its stylo
reach crates.io.
<!-- END_TASK_4 -->
<!-- END_SUBCOMPONENT_B -->

## Phase 6 done when

- Thirteen pages pass; `cargo test --workspace`, clippy and fmt clean
  against the published dist commits.
- README, CHANGELOG, upstream-issues, ROADMAP, CLAUDE.md/AGENTS.md are
  current.
- CI green on the pull request; the design's Definition of Done holds.
