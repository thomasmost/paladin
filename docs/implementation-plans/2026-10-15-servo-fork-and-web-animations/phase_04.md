# Servo Fork and Web Animations Implementation Plan

**Goal:** stylo can hold, resolve and sample an animation whose keyframes
come from script and whose iteration progress is pushed in each tick, with
no caller yet, and Paladin still builds and behaves as before with the
patched stylo.

**Architecture:** `ElementAnimationSet` gains `script_animations:
Vec<ScriptAnimation>` beside `animations` and `transitions`. A
`ScriptAnimation` carries its keyframes (computed offset, easing,
composite, a locked declaration block each), the effect-level composite,
the iteration duration (for easing precision) and the current iteration
progress. On the element's first restyle after it is added, stylo turns
the keyframes into the same `ComputedKeyframe` steps CSS animations use
(reusing `IntermediateComputedKeyframe` resolution through the style
resolver), and from then on samples them at the stored progress inside
`get_value_map_for_active_animations`, after CSS animations and before
transitions. Script animations are never cancelled by the `animation-name`
diff; only the embedder removes them.

**Tech Stack:** stylo 0.22.0 (`style/servo/animation.rs`,
`style/matching.rs`) on the fork's `paladin/0.22.0` branch; Paladin builds
it through the local path-patch loop.

**Scope:** 6 phases from the design plan; this file is Phase 4 of 6.

**Codebase verified:** 2026-10-15 against the crates.io copy of
`stylo-0.22.0` (`servo/animation.rs` 1922 lines, `matching.rs`,
`stylesheets/keyframes_rule.rs`, `properties/`), identical to the
`v0.22.0` tag.

---

## Deviation from the design, and why

The design's stylo contract pushes a **local time** plus the full timing
(`ScriptAnimationTiming`) into stylo and has stylo work out the progress.
Verification showed that CSS animations in stylo advance iterations by
shifting `started_at` and flipping a direction flag, so none of their
time-to-progress code can be reused for a local-time input; it would have
to be re-implemented in stylo, duplicating the timing model that Phase 2
already implemented and unit-tested in the script crate. The spec's own
seam between the timing model and the effect is the **iteration
progress** (section 5.3, "the effect value of a keyframe effect" takes the
iteration progress as input). This phase therefore pushes `progress:
Option<f64>` (the transformed progress, `None` when the effect is not in
effect) instead of a local time, and drops `ScriptAnimationTiming`. The
embedder keeps `iteration_duration` on the struct only so easing functions
get the same solving precision as CSS animations. Everything else in the
design's contract (`ScriptAnimationId`, `ScriptKeyframe`, the three
`ElementAnimationSet` methods, materialisation on restyle, sampling into
the Animations cascade origin, exemption from cancellation) stands.

## Known limitations, accepted for this plan

- **Keyframes resolve once.** A script animation's keyframes are resolved
  against the element's style on the first restyle after it is added and
  not again (`needs_update()` is "not yet resolved"). Later changes to the
  base style (the implicit 0 and 1 keyframes, `em` or `var()` values, the
  underlying value for `add`/`accumulate`) are not picked up until the
  animation is re-pushed (new keyframes, new target). CSS animations in
  stylo re-resolve on `animation-*` changes only, so this matches their
  precision for the common case.
- **Overshooting easings are clamped.** The embedder pushes the
  transformed progress; an effect easing that overshoots (for example
  `cubic-bezier(.5, -.5, .5, 1.5)`) yields values outside 0..1, which the
  spec extrapolates from the first or last keyframe interval. The sampler
  clamps to the end keyframes instead.
- **The first keyframe's easing covers the implicit 0 interval.** When the
  first keyframe omits a property, its value comes from the base style, but
  the interval from 0 to the next keyframe that sets that property keeps the
  first keyframe's easing. Web Animations section 5.3 gives the implicit 0
  keyframe a linear easing instead. The CSS animation path in stylo already
  behaves this way, so the two paths agree. Accepted: it only shows when the
  first keyframe has a non-linear easing and omits a property.
- **`cancel_all_animations` clears script animations too** (node removed
  from the document, or a node rooted for a CSS animation or transition
  that is no longer rendered). The Servo side notices the missing entry on
  its next tick and pushes the keyframes again, so a re-inserted element
  resumes. A target that also carries a CSS animation or transition and
  stops being rendered is cleared and re-pushed on every tick while the
  script animation ticks; a target with only script animations is never
  rooted, so it sees no churn. Accepted: the re-push is cheap (no restyle
  happens for an unrendered node) and the case is rare.

## Facts from verification that shape this phase

- The module is `style/servo/animation.rs` (`#[cfg(feature = "servo")]`,
  re-exported as `style::animation`). `ComputedKeyframe`,
  `IntermediateComputedKeyframe`, `AnimationValueOrReference`,
  `PropertyDeclarationOffsets`, `KeyframeDataForProperty`, `Direction` and
  `PropertyAnimation`'s fields are private to that module, so the new code
  lives there and can use them.
- Keyframes can only be resolved during the style traversal:
  `IntermediateComputedKeyframe::resolve_style` needs a
  `StyleResolverForElement`, which `process_animations_for_style`
  (`matching.rs:699-788`) creates. That is where the materialisation hook
  goes.
- `ElementAnimationSet::is_empty` (line 1243) decides whether
  `process_animations_for_style` puts the set back into the map and
  whether Servo's `do_post_reflow_update` keeps it; `has_active_animation`
  (line 1273) gates `get_value_map_for_active_animations` and, through
  `DocumentAnimationSet::has_active_animations`, Servo's
  `has_css_animations`. Both must count script animations.
- Keyframe blocks are read with `context.guards.author`, so the embedder
  must wrap them with the document's author `SharedRwLock`
  (`Locked::read_with` panics on a guard from another lock).
- `IntermediateComputedKeyframe::generate_for_keyframes` debug-asserts a
  first step at 0 and a last step at 1; synthetic empty steps are added
  when the script keyframes lack them (the CSS path does the same in
  `KeyframesAnimation::from_keyframes`).
- `AnimationValue::Opacity(f32)` exists (generated), which lets unit tests
  build keyframes without a `ComputedValues`. The value map is
  `FxHashMap<OwnedPropertyDeclarationId, AnimationValue>` with
  `OwnedPropertyDeclarationId::Longhand(LonghandId::Opacity)`.
- The stylo crate has no animation tests and no dev-dependencies;
  `cargo test -p stylo --lib` inside the fork checkout runs `#[cfg(test)]`
  modules (`style/build.rs` needs `python3` on PATH; present). The stylo
  repository has no `Cargo.lock`, so that command resolves fresh
  dependency versions.

## Working in the stylo fork

```bash
cd ~/vibes/stylo && git checkout paladin/0.22.0
```

Paladin builds against the checkout through all eleven path patches (the
`style/` crate's siblings must come from the same source):

```bash
cat > ~/.cargo/config.toml <<'EOF'
[patch.crates-io]
stylo = { path = "/Users/thomas/vibes/stylo/style" }
stylo_atoms = { path = "/Users/thomas/vibes/stylo/stylo_atoms" }
stylo_derive = { path = "/Users/thomas/vibes/stylo/style_derive" }
stylo_dom = { path = "/Users/thomas/vibes/stylo/stylo_dom" }
stylo_malloc_size_of = { path = "/Users/thomas/vibes/stylo/malloc_size_of" }
stylo_static_prefs = { path = "/Users/thomas/vibes/stylo/stylo_static_prefs" }
stylo_traits = { path = "/Users/thomas/vibes/stylo/style_traits" }
selectors = { path = "/Users/thomas/vibes/stylo/selectors" }
servo_arc = { path = "/Users/thomas/vibes/stylo/servo_arc" }
to_shmem = { path = "/Users/thomas/vibes/stylo/to_shmem" }
to_shmem_derive = { path = "/Users/thomas/vibes/stylo/to_shmem_derive" }
EOF
```

Commit conventions as in Phase 2 (MPL header exists on the files touched;
`cargo fmt -p stylo` before committing). Tasks 1 and 2 form one logical
change and share one commit (Task 1 alone does not compile: its
`update_script_animations_for_new_style` calls the `materialise` that Task
2 adds).

---

<!-- START_SUBCOMPONENT_A (tasks 1-3) -->
<!-- START_TASK_1 -->
### Task 1: Types, set bookkeeping, and their tests

**Files:**
- Modify: `~/vibes/stylo/style/servo/animation.rs`

**Step 1: Write the failing tests**

Append to `style/servo/animation.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::properties::{LonghandId, OwnedPropertyDeclarationId};
    use crate::values::generics::easing::{StepPosition, TimingKeyword};

    fn linear() -> TimingFunction {
        TimingFunction::Keyword(TimingKeyword::Linear)
    }

    fn script_animation(id: u64) -> ScriptAnimation {
        ScriptAnimation::new(ScriptAnimationId(id), Vec::new(), AnimationComposition::Replace, 1.0)
    }

    fn opacity_step(start_percentage: f64, opacity: Option<f32>, timing_function: TimingFunction) -> ComputedKeyframe {
        let value = match opacity {
            Some(opacity) => AnimationValueOrReference::AnimationValue(AnimationValue::Opacity(opacity)),
            None => AnimationValueOrReference::NotDefinedHere(PropertyDeclarationOffsets {
                preceding_declaration: 0,
                following_declaration: 2,
            }),
        };
        ComputedKeyframe {
            timing_function,
            start_percentage,
            values: vec![value].into_boxed_slice(),
        }
    }

    fn sampled_opacity(steps: &[ComputedKeyframe], progress: f64) -> f32 {
        let mut map = AnimationValueMap::default();
        sample_computed_keyframes(steps, 1, progress, 1.0, &mut map);
        match map.get(&OwnedPropertyDeclarationId::Longhand(LonghandId::Opacity)) {
            Some(AnimationValue::Opacity(opacity)) => *opacity,
            other => panic!("no opacity sampled: {:?}", other),
        }
    }

    #[test]
    fn script_animations_keep_the_set_alive_and_active() {
        let mut set = ElementAnimationSet::default();
        assert!(set.is_empty());
        assert!(!set.has_active_animation());
        set.start_script_animation(script_animation(1));
        assert!(!set.is_empty());
        assert!(set.has_active_animation());
        assert!(set.dirty);
        assert!(set.needs_script_animation_update());
    }

    #[test]
    fn starting_the_same_id_again_replaces_it() {
        let mut set = ElementAnimationSet::default();
        set.start_script_animation(script_animation(7));
        set.start_script_animation(script_animation(7));
        assert_eq!(set.script_animations.len(), 1);
    }

    #[test]
    fn progress_updates_do_not_dirty_the_set() {
        let mut set = ElementAnimationSet::default();
        set.start_script_animation(script_animation(1));
        set.dirty = false;
        assert!(set.set_script_animation_progress(ScriptAnimationId(1), Some(0.5)));
        assert_eq!(set.script_animations[0].progress, Some(0.5));
        assert!(!set.dirty);
        assert!(!set.set_script_animation_progress(ScriptAnimationId(2), Some(0.5)));
    }

    #[test]
    fn cancelling_removes_and_dirties() {
        let mut set = ElementAnimationSet::default();
        set.start_script_animation(script_animation(1));
        set.dirty = false;
        set.cancel_script_animation(ScriptAnimationId(1));
        assert!(set.is_empty());
        assert!(set.dirty);
        set.dirty = false;
        set.cancel_script_animation(ScriptAnimationId(1));
        assert!(!set.dirty);
    }

    #[test]
    fn cancel_all_animations_drops_script_animations() {
        let mut set = ElementAnimationSet::default();
        set.start_script_animation(script_animation(1));
        set.cancel_all_animations();
        assert!(set.is_empty());
    }

    #[test]
    fn samples_between_two_keyframes() {
        let steps = [
            opacity_step(0.0, Some(1.0), linear()),
            opacity_step(1.0, Some(0.0), linear()),
        ];
        assert_eq!(sampled_opacity(&steps, 0.0), 1.0);
        assert_eq!(sampled_opacity(&steps, 0.25), 0.75);
        assert_eq!(sampled_opacity(&steps, 1.0), 0.0);
        assert_eq!(sampled_opacity(&steps, 1.5), 0.0);
    }

    #[test]
    fn a_keyframe_without_the_property_is_skipped() {
        let steps = [
            opacity_step(0.0, Some(1.0), linear()),
            opacity_step(0.5, None, linear()),
            opacity_step(1.0, Some(0.0), linear()),
        ];
        assert_eq!(sampled_opacity(&steps, 0.5), 0.5);
        assert_eq!(sampled_opacity(&steps, 0.75), 0.25);
    }

    #[test]
    fn the_earlier_keyframes_easing_applies_to_the_interval() {
        let steps = [
            opacity_step(0.0, Some(1.0), TimingFunction::Steps(1, StepPosition::End)),
            opacity_step(1.0, Some(0.0), linear()),
        ];
        assert_eq!(sampled_opacity(&steps, 0.5), 1.0);
        assert_eq!(sampled_opacity(&steps, 1.0), 0.0);
    }

    #[test]
    fn unmaterialised_or_out_of_effect_animations_sample_nothing() {
        let mut map = AnimationValueMap::default();
        let mut animation = script_animation(1);
        animation.progress = Some(0.5);
        animation.add_values_to_map(&mut map);
        assert!(map.is_empty());
        animation.computed = Some(ScriptComputedKeyframes {
            steps: vec![
                opacity_step(0.0, Some(1.0), linear()),
                opacity_step(1.0, Some(0.0), linear()),
            ]
            .into_boxed_slice(),
            number_of_animating_properties: 1,
        });
        animation.progress = None;
        animation.add_values_to_map(&mut map);
        assert!(map.is_empty());
        animation.progress = Some(0.5);
        animation.add_values_to_map(&mut map);
        assert_eq!(map.len(), 1);
    }
}
```

**Step 2: Run them to see them fail**

```bash
cd ~/vibes/stylo && cargo test -p stylo --lib servo::animation::tests 2>&1 | grep -E "^error" | head -5
```

Expected: errors about `ScriptAnimation`, `ScriptAnimationId`,
`start_script_animation`, `sample_computed_keyframes`.

**Step 3: Add the types**

Insert after the `KeyframesIterationState` enum (after line 129) in
`style/servo/animation.rs`:

```rust
/// Identifies a script animation within an element's set. Assigned by the
/// embedder; unique per document.
#[derive(Clone, Copy, Debug, Eq, Hash, MallocSizeOf, PartialEq)]
pub struct ScriptAnimationId(pub u64);

/// One keyframe of a script animation, as the Web Animations API supplies
/// it: offsets are already computed.
#[derive(Clone, Debug, MallocSizeOf)]
pub struct ScriptKeyframe {
    /// The computed keyframe offset, from 0 to 1.
    pub offset: f64,
    /// The keyframe-specific easing function, applied between this keyframe
    /// and the next one.
    pub easing: TimingFunction,
    /// The keyframe-specific composite operation; `None` uses the
    /// animation's.
    pub composite: Option<AnimationComposition>,
    /// The declarations of this keyframe, locked by the document's author
    /// lock (they are read with the traversal's author guard).
    #[ignore_malloc_size_of = "Arc"]
    pub declarations: Arc<Locked<PropertyDeclarationBlock>>,
}

/// An animation driven by script through the Web Animations API. The
/// embedder owns the timing model and pushes the iteration progress each
/// tick; stylo resolves the keyframes against the element's style on the
/// first restyle after the animation is added, and samples them whenever
/// the element's animation declarations are requested.
#[derive(Clone, Debug, MallocSizeOf)]
pub struct ScriptAnimation {
    /// The embedder's identifier for this animation.
    pub id: ScriptAnimationId,
    /// The keyframes, in offset order.
    pub keyframes: Vec<ScriptKeyframe>,
    /// The composite operation for keyframes that do not specify one.
    pub composite: AnimationComposition,
    /// The iteration duration in seconds. Only used to choose the solving
    /// precision of timing functions, like `Animation::duration`.
    pub iteration_duration: f64,
    /// The iteration progress to sample at, from 0 to 1; `None` while the
    /// effect is not in effect (before it starts or after it ends without
    /// a fill), in which case it contributes no values.
    pub progress: Option<f64>,
    /// The resolved keyframes, filled in on the first restyle.
    computed: Option<ScriptComputedKeyframes>,
}

/// The resolved keyframes of a script animation, in the representation
/// CSS animations use.
#[derive(Clone, Debug, MallocSizeOf)]
struct ScriptComputedKeyframes {
    steps: Box<[ComputedKeyframe]>,
    number_of_animating_properties: usize,
}

impl ScriptAnimation {
    /// A script animation that still needs its keyframes resolved.
    pub fn new(
        id: ScriptAnimationId,
        keyframes: Vec<ScriptKeyframe>,
        composite: AnimationComposition,
        iteration_duration: f64,
    ) -> Self {
        ScriptAnimation {
            id,
            keyframes,
            composite,
            iteration_duration,
            progress: None,
            computed: None,
        }
    }

    /// Whether the keyframes still need resolving against the element's
    /// style.
    pub fn needs_update(&self) -> bool {
        self.computed.is_none()
    }

    /// Adds the values sampled at the current progress to `map`.
    fn add_values_to_map(&self, map: &mut AnimationValueMap) {
        let (Some(progress), Some(computed)) = (self.progress, &self.computed) else {
            return;
        };
        // An overshooting effect easing can push the progress outside 0..1;
        // the end keyframes are used rather than extrapolating.
        sample_computed_keyframes(
            &computed.steps,
            computed.number_of_animating_properties,
            progress.clamp(0.0, 1.0),
            self.iteration_duration,
            map,
        );
    }
}
```

**Step 4: The sampler**

Insert after `Animation`'s `impl` block (after the `Debug` impl at line
1030) two free functions:

```rust
/// The keyframe that defines `property_index` at or next to
/// `keyframe_index` in `direction`, with its easing and offset.
fn relevant_keyframe_for_property<'a>(
    steps: &'a [ComputedKeyframe],
    property_index: usize,
    keyframe_index: usize,
    direction: Direction,
) -> Option<KeyframeDataForProperty<'a>> {
    let keyframe = steps.get(keyframe_index)?;
    let index = match &keyframe.values[property_index] {
        AnimationValueOrReference::AnimationValue(_) => keyframe_index,
        AnimationValueOrReference::NotDefinedHere(offsets) => match direction {
            Direction::Forward => offsets.following_declaration,
            Direction::Backward => offsets.preceding_declaration,
        },
    };
    let keyframe = steps.get(index)?;
    let AnimationValueOrReference::AnimationValue(value) = &keyframe.values[property_index] else {
        unreachable!("Referenced keyframe does not set property");
    };
    Some(KeyframeDataForProperty {
        timing_function: &keyframe.timing_function,
        start_percentage: keyframe.start_percentage,
        value,
    })
}

/// Samples `steps` at `progress` (0 to 1, going forwards) into `map`: for
/// each animating property, the two keyframes that define it around
/// `progress` are interpolated with the earlier keyframe's easing.
///
/// This is the forward-direction core of
/// `Animation::get_property_declaration_at_time`; script animations fold
/// direction and iterations into `progress` before calling it.
fn sample_computed_keyframes(
    steps: &[ComputedKeyframe],
    number_of_animating_properties: usize,
    progress: f64,
    iteration_duration: f64,
    map: &mut AnimationValueMap,
) {
    let Some(last) = steps.last() else {
        return;
    };
    let add_declarations_to_map = |keyframe: &ComputedKeyframe, map: &mut AnimationValueMap| {
        for value_or_reference in keyframe.values.iter() {
            let AnimationValueOrReference::AnimationValue(value) = value_or_reference else {
                unreachable!("First or last keyframes define all properties");
            };
            map.insert(value.id().to_owned(), value.clone());
        }
    };

    if progress >= 1.0 {
        add_declarations_to_map(last, map);
        return;
    }
    let Some(next_keyframe_index) = steps
        .iter()
        .position(|step| progress < step.start_percentage)
    else {
        add_declarations_to_map(last, map);
        return;
    };
    let prev_keyframe_index = next_keyframe_index.saturating_sub(1);
    if prev_keyframe_index == next_keyframe_index {
        add_declarations_to_map(&steps[prev_keyframe_index], map);
        return;
    }

    for property_index in 0..number_of_animating_properties {
        let Some(previous_keyframe) = relevant_keyframe_for_property(
            steps,
            property_index,
            prev_keyframe_index,
            Direction::Backward,
        ) else {
            continue;
        };
        let Some(next_keyframe) = relevant_keyframe_for_property(
            steps,
            property_index,
            next_keyframe_index,
            Direction::Forward,
        ) else {
            map.insert(
                previous_keyframe.value.id().to_owned(),
                previous_keyframe.value.clone(),
            );
            continue;
        };
        let percentage_between_keyframes =
            next_keyframe.start_percentage - previous_keyframe.start_percentage;
        let progress_between_keyframes = if percentage_between_keyframes <= 0.0 {
            1.0
        } else {
            (progress - previous_keyframe.start_percentage) / percentage_between_keyframes
        };
        let animation = PropertyAnimation {
            from: previous_keyframe.value.clone(),
            to: next_keyframe.value.clone(),
            timing_function: previous_keyframe.timing_function.clone(),
            // Bounded below so the timing function's epsilon stays finite
            // for zero-duration effects.
            duration: (percentage_between_keyframes * iteration_duration).max(1e-3),
        };
        let value = animation.calculate_value(progress_between_keyframes);
        map.insert(value.id().to_owned(), value);
    }
}
```

**Step 5: `ElementAnimationSet`**

Change the struct (line 1180) to:

```rust
#[derive(Debug, Default, MallocSizeOf)]
pub struct ElementAnimationSet {
    /// The `Animation` objects for this element.
    pub animations: Vec<Animation>,

    /// The `Transition` objects for this element.
    pub transitions: Vec<Transition>,

    /// The animations script drives through the Web Animations API.
    pub script_animations: Vec<ScriptAnimation>,

    /// Whether or not this ElementAnimationSet has had animations or transitions
    /// which have been added, removed, or had their state changed.
    pub dirty: bool,
}
```

(Keep the existing doc comments on `animations` and `transitions` as they
are in the file.)

In `cancel_all_animations` (line 1195) add, before
`self.cancel_active_transitions();`:

```rust
        if !self.script_animations.is_empty() {
            self.dirty = true;
            self.script_animations.clear();
        }
```

`is_empty` (line 1243):

```rust
    pub fn is_empty(&self) -> bool {
        self.animations.is_empty() &&
            self.transitions.is_empty() &&
            self.script_animations.is_empty()
    }
```

`has_active_animation` (line 1273):

```rust
    pub fn has_active_animation(&self) -> bool {
        !self.script_animations.is_empty() ||
            self.animations
                .iter()
                .any(|animation| animation.state != AnimationState::Canceled)
    }
```

`get_value_map_for_active_animations` (line 1600): after the loop over
`self.animations`, add

```rust
        // Script animations composite after CSS animations
        // (https://drafts.csswg.org/web-animations-1/#animation-classes).
        for animation in &self.script_animations {
            animation.add_values_to_map(&mut map);
        }
```

Add these methods to `impl ElementAnimationSet` (after
`has_active_transition`):

```rust
    /// Adds a script animation, replacing one with the same id. Its
    /// keyframes are resolved on the element's next restyle.
    pub fn start_script_animation(&mut self, animation: ScriptAnimation) {
        self.script_animations
            .retain(|existing| existing.id != animation.id);
        self.script_animations.push(animation);
        self.dirty = true;
    }

    /// Sets the iteration progress a script animation samples at. Returns
    /// false if the set holds no animation with that id (the embedder then
    /// starts it again). Progress changes alone do not dirty the set: the
    /// embedder restyles the element, and the values are read during the
    /// cascade.
    pub fn set_script_animation_progress(
        &mut self,
        id: ScriptAnimationId,
        progress: Option<f64>,
    ) -> bool {
        match self
            .script_animations
            .iter_mut()
            .find(|animation| animation.id == id)
        {
            Some(animation) => {
                animation.progress = progress;
                true
            },
            None => false,
        }
    }

    /// Removes a script animation. Its values disappear from the cascade
    /// on the next restyle.
    pub fn cancel_script_animation(&mut self, id: ScriptAnimationId) {
        let count = self.script_animations.len();
        self.script_animations
            .retain(|animation| animation.id != id);
        if self.script_animations.len() != count {
            self.dirty = true;
        }
    }

    /// Whether any script animation still needs its keyframes resolved.
    pub fn needs_script_animation_update(&self) -> bool {
        self.script_animations
            .iter()
            .any(|animation| animation.needs_update())
    }

    /// Resolves the keyframes of every script animation that still needs
    /// it against `new_style`. Must run inside the style traversal.
    pub fn update_script_animations_for_new_style<E>(
        &mut self,
        element: E,
        context: &SharedStyleContext,
        new_style: &Arc<ComputedValues>,
        resolver: &mut StyleResolverForElement<E>,
    ) where
        E: TElement,
    {
        for animation in self.script_animations.iter_mut() {
            if animation.needs_update() {
                animation.materialise(element, context, new_style, resolver);
                self.dirty = true;
            }
        }
    }
```

**Step 6: `DocumentAnimationSet` helpers**

Add to `impl DocumentAnimationSet` (after `cancel_all_animations_for_key`):

```rust
    /// Adds a script animation to the set for `key`, creating the set.
    pub fn start_script_animation(&self, key: &AnimationSetKey, animation: ScriptAnimation) {
        self.sets
            .write()
            .entry(key.clone())
            .or_default()
            .start_script_animation(animation);
    }

    /// Updates a script animation's progress; false if it is not in the set.
    pub fn set_script_animation_progress(
        &self,
        key: &AnimationSetKey,
        id: ScriptAnimationId,
        progress: Option<f64>,
    ) -> bool {
        self.sets
            .write()
            .get_mut(key)
            .is_some_and(|set| set.set_script_animation_progress(id, progress))
    }

    /// Removes a script animation from the set for `key`.
    pub fn cancel_script_animation(&self, key: &AnimationSetKey, id: ScriptAnimationId) {
        if let Some(set) = self.sets.write().get_mut(key) {
            set.cancel_script_animation(id);
        }
    }
```

**Step 7: Compile and run the tests**

`materialise` does not exist yet, so compile only the tests that do not
need it by adding it in Task 2 first, or run after Task 2. Proceed to Task
2 and run both together.
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: Materialisation on restyle

**Files:**
- Modify: `~/vibes/stylo/style/servo/animation.rs`
- Modify: `~/vibes/stylo/style/matching.rs` (lines 699-788,
  `process_animations_for_style`)

**Step 1: Split `ComputedKeyframe::generate_for_keyframes`**

The function (lines 417-557) first builds the intermediate steps, then
resolves them. Split it so the second half is reusable:

```rust
impl ComputedKeyframe {
    fn generate_for_keyframes<E>(
        element: E,
        animation: &KeyframesAnimation,
        context: &SharedStyleContext,
        base_style: &Arc<ComputedValues>,
        default_timing_function: TimingFunction,
        default_composition: AnimationComposition,
        resolver: &mut StyleResolverForElement<E>,
        animating_properties: PropertyDeclarationIdSet,
        number_of_animating_properties: usize,
    ) -> Box<[Self]>
    where
        E: TElement,
    {
        let intermediate_steps =
            IntermediateComputedKeyframe::generate_for_keyframes(animation, context, base_style);
        Self::generate_for_intermediate_steps(
            element,
            intermediate_steps,
            context,
            base_style,
            default_timing_function,
            default_composition,
            resolver,
            animating_properties,
            number_of_animating_properties,
        )
    }

    /// Resolves `intermediate_steps` (in offset order, first at 0, last at
    /// 1) against `base_style` into computed keyframes.
    fn generate_for_intermediate_steps<E>(
        element: E,
        intermediate_steps: Vec<IntermediateComputedKeyframe>,
        context: &SharedStyleContext,
        base_style: &Arc<ComputedValues>,
        default_timing_function: TimingFunction,
        default_composition: AnimationComposition,
        resolver: &mut StyleResolverForElement<E>,
        animating_properties: PropertyDeclarationIdSet,
        number_of_animating_properties: usize,
    ) -> Box<[Self]>
    where
        E: TElement,
    {
        // ... the original body from `let animation_values_from_style`
        // onwards, unchanged, minus the line that computed
        // `intermediate_steps` ...
    }
}
```

The only edit to the original body is deleting the line
`let intermediate_steps = IntermediateComputedKeyframe::generate_for_keyframes(animation, context, base_style);`.

**Step 2: `ScriptAnimation::materialise`**

Add to `impl ScriptAnimation` (from Task 1):

```rust
    /// Resolves the keyframes against `base_style`, like
    /// `maybe_start_animations` does for CSS keyframes: declarations are
    /// filtered to animatable, non-`display` longhands and made physical;
    /// synthetic keyframes at 0 and 1 hold the underlying values; the
    /// keyframe easing and composite travel with each step.
    fn materialise<E>(
        &mut self,
        element: E,
        context: &SharedStyleContext,
        base_style: &Arc<ComputedValues>,
        resolver: &mut StyleResolverForElement<E>,
    ) where
        E: TElement,
    {
        let guard = &context.guards.author;
        let mut animating_properties = PropertyDeclarationIdSet::default();
        let mut number_of_animating_properties = 0;
        let mut intermediate_steps: Vec<IntermediateComputedKeyframe> =
            Vec::with_capacity(self.keyframes.len() + 2);

        for keyframe in &self.keyframes {
            let mut step = IntermediateComputedKeyframe::new(keyframe.offset);
            step.timing_function = Some(keyframe.easing.clone());
            step.composition = Some(keyframe.composite.unwrap_or(self.composite));
            let block = keyframe.declarations.read_with(guard);
            for declaration in block.normal_declaration_iter() {
                if let PropertyDeclarationId::Longhand(id) = declaration.id() {
                    if id == LonghandId::Display || !id.is_animatable() {
                        continue;
                    }
                }
                let declaration = declaration.to_physical(base_style.writing_mode);
                if animating_properties.insert(declaration.id()) {
                    number_of_animating_properties += 1;
                }
                step.declarations.push(declaration, Importance::Normal);
            }
            intermediate_steps.push(step);
        }

        if intermediate_steps
            .first()
            .is_none_or(|step| step.start_percentage != 0.0)
        {
            intermediate_steps.insert(0, IntermediateComputedKeyframe::new(0.0));
        }
        if intermediate_steps
            .last()
            .is_none_or(|step| step.start_percentage != 1.0)
        {
            intermediate_steps.push(IntermediateComputedKeyframe::new(1.0));
        }

        let steps = ComputedKeyframe::generate_for_intermediate_steps(
            element,
            intermediate_steps,
            context,
            base_style,
            TimingFunction::Keyword(TimingKeyword::Linear),
            self.composite,
            resolver,
            animating_properties,
            number_of_animating_properties,
        );
        self.computed = Some(ScriptComputedKeyframes {
            steps,
            number_of_animating_properties,
        });
    }
```

Add `use crate::values::generics::easing::TimingKeyword;` to the imports
at the top of the file (next to `BeforeFlag`).

Notes: `IntermediateComputedKeyframe::generate_for_keyframes` merges CSS
steps with equal percentages into one; script keyframes with equal offsets
stay separate here, which the sampler handles (consecutive steps with a
zero interval are never interpolated across). `declaration.id()` borrows
the local `declaration`; `PropertyDeclarationIdSet::insert` copies what it
needs, so the following `push` is fine.

**Step 3: The hook in `matching.rs`**

In `process_animations_for_style` replace

```rust
        if needs_animations_update {
            let mut resolver = StyleResolverForElement::new(
                *self,
                context,
                RuleInclusion::All,
                PseudoElementResolution::IfApplicable,
            );
            animation_set.update_animations_for_new_style::<Self>(
                *self,
                &shared_context,
                &new_values,
                &mut resolver,
            );
        }
```

with

```rust
        if needs_animations_update || animation_set.needs_script_animation_update() {
            let mut resolver = StyleResolverForElement::new(
                *self,
                context,
                RuleInclusion::All,
                PseudoElementResolution::IfApplicable,
            );
            if needs_animations_update {
                animation_set.update_animations_for_new_style::<Self>(
                    *self,
                    &shared_context,
                    &new_values,
                    &mut resolver,
                );
            }
            // Script animations added since the last restyle resolve their
            // keyframes here; the set is then dirty and re-cascades below.
            animation_set.update_script_animations_for_new_style::<Self>(
                *self,
                &shared_context,
                &new_values,
                &mut resolver,
            );
        }
```

Nothing else in `matching.rs` changes: `needs_animations_update` keeps its
meaning (CSS animations), and the recascade that `process_animations`
performs when the set is dirty picks up the freshly resolved values in the
same restyle.

**Step 4: Run the stylo tests**

```bash
cd ~/vibes/stylo && cargo test -p stylo --lib servo::animation::tests 2>&1 | tail -5
```

Expected: `test result: ok. 9 passed`. (First run resolves and builds
stylo's dependencies, a few minutes.) If `cargo test` fails to resolve or
build a dependency because the stylo workspace has no lockfile, pin the
offending crate with `cargo update -p <crate> --precise <version from
Paladin's Cargo.lock>` and do not commit the resulting `Cargo.lock`.

**Step 5: Commit**

```bash
cd ~/vibes/stylo
cargo fmt -p stylo
git add style/servo/animation.rs style/matching.rs
git commit -m "Add script-driven animations to ElementAnimationSet

Script animations carry keyframes from the Web Animations API and an
iteration progress the embedder updates each tick. Their keyframes are
resolved against the element's style on the first restyle after they are
added, through the same path as CSS keyframes, and sampled into the
Animations cascade origin after CSS animations. They are only removed by
the embedder."
```
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: Paladin still builds and behaves as before; publish and pin

**Files:**
- Modify: `Cargo.lock`
- Modify: `CHANGELOG.md`, `docs/upstream-issues.md`

**Step 1: Build Paladin against the path-patched stylo**

With the eleven-entry `~/.cargo/config.toml` from "Working in the stylo
fork":

```bash
cd /Users/thomas/vibes/paladin
cargo build --workspace --all-targets 2>&1 | tail -3
cargo test --workspace 2>&1 | grep -E '^test result|FAILED|panicked'
```

Expected: `Finished`; every `test result: ok`, including both engine tests
(the eight Web Animations pages still pass: nothing samples the new
vector yet, and CSS animations are untouched).

**Step 2: Manual spot check**

`cargo run -- https://en.wikipedia.org/wiki/Main_Page` and a page with a
CSS animation or transition (for example
`https://developer.mozilla.org/en-US/docs/Web/CSS/animation`): animations
still play, no panics in the log.

**Step 3: Publish and pin**

```bash
cd ~/vibes/stylo && git push origin paladin/0.22.0 && etc/paladin/publish-dist.sh dist/paladin-0.22.0 | tee /tmp/stylo-dist-sha
rm ~/.cargo/config.toml
cd /Users/thomas/vibes/paladin
git checkout Cargo.lock
cargo update -p stylo --precise $(cat /tmp/stylo-dist-sha) 2>&1 | tail -2
grep -c "$(cat /tmp/stylo-dist-sha)" Cargo.lock
cargo test --workspace 2>&1 | grep -E '^test result|FAILED|panicked'
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo fmt --all --check && echo fmt-ok
```

Expected: the count is `11`; all `ok`; `fmt-ok`.

**Step 4: Docs and commit**

`CHANGELOG.md`: no user-visible change in this phase; nothing to add.

`docs/upstream-issues.md`, "Carried in the fork", add:

```markdown
- **S13 (Web Animations API), stylo half.** `thomasmost/stylo`
  `paladin/0.22.0`: "Add script-driven animations to ElementAnimationSet".
  Keyframe resolution and sampling for script animations; wired up by the
  Servo-side patches.
```

```bash
git add Cargo.lock docs/upstream-issues.md
git commit -m "build(engine): pin the stylo fork with script animation support"
git push origin vnext
```
<!-- END_TASK_3 -->
<!-- END_SUBCOMPONENT_A -->

## Phase 4 done when

- `servo::animation::tests` pass in the stylo fork (9 tests).
- Paladin builds and `cargo test --workspace` is green against the
  published stylo dist commit, with behaviour unchanged (no caller yet).
