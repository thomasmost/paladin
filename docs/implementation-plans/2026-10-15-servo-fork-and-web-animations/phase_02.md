# Servo Fork and Web Animations Implementation Plan

**Goal:** `Animation` behaves per the Web Animations timing model (play,
pause, cancel, finish, reverse, playback rate, `currentTime`, `startTime`,
`playState`, `ready`, `finished`, `onfinish`, `oncancel`) without yet
changing any pixel, and Paladin's headless engine test proves it with local
HTML pages.

**Architecture:** The pure timing calculations go in a new, unit-tested
module `components/script/dom/animations/timing.rs`. `Animation` gains the
spec state and procedures and registers with the document's
`AnimationManager` when it leaves the idle play state; the manager ticks
script animations from `Document::update_animations_and_send_events`
(running pending play and pause tasks with the timeline time as ready time,
then updating the finished state), counts them toward
`running_animation_count` so the tick loop keeps going, and holds the
document's pending animation event queue for `finish` and `cancel` events,
which `send_pending_events` dispatches after the microtask checkpoint.
Finish notifications are real microtasks (`MicrotaskRunnable`).

**Tech Stack:** Servo 0.7.0 script crate (`servo-script`, lib name
`script`) and `servo-script-bindings` on the fork's `paladin/0.7.0` branch;
Paladin's `paladin-engine` crate.

**Scope:** 6 phases from the design plan; this file is Phase 2 of 6.

**Codebase verified:** 2026-10-15 against the crates.io copies of
`servo-script-0.7.0` and `servo-script-bindings-0.7.0` (byte-identical to
the `v0.7.0` tag for the files touched).

---

## Facts from verification that shape this phase

- **No `CanGc` in this Servo.** The GC-capable context is `cx: &mut
  JSContext` (`js::context::JSContext`); `cx.no_gc()` gives `&NoGC`.
  Methods that need it are listed per interface in
  `components/script_bindings/codegen/Bindings.conf` under `'cx'`
  (getter names are bare, e.g. `Ready`; setters are `SetX`). Constructors
  and `EventHandler` attributes get `cx` automatically.
- **Promises:** `Promise::new(cx, &GlobalScope) -> RootedPromise`
  (stack), stored in DOM structs as `TracedPromise`
  (`rooted.to_traced()`, `traced.root(cx)`), resolved with
  `resolve_native(cx, &value)`, rejected with
  `reject_error(cx, Error::Abort(None))`, queried with `is_pending()` /
  `is_fulfilled()` (true for fulfilled or rejected), marked handled with
  `set_promise_is_handled(cx)`. `Promise::new_resolved(cx, global, value)`
  exists. Imports: `crate::dom::promise::{Promise, RootedPromise,
  TracedPromise}`.
- **Microtasks:** `crate::runtime::job_queue::MicrotaskRunnable` (trait
  with `fn handler(&self, cx: &mut JSContext)`; implementors derive
  `JSTraceable, MallocSizeOf` and hold `Dom<T>` fields, see `UserMicrotask`
  in `components/script/runtime/job_queue.rs:175-188`), enqueued with
  `GlobalScope::enqueue_microtask(&self, cx: &JSContext, job: Box<dyn
  MicrotaskRunnable>)`. Realm entry: `crate::realms::enter_auto_realm(cx,
  &*global)` then `let cx = &mut realm.current_realm();`.
- **Timeline time** is milliseconds from `AnimationTimeline::GetCurrentTime()
  -> Option<Finite<f64>>` (trait
  `AnimationTimelineMethods`); the manager's internal CSS-animation clock
  is seconds. `Document::Timeline()` (trait `DocumentMethods`) returns the
  default `DocumentTimeline`, whose global is the `Window`.
- **Events** are fired synchronously with `Event::fire(cx, target)` inside
  `AnimationManager::send_pending_events`, which `Document::
  update_animations_and_send_events` calls after the microtask checkpoint.
  There is no animation task source.
- **WebIDL `double`** maps to `Finite<f64>` (`*value` reads it,
  `Finite::new(v) -> Option<Finite<f64>>` builds it); `double?` to
  `Option<Finite<f64>>`; `unrestricted double` to `f64`.
- **`#[dom_struct]` types compare by identity** (`PartialEq` is derived
  by `dom_struct`), so `*a == *b` on two `Animation`s is pointer equality.
- **The script crate has no unit tests today** and no dev-dependencies;
  `#[cfg(test)]` modules compile with `cargo test -p servo-script --lib`
  inside the fork checkout (first run builds SpiderMonkey, 10 to 20
  minutes).
- `Element::Animate` already builds the effect and animation (`components/
  script/dom/element/element.rs:4595-4659`) and is listed under
  `'Element': { 'cx': [... 'Animate' ...] }`. It lacks the `id` and the
  play step, and because the 0.7.0 `KeyframeEffect` constructor ignores
  its options, Task 6 also applies the timing input to the effect (Phase 3
  moves that into the constructor).

## Working in the fork

All Servo edits happen in `~/vibes/servo` on branch `paladin/0.7.0` (Phase
1). Paladin builds against that checkout through the local path-patch loop:

```bash
cat > ~/.cargo/config.toml <<'EOF'
[patch.crates-io]
servo = { path = "/Users/thomas/vibes/servo/components/servo" }
EOF
```

(Cargo warns that the other 59 manifest patches are unused; that is
expected. Remove the file, or the table, and run `git checkout Cargo.lock`
in Paladin before committing Paladin changes.)

Commit conventions on the fork: MPL header on every new file (copy the three
lines from any existing file), `cargo fmt -p servo-script` before each
commit, one commit per task below, subject in Servo's style (imperative, no
Paladin-specific names), body naming the spec section.

Fast compile check for a task: `cd ~/vibes/servo && UV_NO_SYNC=1 cargo check
-p servo-script`.

---

<!-- START_SUBCOMPONENT_A (tasks 1-2) -->
<!-- START_TASK_1 -->
### Task 1: Pure timing calculations with unit tests

**Files:**
- Create: `~/vibes/servo/components/script/dom/animations/timing.rs`
- Modify: `~/vibes/servo/components/script/dom/animations/mod.rs`

**Step 1: Write the failing tests and the module together**

The script crate has no test scaffolding; the module and its tests are one
file. Create `components/script/dom/animations/timing.rs`:

```rust
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The Web Animations timing model as pure functions, shared by
//! `AnimationEffect::getComputedTiming()` and `Animation`.
//!
//! <https://drafts.csswg.org/web-animations-1/#timing-model>

use style::values::computed::easing::TimingFunction;
use style::values::generics::easing::BeforeFlag;

/// <https://drafts.csswg.org/web-animations-1/#fill-mode>, with `auto`
/// already resolved to `none`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Fill {
    None,
    Forwards,
    Backwards,
    Both,
}

/// <https://drafts.csswg.org/web-animations-1/#playback-direction>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Direction {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// The timing properties of an animation effect with `auto` values
/// resolved. Times are in milliseconds.
#[derive(Clone, Debug)]
pub(crate) struct Timing {
    pub(crate) start_delay: f64,
    pub(crate) end_delay: f64,
    pub(crate) fill: Fill,
    pub(crate) iteration_start: f64,
    pub(crate) iteration_count: f64,
    pub(crate) iteration_duration: f64,
    pub(crate) direction: Direction,
    pub(crate) easing: TimingFunction,
}

/// <https://drafts.csswg.org/web-animations-1/#animation-effect-phases-and-states>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Phase {
    Before,
    Active,
    After,
    Idle,
}

/// The values `getComputedTiming()` reports on top of the specified ones.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ComputedTiming {
    pub(crate) end_time: f64,
    pub(crate) active_duration: f64,
    pub(crate) local_time: Option<f64>,
    /// The transformed (iteration) progress, `None` when the effect is
    /// not in effect.
    pub(crate) progress: Option<f64>,
    pub(crate) current_iteration: Option<f64>,
}

/// <https://drafts.csswg.org/web-animations-1/#active-duration>
pub(crate) fn active_duration(timing: &Timing) -> f64 {
    // The product is undefined when one factor is zero and the other
    // infinite, so the spec special-cases zero.
    if timing.iteration_duration == 0.0 || timing.iteration_count == 0.0 {
        return 0.0;
    }
    timing.iteration_duration * timing.iteration_count
}

/// <https://drafts.csswg.org/web-animations-1/#end-time>
pub(crate) fn end_time(timing: &Timing) -> f64 {
    (timing.start_delay + active_duration(timing) + timing.end_delay).max(0.0)
}

/// <https://drafts.csswg.org/web-animations-1/#animation-effect-phases-and-states>
///
/// `playback_rate` is the associated animation's playback rate; a negative
/// value makes the animation direction "backwards".
pub(crate) fn phase(timing: &Timing, local_time: Option<f64>, playback_rate: f64) -> Phase {
    let Some(local_time) = local_time else {
        return Phase::Idle;
    };
    let end_time = end_time(timing);
    let before_active_boundary = timing.start_delay.min(end_time).max(0.0);
    let active_after_boundary = (timing.start_delay + active_duration(timing))
        .min(end_time)
        .max(0.0);
    let backwards = playback_rate < 0.0;
    if local_time < before_active_boundary || (backwards && local_time == before_active_boundary)
    {
        return Phase::Before;
    }
    if local_time > active_after_boundary || (!backwards && local_time == active_after_boundary)
    {
        return Phase::After;
    }
    Phase::Active
}

/// <https://drafts.csswg.org/web-animations-1/#calculating-the-active-time>
pub(crate) fn active_time(timing: &Timing, local_time: Option<f64>, phase: Phase) -> Option<f64> {
    let local_time = local_time?;
    match phase {
        Phase::Before => matches!(timing.fill, Fill::Backwards | Fill::Both)
            .then(|| (local_time - timing.start_delay).max(0.0)),
        Phase::Active => Some(local_time - timing.start_delay),
        Phase::After => matches!(timing.fill, Fill::Forwards | Fill::Both).then(|| {
            (local_time - timing.start_delay)
                .min(active_duration(timing))
                .max(0.0)
        }),
        Phase::Idle => None,
    }
}

/// <https://drafts.csswg.org/web-animations-1/#calculating-the-overall-progress>
fn overall_progress(timing: &Timing, active_time: f64, phase: Phase) -> f64 {
    let progress = if timing.iteration_duration == 0.0 {
        if phase == Phase::Before {
            0.0
        } else {
            timing.iteration_count
        }
    } else {
        active_time / timing.iteration_duration
    };
    progress + timing.iteration_start
}

/// <https://drafts.csswg.org/web-animations-1/#calculating-the-simple-iteration-progress>
fn simple_iteration_progress(
    timing: &Timing,
    overall_progress: f64,
    phase: Phase,
    active_time: f64,
) -> f64 {
    let progress = if overall_progress.is_infinite() {
        timing.iteration_start % 1.0
    } else {
        overall_progress % 1.0
    };
    if progress == 0.0 &&
        matches!(phase, Phase::Active | Phase::After) &&
        active_time == active_duration(timing) &&
        timing.iteration_count != 0.0
    {
        return 1.0;
    }
    progress
}

/// <https://drafts.csswg.org/web-animations-1/#calculating-the-current-iteration>
fn current_iteration(
    timing: &Timing,
    overall_progress: f64,
    simple_iteration_progress: f64,
    phase: Phase,
) -> f64 {
    if phase == Phase::After && timing.iteration_count.is_infinite() {
        return f64::INFINITY;
    }
    if simple_iteration_progress == 1.0 {
        return overall_progress.floor() - 1.0;
    }
    overall_progress.floor()
}

/// The "current direction" of
/// <https://drafts.csswg.org/web-animations-1/#calculating-the-directed-progress>:
/// true when forwards.
fn current_direction_is_forwards(timing: &Timing, current_iteration: f64) -> bool {
    match timing.direction {
        Direction::Normal => true,
        Direction::Reverse => false,
        Direction::Alternate | Direction::AlternateReverse => {
            if current_iteration.is_infinite() {
                return true;
            }
            let mut d = current_iteration;
            if timing.direction == Direction::AlternateReverse {
                d += 1.0;
            }
            d % 2.0 == 0.0
        },
    }
}

/// Runs the whole chain from local time to transformed progress.
///
/// <https://drafts.csswg.org/web-animations-1/#calculating-the-transformed-progress>
pub(crate) fn compute(timing: &Timing, local_time: Option<f64>, playback_rate: f64) -> ComputedTiming {
    let end_time = end_time(timing);
    let active_duration = active_duration(timing);
    let phase = phase(timing, local_time, playback_rate);
    let Some(active_time) = active_time(timing, local_time, phase) else {
        return ComputedTiming {
            end_time,
            active_duration,
            local_time,
            progress: None,
            current_iteration: None,
        };
    };
    let overall_progress = overall_progress(timing, active_time, phase);
    let simple_iteration_progress =
        simple_iteration_progress(timing, overall_progress, phase, active_time);
    let current_iteration =
        current_iteration(timing, overall_progress, simple_iteration_progress, phase);
    let forwards = current_direction_is_forwards(timing, current_iteration);
    let directed_progress = if forwards {
        simple_iteration_progress
    } else {
        1.0 - simple_iteration_progress
    };
    let before_flag = if (phase == Phase::Before && forwards) || (phase == Phase::After && !forwards)
    {
        BeforeFlag::Set
    } else {
        BeforeFlag::Unset
    };
    // The same tolerance stylo uses for CSS animations: a 200th of the
    // iteration (in seconds), bounded for zero-duration effects.
    let epsilon = if timing.iteration_duration > 0.0 {
        1.0 / (200.0 * timing.iteration_duration / 1000.0)
    } else {
        1.0 / 200.0
    };
    let progress = timing
        .easing
        .calculate_output(directed_progress, before_flag, epsilon);
    ComputedTiming {
        end_time,
        active_duration,
        local_time,
        progress: Some(progress),
        current_iteration: Some(current_iteration),
    }
}

#[cfg(test)]
mod tests {
    use style::values::generics::easing::TimingKeyword;

    use super::*;

    fn linear(duration: f64) -> Timing {
        Timing {
            start_delay: 0.0,
            end_delay: 0.0,
            fill: Fill::None,
            iteration_start: 0.0,
            iteration_count: 1.0,
            iteration_duration: duration,
            direction: Direction::Normal,
            easing: TimingFunction::Keyword(TimingKeyword::Linear),
        }
    }

    #[test]
    fn active_duration_is_zero_when_duration_or_count_is_zero() {
        let mut timing = linear(0.0);
        timing.iteration_count = f64::INFINITY;
        assert_eq!(active_duration(&timing), 0.0);
        let mut timing = linear(1000.0);
        timing.iteration_count = 0.0;
        assert_eq!(active_duration(&timing), 0.0);
        let mut timing = linear(1000.0);
        timing.iteration_count = 2.5;
        assert_eq!(active_duration(&timing), 2500.0);
    }

    #[test]
    fn end_time_includes_both_delays() {
        let mut timing = linear(1000.0);
        timing.start_delay = 100.0;
        timing.end_delay = 50.0;
        assert_eq!(end_time(&timing), 1150.0);
        timing.end_delay = -2000.0;
        assert_eq!(end_time(&timing), 0.0);
    }

    #[test]
    fn progress_is_linear_in_the_active_phase() {
        let computed = compute(&linear(1000.0), Some(250.0), 1.0);
        assert_eq!(computed.progress, Some(0.25));
        assert_eq!(computed.current_iteration, Some(0.0));
        assert_eq!(computed.local_time, Some(250.0));
    }

    #[test]
    fn unresolved_local_time_is_idle() {
        let computed = compute(&linear(1000.0), None, 1.0);
        assert_eq!(computed.progress, None);
        assert_eq!(computed.current_iteration, None);
        assert_eq!(phase(&linear(1000.0), None, 1.0), Phase::Idle);
    }

    #[test]
    fn before_phase_only_applies_with_a_backwards_fill() {
        let mut timing = linear(1000.0);
        timing.start_delay = 100.0;
        assert_eq!(phase(&timing, Some(50.0), 1.0), Phase::Before);
        assert_eq!(compute(&timing, Some(50.0), 1.0).progress, None);
        timing.fill = Fill::Backwards;
        assert_eq!(compute(&timing, Some(50.0), 1.0).progress, Some(0.0));
    }

    #[test]
    fn after_phase_only_applies_with_a_forwards_fill() {
        let mut timing = linear(1000.0);
        assert_eq!(phase(&timing, Some(1500.0), 1.0), Phase::After);
        assert_eq!(compute(&timing, Some(1500.0), 1.0).progress, None);
        timing.fill = Fill::Forwards;
        let computed = compute(&timing, Some(1500.0), 1.0);
        assert_eq!(computed.progress, Some(1.0));
        assert_eq!(computed.current_iteration, Some(0.0));
    }

    #[test]
    fn the_end_boundary_belongs_to_the_after_phase_when_going_forwards() {
        let timing = linear(1000.0);
        assert_eq!(phase(&timing, Some(1000.0), 1.0), Phase::After);
        assert_eq!(phase(&timing, Some(1000.0), -1.0), Phase::Active);
        assert_eq!(phase(&timing, Some(0.0), 1.0), Phase::Active);
        assert_eq!(phase(&timing, Some(0.0), -1.0), Phase::Before);
    }

    #[test]
    fn reverse_direction_counts_down() {
        let mut timing = linear(1000.0);
        timing.direction = Direction::Reverse;
        assert_eq!(compute(&timing, Some(250.0), 1.0).progress, Some(0.75));
    }

    #[test]
    fn alternate_flips_on_odd_iterations() {
        let mut timing = linear(1000.0);
        timing.iteration_count = 2.0;
        timing.direction = Direction::Alternate;
        let computed = compute(&timing, Some(1250.0), 1.0);
        assert_eq!(computed.current_iteration, Some(1.0));
        assert_eq!(computed.progress, Some(0.75));
        timing.direction = Direction::AlternateReverse;
        assert_eq!(compute(&timing, Some(1250.0), 1.0).progress, Some(0.25));
    }

    #[test]
    fn iteration_start_offsets_the_progress() {
        let mut timing = linear(1000.0);
        timing.iteration_start = 0.5;
        assert_eq!(compute(&timing, Some(0.0), 1.0).progress, Some(0.5));
    }

    #[test]
    fn infinite_iterations_never_finish() {
        let mut timing = linear(1000.0);
        timing.iteration_count = f64::INFINITY;
        assert_eq!(end_time(&timing), f64::INFINITY);
        assert_eq!(phase(&timing, Some(1e9), 1.0), Phase::Active);
        assert_eq!(compute(&timing, Some(2500.0), 1.0).current_iteration, Some(2.0));
    }

    #[test]
    fn easing_is_applied_to_the_directed_progress() {
        let mut timing = linear(1000.0);
        timing.easing = TimingFunction::ease();
        let progress = compute(&timing, Some(500.0), 1.0).progress.unwrap();
        // cubic-bezier(0.25, 0.1, 0.25, 1) at 0.5
        assert!((progress - 0.8024).abs() < 1e-3, "got {progress}");
    }
}
```

**Step 2: Declare the module**

In `components/script/dom/animations/mod.rs` add, keeping the list
alphabetical:

```rust
pub(crate) mod timing;
```

**Step 3: Run the tests**

```bash
cd ~/vibes/servo
UV_NO_SYNC=1 cargo test -p servo-script --lib animations::timing 2>&1 | tail -20
```

Expected: `test result: ok. 12 passed`. (The first run builds the script
crate's dependency tree; later runs take about a minute.) A warning about
unused functions is possible until Task 3 uses them; that is fine for this
step.

**Step 4: Commit**

```bash
cd ~/vibes/servo
cargo fmt -p servo-script
git add components/script/dom/animations/timing.rs components/script/dom/animations/mod.rs
git commit -m "script: add the Web Animations timing calculations

Pure functions for active duration, end time, phase, active time and
transformed progress (Web Animations Level 1 sections 4.6 and 4.7), with
unit tests. Used by the following commits for getComputedTiming() and the
Animation playback procedures."
```
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: `AnimationPlaybackEvent`

**Files:**
- Create: `~/vibes/servo/components/script_bindings/webidls/AnimationPlaybackEvent.webidl`
- Create: `~/vibes/servo/components/script/dom/animations/animationplaybackevent.rs`
- Modify: `~/vibes/servo/components/script/dom/animations/mod.rs`

**Step 1: The IDL**

`components/script_bindings/webidls/AnimationPlaybackEvent.webidl`:

```webidl
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// https://drafts.csswg.org/web-animations-1/#the-animationplaybackevent-interface

[Exposed=Window, Pref="dom_web_animations_enabled"]
interface AnimationPlaybackEvent : Event {
  constructor(DOMString type, optional AnimationPlaybackEventInit eventInitDict = {});
  readonly attribute double? currentTime;
  readonly attribute double? timelineTime;
};

dictionary AnimationPlaybackEventInit : EventInit {
  double? currentTime = null;
  double? timelineTime = null;
};
```

The codegen picks up every file in `webidls/`; no registration is needed.

**Step 2: The DOM object**

`components/script/dom/animations/animationplaybackevent.rs` (modelled on
`components/script/dom/event/animationevent.rs`):

```rust
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use dom_struct::dom_struct;
use js::context::JSContext;
use js::rust::HandleObject;
use script_bindings::reflector::reflect_dom_object_with_proto;
use stylo_atoms::Atom;

use crate::dom::bindings::codegen::Bindings::AnimationPlaybackEventBinding::{
    AnimationPlaybackEventInit, AnimationPlaybackEventMethods,
};
use crate::dom::bindings::codegen::Bindings::EventBinding::EventMethods;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::num::Finite;
use crate::dom::bindings::root::DomRoot;
use crate::dom::bindings::str::DOMString;
use crate::dom::event::Event;
use crate::dom::window::Window;

/// <https://drafts.csswg.org/web-animations-1/#animationplaybackevent>
#[dom_struct]
pub(crate) struct AnimationPlaybackEvent {
    event: Event,
    current_time: Option<Finite<f64>>,
    timeline_time: Option<Finite<f64>>,
}

impl AnimationPlaybackEvent {
    fn new_inherited(init: &AnimationPlaybackEventInit) -> AnimationPlaybackEvent {
        AnimationPlaybackEvent {
            event: Event::new_inherited(),
            current_time: init.currentTime,
            timeline_time: init.timelineTime,
        }
    }

    pub(crate) fn new(
        cx: &mut JSContext,
        window: &Window,
        type_: Atom,
        init: &AnimationPlaybackEventInit,
    ) -> DomRoot<AnimationPlaybackEvent> {
        Self::new_with_proto(cx, window, None, type_, init)
    }

    fn new_with_proto(
        cx: &mut JSContext,
        window: &Window,
        proto: Option<HandleObject>,
        type_: Atom,
        init: &AnimationPlaybackEventInit,
    ) -> DomRoot<AnimationPlaybackEvent> {
        let event = reflect_dom_object_with_proto(
            cx,
            Box::new(AnimationPlaybackEvent::new_inherited(init)),
            window,
            proto,
        );
        {
            let event = event.upcast::<Event>();
            event.init_event(type_, init.parent.bubbles, init.parent.cancelable);
        }
        event
    }
}

impl AnimationPlaybackEventMethods<crate::DomTypeHolder> for AnimationPlaybackEvent {
    fn Constructor(
        cx: &mut JSContext,
        window: &Window,
        proto: Option<HandleObject>,
        type_: DOMString,
        init: &AnimationPlaybackEventInit,
    ) -> DomRoot<AnimationPlaybackEvent> {
        AnimationPlaybackEvent::new_with_proto(cx, window, proto, Atom::from(type_), init)
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animationplaybackevent-currenttime>
    fn GetCurrentTime(&self) -> Option<Finite<f64>> {
        self.current_time
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animationplaybackevent-timelinetime>
    fn GetTimelineTime(&self) -> Option<Finite<f64>> {
        self.timeline_time
    }

    /// <https://dom.spec.whatwg.org/#dom-event-istrusted>
    fn IsTrusted(&self) -> bool {
        self.upcast::<Event>().IsTrusted()
    }
}
```

**Step 3: Declare the module**

In `components/script/dom/animations/mod.rs` add `pub(crate) mod
animationplaybackevent;` (alphabetical: after `animationeffect`). The
generated `InterfaceTypes.rs` refers to
`crate::dom::animationplaybackevent::AnimationPlaybackEvent`, which
resolves through the `pub(crate) use self::animations::*;` glob in
`components/script/dom/mod.rs:217`.

**Step 4: Compile**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo check -p servo-script 2>&1 | tail -3
```

Expected: `Finished` (warnings about the unused `new` are fine until Task
4).

**Step 5: Commit**

```bash
cd ~/vibes/servo
cargo fmt -p servo-script
git add components/script_bindings/webidls/AnimationPlaybackEvent.webidl components/script/dom/animations/animationplaybackevent.rs components/script/dom/animations/mod.rs
git commit -m "script: add AnimationPlaybackEvent

The event type for finish and cancel notifications
(https://drafts.csswg.org/web-animations-1/#the-animationplaybackevent-interface),
behind the dom_web_animations_enabled pref like the rest of the API."
```
<!-- END_TASK_2 -->
<!-- END_SUBCOMPONENT_A -->

<!-- START_SUBCOMPONENT_B (tasks 3-5) -->
<!-- START_TASK_3 -->
### Task 3: Restore the `Animation` IDL and give `AnimationEffect` its timing

**Files:**
- Modify: `~/vibes/servo/components/script_bindings/webidls/Animation.webidl`
- Modify: `~/vibes/servo/components/script_bindings/codegen/Bindings.conf`
- Modify: `~/vibes/servo/components/script/dom/animations/animationeffect.rs`

**Step 1: `Animation.webidl`**

Replace the interface body so the file reads (keep the MPL header and the
spec URL comment above it):

```webidl
[Exposed=Window, Pref="dom_web_animations_enabled"]
interface Animation : EventTarget {
  constructor(optional AnimationEffect? effect = null,
              optional AnimationTimeline? timeline);
           attribute DOMString                id;
           attribute AnimationEffect?         effect;
           attribute AnimationTimeline?       timeline;
           attribute double?                  startTime;
  [SetterThrows]
           attribute double?                  currentTime;
           attribute double                   playbackRate;
  readonly attribute AnimationPlayState       playState;
  readonly attribute AnimationReplaceState    replaceState;
  readonly attribute boolean                  pending;
  readonly attribute Promise<Animation>       ready;
  readonly attribute Promise<Animation>       finished;
           attribute EventHandler             onfinish;
           attribute EventHandler             oncancel;
           attribute EventHandler             onremove;
  undefined cancel();
  [Throws] undefined finish();
  [Throws] undefined play();
  [Throws] undefined pause();
  [Throws] undefined updatePlaybackRate(double playbackRate);
  [Throws] undefined reverse();
  // undefined persist();
  // [CEReactions]
  // undefined commitStyles();
};

enum AnimationPlayState { "idle", "running", "paused", "finished" };

enum AnimationReplaceState { "active", "removed", "persisted" };
```

**Step 2: `Bindings.conf`**

Insert, in alphabetical order among the top-level entries (before
`'AudioBuffer'` or whichever `'A…'` entry follows `Animation`
alphabetically; check with `grep -n "^'A" Bindings.conf`):

```python
'Animation': {
    'cx': [
        'Cancel', 'Finish', 'Finished', 'Pause', 'Play', 'Ready', 'Reverse',
        'SetCurrentTime', 'SetEffect', 'SetPlaybackRate', 'SetStartTime',
        'SetTimeline', 'UpdatePlaybackRate',
    ]
},

'AnimationEffect': {
    'cx': ['UpdateTiming']
},
```

**Step 3: `animationeffect.rs`**

Add imports:

```rust
use js::context::JSContext;

use crate::dom::animation::Animation;
use crate::dom::animations::timing::{self, Timing};
use crate::dom::bindings::root::{DomRoot, MutNullableDom};
```

Give `new_inherited` the spec defaults instead of placeholders (an effect
built without options, or by `animate()` with a bare duration, keeps the
members it was not given). Replace the `specified_timing_properties:
DomRefCell::new(SpecifiedTimingProperties { ... })` initialiser and the
three-line comment above it with:

```rust
            // The defaults of the EffectTiming dictionary: members that the
            // constructor's options do not mention keep these.
            specified_timing_properties: DomRefCell::new(SpecifiedTimingProperties {
                start_delay: Finite::wrap(0.0),
                end_delay: Finite::wrap(0.0),
                fill_mode: FillMode::Auto,
                iteration_count: 1.0,
                iteration_start: Finite::wrap(0.0),
                iteration_duration: IterationDurationOrAuto::Auto,
                playback_direction: PlaybackDirection::Normal,
                easing_function: TimingFunction::Keyword(TimingKeyword::Linear),
            }),
```

Add two free functions at the end of the file (before
`IterationDurationOrAuto`), used by `Element::Animate` now and by the
`KeyframeEffect` constructor in Phase 3:

```rust
/// Every member of `timing` as an `OptionalEffectTiming`: the "timing
/// input" of the KeyframeEffect constructor for a dictionary argument.
pub(crate) fn optional_effect_timing(timing: &EffectTiming) -> OptionalEffectTiming {
    OptionalEffectTiming {
        delay: Some(timing.delay),
        direction: Some(timing.direction),
        duration: Some(match &timing.duration {
            UnrestrictedDoubleOrString::UnrestrictedDouble(duration) => {
                UnrestrictedDoubleOrString::UnrestrictedDouble(*duration)
            },
            UnrestrictedDoubleOrString::String(duration) => {
                UnrestrictedDoubleOrString::String(duration.clone())
            },
        }),
        easing: Some(timing.easing.clone()),
        endDelay: Some(timing.endDelay),
        fill: Some(timing.fill),
        iterationStart: Some(timing.iterationStart),
        iterations: Some(timing.iterations),
    }
}

/// The "timing input" for a bare number argument: a duration, with every
/// other member left at its default.
pub(crate) fn optional_effect_timing_for_duration(duration: f64) -> OptionalEffectTiming {
    OptionalEffectTiming {
        delay: None,
        direction: None,
        duration: Some(UnrestrictedDoubleOrString::UnrestrictedDouble(duration)),
        easing: None,
        endDelay: None,
        fill: None,
        iterationStart: None,
        iterations: None,
    }
}
```

Add a field to the struct, after `specified_timing_properties`:

```rust
    /// The animation this effect is associated with, if any.
    ///
    /// <https://drafts.csswg.org/web-animations-1/#animation-associated-effect>
    animation: MutNullableDom<Animation>,
```

and initialise it in `new_inherited` with `animation: Default::default(),`.

Add these methods to `impl AnimationEffect` (after `window()`):

```rust
    pub(crate) fn animation(&self) -> Option<DomRoot<Animation>> {
        self.animation.get()
    }

    /// Called by `Animation::set_the_associated_effect`; keeps the back
    /// pointer that local time and finished-state updates need.
    pub(crate) fn set_animation(&self, animation: Option<&Animation>) {
        self.animation.set(animation);
    }

    /// The timing properties with `auto` resolved, as the timing model
    /// consumes them.
    pub(crate) fn timing(&self) -> Timing {
        let properties = self.specified_timing_properties.borrow();
        Timing {
            start_delay: *properties.start_delay,
            end_delay: *properties.end_delay,
            fill: match properties.fill_mode {
                FillMode::None | FillMode::Auto => timing::Fill::None,
                FillMode::Forwards => timing::Fill::Forwards,
                FillMode::Backwards => timing::Fill::Backwards,
                FillMode::Both => timing::Fill::Both,
            },
            iteration_start: *properties.iteration_start,
            iteration_count: properties.iteration_count,
            iteration_duration: properties.iteration_duration.computed_value(),
            direction: match properties.playback_direction {
                PlaybackDirection::Normal => timing::Direction::Normal,
                PlaybackDirection::Reverse => timing::Direction::Reverse,
                PlaybackDirection::Alternate => timing::Direction::Alternate,
                PlaybackDirection::Alternate_reverse => timing::Direction::AlternateReverse,
            },
            easing: properties
                .easing_function
                .to_computed_value_without_context(),
        }
    }

    /// <https://drafts.csswg.org/web-animations-1/#end-time>
    pub(crate) fn end_time(&self) -> f64 {
        timing::end_time(&self.timing())
    }
```

(The generated variant for `"alternate-reverse"` is
`PlaybackDirection::Alternate_reverse`; confirm with
`grep -n "Alternate" target/debug/build/servo-script-bindings-*/out/Bindings/AnimationEffectBinding.rs`
in Paladin after the next build if the compiler disagrees.)

(`DomRoot` comes from the import added above; the file had only
`script_bindings::root::Dom`.)

Change `update_the_timing_properties` to take the context and run spec
step 6:

```rust
    pub(crate) fn update_the_timing_properties(
        &self,
        cx: &mut JSContext,
        input: &OptionalEffectTiming,
    ) -> Fallible<()> {
```

and at the end, replace the final `Ok(())` with:

```rust
        drop(specified_timing_properties);

        // Step 6. If effect is associated with an animation, run the
        // procedure to update an animation's finished state.
        if let Some(animation) = self.animation.get() {
            animation.update_finished_state(cx, false, false);
        }
        Ok(())
```

Update the trait method:

```rust
    fn UpdateTiming(&self, cx: &mut JSContext, timing: &OptionalEffectTiming) -> Fallible<()> {
        self.update_the_timing_properties(cx, timing)
    }
```

Replace the placeholder values in `GetComputedTiming`: before the
`ComputedEffectTiming { ... }` literal add

```rust
        let (local_time, playback_rate) = match self.animation.get() {
            Some(animation) => (animation.current_time(), animation.playback_rate()),
            None => (None, 1.0),
        };
        let computed = timing::compute(&self.timing(), local_time, playback_rate);
```

and replace the five `None` fields with

```rust
            endTime: Some(computed.end_time),
            activeDuration: Some(computed.active_duration),
            localTime: Some(computed.local_time.and_then(Finite::new)),
            progress: Some(computed.progress.and_then(Finite::new)),
            currentIteration: Some(computed.current_iteration),
```

(The `ComputedEffectTiming` members have no IDL defaults, so the generated
struct wraps each in an outer `Option` meaning "present":
`endTime`/`activeDuration: Option<f64>`, `currentIteration:
Option<Option<f64>>`, `localTime`/`progress: Option<Option<Finite<f64>>>`.
An unresolved value must be `Some(None)` so JS sees `null`; an outer `None`
would make the member `undefined`.)

**Step 4: Compile check is deferred to Task 4** (this task references
`Animation::update_finished_state`, `current_time` and `playback_rate`,
which Task 4 adds). Commit Tasks 3 and 4 together.
<!-- END_TASK_3 -->

<!-- START_TASK_4 -->
### Task 4: The `Animation` timing model

**Files:**
- Modify: `~/vibes/servo/components/script/dom/animations/animation.rs`
  (replace the whole file)

**Step 1: Replace `animation.rs`**

```rust
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use dom_struct::dom_struct;
use js::context::JSContext;
use js::rust::HandleObject;
use script_bindings::cell::DomRefCell;
use script_bindings::codegen::GenericBindings::AnimationBinding::{
    AnimationMethods, AnimationPlayState, AnimationReplaceState,
};
use script_bindings::codegen::GenericBindings::AnimationTimelineBinding::AnimationTimelineMethods;
use script_bindings::codegen::GenericBindings::DocumentBinding::DocumentMethods;
use script_bindings::codegen::GenericBindings::WindowBinding::WindowMethods;
use script_bindings::error::{Error, Fallible};
use script_bindings::inheritance::Castable;
use script_bindings::num::Finite;
use script_bindings::reflector::reflect_dom_object_with_proto;
use script_bindings::refcounted::Trusted;
use script_bindings::root::DomRoot;
use script_bindings::str::DOMString;
use stylo_atoms::Atom;

use crate::dom::animationeffect::AnimationEffect;
use crate::dom::animationplaybackevent::AnimationPlaybackEvent;
use crate::dom::animations::timing;
use crate::dom::bindings::codegen::Bindings::AnimationPlaybackEventBinding::AnimationPlaybackEventInit;
use crate::dom::bindings::codegen::Bindings::EventBinding::EventInit;
use crate::dom::bindings::reflector::DomGlobal;
use crate::dom::bindings::root::{Dom, MutNullableDom};
use crate::dom::document::Document;
use crate::dom::event::Event;
use crate::dom::eventtarget::EventTarget;
use crate::dom::globalscope::GlobalScope;
use crate::dom::promise::{Promise, RootedPromise, TracedPromise};
use crate::dom::types::{AnimationTimeline, DocumentTimeline};
use crate::dom::window::Window;
use crate::realms::enter_auto_realm;
use crate::runtime::job_queue::MicrotaskRunnable;

/// Position in the global animation list, assigned at creation.
///
/// <https://drafts.csswg.org/web-animations-1/#global-animation-list>
static NEXT_SEQUENCE_NUMBER: AtomicU64 = AtomicU64::new(1);

/// A scheduled "pending play task" or "pending pause task". The task runs
/// on the next animation frame, when the timeline provides the ready time.
#[derive(Clone, Copy, Debug, JSTraceable, MallocSizeOf, PartialEq)]
enum PendingTask {
    Play,
    Pause,
}

/// <https://drafts.csswg.org/web-animations-1/#animation-playback-event-types>
#[derive(Clone, Copy, Debug, JSTraceable, MallocSizeOf, PartialEq)]
pub(crate) enum PlaybackEventType {
    Finish,
    Cancel,
}

impl PlaybackEventType {
    fn atom(self) -> Atom {
        match self {
            PlaybackEventType::Finish => Atom::from("finish"),
            PlaybackEventType::Cancel => Atom::from("cancel"),
        }
    }
}

/// <https://drafts.csswg.org/web-animations-1/#animation>
#[dom_struct]
pub(crate) struct Animation {
    event_target: EventTarget,

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-id>
    id: DomRefCell<DOMString>,

    /// <https://drafts.csswg.org/web-animations-1/#timeline>
    timeline: MutNullableDom<AnimationTimeline>,

    /// <https://drafts.csswg.org/web-animations-1/#animation-associated-effect>
    associated_effect: MutNullableDom<AnimationEffect>,

    /// <https://drafts.csswg.org/web-animations-1/#animation-start-time>
    start_time: Cell<Option<f64>>,

    /// <https://drafts.csswg.org/web-animations-1/#animation-hold-time>
    hold_time: Cell<Option<f64>>,

    /// <https://drafts.csswg.org/web-animations-1/#animation-previous-current-time>
    previous_current_time: Cell<Option<f64>>,

    /// <https://drafts.csswg.org/web-animations-1/#animation-playback-rate>
    playback_rate: Cell<f64>,

    /// <https://drafts.csswg.org/web-animations-1/#pending-playback-rate>
    pending_playback_rate: Cell<Option<f64>>,

    /// The pending play or pause task, if one is scheduled.
    pending_task: Cell<Option<PendingTask>>,

    /// <https://drafts.csswg.org/web-animations-1/#current-ready-promise>
    ready: DomRefCell<TracedPromise>,

    /// <https://drafts.csswg.org/web-animations-1/#current-finished-promise>
    finished: DomRefCell<TracedPromise>,

    /// Whether a microtask to run the finish notification steps is queued.
    finish_notification_queued: Cell<bool>,

    /// <https://drafts.csswg.org/web-animations-1/#animation-replace-state>
    replace_state: Cell<AnimationReplaceState>,

    /// Position in the global animation list.
    sequence_number: u64,
}

impl Animation {
    fn new_inherited(ready: &RootedPromise, finished: &RootedPromise) -> Self {
        Self {
            event_target: EventTarget::new_inherited(),
            id: DomRefCell::new(DOMString::new()),
            timeline: Default::default(),
            associated_effect: Default::default(),
            start_time: Cell::new(None),
            hold_time: Cell::new(None),
            previous_current_time: Cell::new(None),
            playback_rate: Cell::new(1.0),
            pending_playback_rate: Cell::new(None),
            pending_task: Cell::new(None),
            ready: DomRefCell::new(ready.to_traced()),
            finished: DomRefCell::new(finished.to_traced()),
            finish_notification_queued: Cell::new(false),
            replace_state: Cell::new(AnimationReplaceState::Active),
            sequence_number: NEXT_SEQUENCE_NUMBER.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn new_with_proto(
        cx: &mut JSContext,
        global: &GlobalScope,
        proto: Option<HandleObject>,
    ) -> DomRoot<Self> {
        let ready = Promise::new(cx, global);
        let finished = Promise::new(cx, global);
        let animation = reflect_dom_object_with_proto(
            cx,
            Box::new(Self::new_inherited(&ready, &finished)),
            global,
            proto,
        );
        // The current ready promise is initially resolved with the
        // animation itself; the finished promise stays pending.
        ready.resolve_native(cx, &*animation);
        animation
    }

    /// Position in the global animation list; `getAnimations()` sorts by it.
    #[allow(dead_code)] // Used from Phase 6 on; remove the attribute then.
    pub(crate) fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    pub(crate) fn playback_rate(&self) -> f64 {
        self.playback_rate.get()
    }

    /// The document whose animation manager ticks this animation.
    ///
    /// <https://drafts.csswg.org/web-animations-1/#animation-document-for-timing>
    fn document_for_timing(&self) -> Option<DomRoot<Document>> {
        let timeline = self.timeline.get()?;
        timeline.downcast::<DocumentTimeline>()?;
        Some(timeline.global().as_window().Document())
    }

    /// The current time of the associated timeline, in milliseconds;
    /// `None` when there is no timeline or it is inactive.
    fn timeline_time(&self) -> Option<f64> {
        self.timeline
            .get()
            .and_then(|timeline| timeline.GetCurrentTime())
            .map(|time| *time)
    }

    /// <https://drafts.csswg.org/web-animations-1/#the-current-time-of-an-animation>
    pub(crate) fn current_time(&self) -> Option<f64> {
        if let Some(hold_time) = self.hold_time.get() {
            return Some(hold_time);
        }
        self.unconstrained_current_time()
    }

    /// The current time calculated with an unresolved hold time.
    fn unconstrained_current_time(&self) -> Option<f64> {
        let timeline_time = self.timeline_time()?;
        let start_time = self.start_time.get()?;
        Some((timeline_time - start_time) * self.playback_rate.get())
    }

    /// <https://drafts.csswg.org/web-animations-1/#effective-playback-rate>
    fn effective_playback_rate(&self) -> f64 {
        self.pending_playback_rate
            .get()
            .unwrap_or_else(|| self.playback_rate.get())
    }

    /// <https://drafts.csswg.org/web-animations-1/#apply-any-pending-playback-rate>
    fn apply_any_pending_playback_rate(&self) {
        if let Some(rate) = self.pending_playback_rate.take() {
            self.playback_rate.set(rate);
        }
    }

    /// <https://drafts.csswg.org/web-animations-1/#associated-effect-end>
    fn associated_effect_end(&self) -> f64 {
        self.associated_effect
            .get()
            .map_or(0.0, |effect| effect.end_time())
    }

    /// <https://drafts.csswg.org/web-animations-1/#play-states>
    pub(crate) fn play_state(&self) -> AnimationPlayState {
        let current_time = self.current_time();
        let start_time = self.start_time.get();
        let pending_task = self.pending_task.get();
        if current_time.is_none() && start_time.is_none() && pending_task.is_none() {
            return AnimationPlayState::Idle;
        }
        if pending_task == Some(PendingTask::Pause) ||
            (start_time.is_none() && pending_task != Some(PendingTask::Play))
        {
            return AnimationPlayState::Paused;
        }
        if let Some(current_time) = current_time {
            let rate = self.effective_playback_rate();
            let end = self.associated_effect_end();
            if (rate > 0.0 && current_time >= end) || (rate < 0.0 && current_time <= 0.0) {
                return AnimationPlayState::Finished;
            }
        }
        AnimationPlayState::Running
    }

    /// Whether the manager must keep ticking for this animation.
    pub(crate) fn needs_ticks(&self) -> bool {
        self.pending_task.get().is_some() || self.play_state() == AnimationPlayState::Running
    }

    /// Whether the effect is in effect or current, which is what keeps a
    /// finished animation observable through `getAnimations()`.
    ///
    /// <https://drafts.csswg.org/web-animations-1/#relevant>
    pub(crate) fn is_relevant(&self) -> bool {
        let Some(effect) = self.associated_effect.get() else {
            return false;
        };
        let timing = effect.timing();
        let local_time = self.current_time();
        let rate = self.playback_rate.get();
        let phase = timing::phase(&timing, local_time, rate);
        let in_effect = timing::active_time(&timing, local_time, phase).is_some();
        let current = match phase {
            timing::Phase::Active => self.play_state() != AnimationPlayState::Finished,
            timing::Phase::Before => rate > 0.0,
            timing::Phase::After => rate < 0.0,
            timing::Phase::Idle => false,
        };
        in_effect || current
    }

    fn replace_ready_promise(&self, cx: &mut JSContext) {
        let promise = Promise::new(cx, &self.global());
        *self.ready.borrow_mut() = promise.to_traced();
    }

    fn resolve_ready_promise(&self, cx: &mut JSContext) {
        let promise = self.ready.borrow().root(cx);
        promise.resolve_native(cx, self);
    }

    fn replace_finished_promise(&self, cx: &mut JSContext) {
        let promise = Promise::new(cx, &self.global());
        *self.finished.borrow_mut() = promise.to_traced();
    }

    /// Hands the animation to the document's manager, which ticks it from
    /// then on. Idempotent.
    fn register_with_document(&self) {
        if let Some(document) = self.document_for_timing() {
            document.animation_manager().register_script_animation(self);
        }
    }

    /// <https://drafts.csswg.org/web-animations-1/#setting-the-timeline>
    fn set_the_timeline(&self, cx: &mut JSContext, new_timeline: Option<&AnimationTimeline>) {
        let old_timeline = self.timeline.get();
        if old_timeline.as_deref() == new_timeline {
            return;
        }
        self.timeline.set(new_timeline);
        if self.start_time.get().is_some() {
            self.hold_time.set(None);
        }
        self.update_finished_state(cx, false, false);
    }

    /// <https://drafts.csswg.org/web-animations-1/#setting-the-associated-effect>
    fn set_the_associated_effect(&self, cx: &mut JSContext, new_effect: Option<&AnimationEffect>) {
        let old_effect = self.associated_effect.get();
        if old_effect.as_deref() == new_effect {
            return;
        }
        // Steps 3 and 4: a pending task is rescheduled to run when the
        // animation is ready, which is the next frame either way.
        if let Some(new_effect) = new_effect {
            // Step 5. An effect belongs to at most one animation.
            if let Some(previous_animation) = new_effect.animation() {
                previous_animation.set_the_associated_effect(cx, None);
            }
        }
        if let Some(old_effect) = old_effect {
            old_effect.set_animation(None);
        }
        self.associated_effect.set(new_effect);
        if let Some(new_effect) = new_effect {
            new_effect.set_animation(Some(self));
        }
        self.update_finished_state(cx, false, false);
    }

    /// <https://drafts.csswg.org/web-animations-1/#silently-set-the-current-time>
    fn silently_set_the_current_time(&self, seek_time: Option<f64>) -> Fallible<()> {
        let Some(seek_time) = seek_time else {
            if self.current_time().is_some() {
                return Err(Error::Type(
                    c"currentTime may not be set to null on an animation with a resolved current time"
                        .to_owned(),
                ));
            }
            return Ok(());
        };
        let timeline_time = self.timeline_time();
        if self.hold_time.get().is_some() ||
            self.start_time.get().is_none() ||
            timeline_time.is_none() ||
            self.playback_rate.get() == 0.0
        {
            self.hold_time.set(Some(seek_time));
        } else if let Some(timeline_time) = timeline_time {
            self.start_time
                .set(Some(timeline_time - seek_time / self.playback_rate.get()));
        }
        if timeline_time.is_none() {
            self.start_time.set(None);
        }
        self.previous_current_time.set(None);
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#set-the-current-time>
    fn set_the_current_time(&self, cx: &mut JSContext, seek_time: Option<f64>) -> Fallible<()> {
        self.silently_set_the_current_time(seek_time)?;
        if self.pending_task.get() == Some(PendingTask::Pause) {
            self.hold_time.set(seek_time);
            self.apply_any_pending_playback_rate();
            self.start_time.set(None);
            self.pending_task.set(None);
            self.resolve_ready_promise(cx);
        }
        self.register_with_document();
        self.update_finished_state(cx, true, false);
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#set-the-start-time>
    fn set_the_start_time(&self, cx: &mut JSContext, new_start_time: Option<f64>) {
        let timeline_time = self.timeline_time();
        if timeline_time.is_none() && new_start_time.is_some() {
            self.hold_time.set(None);
        }
        let previous_current_time = self.current_time();
        self.apply_any_pending_playback_rate();
        self.start_time.set(new_start_time);
        if new_start_time.is_some() {
            if self.playback_rate.get() != 0.0 {
                self.hold_time.set(None);
            }
        } else {
            self.hold_time.set(previous_current_time);
        }
        if self.pending_task.take().is_some() {
            self.resolve_ready_promise(cx);
        }
        self.register_with_document();
        self.update_finished_state(cx, true, false);
    }

    /// <https://drafts.csswg.org/web-animations-1/#play-an-animation>
    pub(crate) fn play_an_animation(&self, cx: &mut JSContext, auto_rewind: bool) -> Fallible<()> {
        // Step 1.
        let aborted_pause = self.pending_task.get() == Some(PendingTask::Pause);
        // Step 2.
        let mut has_pending_ready_promise = false;
        // Step 3.
        let mut seek_time: Option<f64> = None;

        // Step 4.
        if auto_rewind {
            let rate = self.effective_playback_rate();
            let current_time = self.current_time();
            let end = self.associated_effect_end();
            if rate >= 0.0 && current_time.is_none_or(|time| time < 0.0 || time >= end) {
                seek_time = Some(0.0);
            } else if rate < 0.0 && current_time.is_none_or(|time| time <= 0.0 || time > end) {
                if end.is_infinite() {
                    return Err(Error::InvalidState(None));
                }
                seek_time = Some(end);
            }
        }

        // Step 5.
        if seek_time.is_none() && self.start_time.get().is_none() && self.current_time().is_none()
        {
            seek_time = Some(0.0);
        }

        // Steps 6 and 7. Document timelines are monotonically increasing,
        // so "has finite timeline" is always false here.
        if let Some(seek_time) = seek_time {
            self.hold_time.set(Some(seek_time));
        }

        // Step 8.
        if self.hold_time.get().is_some() {
            self.start_time.set(None);
        }

        // Step 9.
        if self.pending_task.take().is_some() {
            has_pending_ready_promise = true;
        }

        // Step 10.
        if self.hold_time.get().is_none() &&
            seek_time.is_none() &&
            !aborted_pause &&
            self.pending_playback_rate.get().is_none()
        {
            return Ok(());
        }

        // Step 11.
        if !has_pending_ready_promise {
            self.replace_ready_promise(cx);
        }

        // Step 12. The task runs on the next animation frame.
        self.pending_task.set(Some(PendingTask::Play));
        self.register_with_document();

        // Step 13.
        self.update_finished_state(cx, false, false);
        Ok(())
    }

    /// The pending play task of
    /// <https://drafts.csswg.org/web-animations-1/#play-an-animation>, step 12.
    fn run_pending_play_task(&self, cx: &mut JSContext, ready_time: f64) {
        debug_assert!(self.start_time.get().is_some() || self.hold_time.get().is_some());
        if let Some(hold_time) = self.hold_time.get() {
            self.apply_any_pending_playback_rate();
            let rate = self.playback_rate.get();
            let new_start_time = if rate == 0.0 {
                ready_time
            } else {
                ready_time - hold_time / rate
            };
            self.start_time.set(Some(new_start_time));
            if rate != 0.0 {
                self.hold_time.set(None);
            }
        } else if let (Some(start_time), Some(_)) =
            (self.start_time.get(), self.pending_playback_rate.get())
        {
            let current_time_to_match = (ready_time - start_time) * self.playback_rate.get();
            self.apply_any_pending_playback_rate();
            let rate = self.playback_rate.get();
            if rate == 0.0 {
                self.hold_time.set(Some(current_time_to_match));
            }
            let new_start_time = if rate == 0.0 {
                ready_time
            } else {
                ready_time - current_time_to_match / rate
            };
            self.start_time.set(Some(new_start_time));
        }
        self.resolve_ready_promise(cx);
        self.update_finished_state(cx, false, false);
    }

    /// <https://drafts.csswg.org/web-animations-1/#pause-an-animation>
    fn pause_an_animation(&self, cx: &mut JSContext) -> Fallible<()> {
        // Steps 1 and 2.
        if self.pending_task.get() == Some(PendingTask::Pause) ||
            self.play_state() == AnimationPlayState::Paused
        {
            return Ok(());
        }
        // Steps 3 to 5.
        let mut seek_time = None;
        if self.current_time().is_none() {
            if self.playback_rate.get() >= 0.0 {
                seek_time = Some(0.0);
            } else {
                let end = self.associated_effect_end();
                if end.is_infinite() {
                    return Err(Error::InvalidState(None));
                }
                seek_time = Some(end);
            }
        }
        // Step 6 (monotonically increasing timeline: set the hold time).
        if let Some(seek_time) = seek_time {
            self.hold_time.set(Some(seek_time));
        }
        // Steps 7 to 9.
        let mut has_pending_ready_promise = false;
        if self.pending_task.get() == Some(PendingTask::Play) {
            self.pending_task.set(None);
            has_pending_ready_promise = true;
        }
        if !has_pending_ready_promise {
            self.replace_ready_promise(cx);
        }
        // Step 10. The task runs on the next animation frame.
        self.pending_task.set(Some(PendingTask::Pause));
        self.register_with_document();
        // Step 11.
        self.update_finished_state(cx, false, false);
        Ok(())
    }

    /// The pending pause task of
    /// <https://drafts.csswg.org/web-animations-1/#pause-an-animation>, step 10.
    fn run_pending_pause_task(&self, cx: &mut JSContext, ready_time: f64) {
        if let (Some(start_time), None) = (self.start_time.get(), self.hold_time.get()) {
            self.hold_time
                .set(Some((ready_time - start_time) * self.playback_rate.get()));
        }
        self.apply_any_pending_playback_rate();
        self.start_time.set(None);
        self.resolve_ready_promise(cx);
        self.update_finished_state(cx, false, false);
    }

    /// <https://drafts.csswg.org/web-animations-1/#set-the-playback-rate>
    fn set_the_playback_rate(&self, cx: &mut JSContext, new_playback_rate: f64) {
        self.pending_playback_rate.set(None);
        let previous_time = self.current_time();
        self.playback_rate.set(new_playback_rate);
        if let Some(previous_time) = previous_time {
            // Cannot fail: the seek time is resolved.
            let _ = self.set_the_current_time(cx, Some(previous_time));
        }
        self.update_finished_state(cx, true, false);
    }

    /// <https://drafts.csswg.org/web-animations-1/#seamlessly-update-the-playback-rate>
    fn seamlessly_update_the_playback_rate(
        &self,
        cx: &mut JSContext,
        new_playback_rate: f64,
    ) -> Fallible<()> {
        let previous_play_state = self.play_state();
        self.pending_playback_rate.set(Some(new_playback_rate));
        if self.pending_task.get().is_some() {
            return Ok(());
        }
        match previous_play_state {
            AnimationPlayState::Idle | AnimationPlayState::Paused => {
                self.apply_any_pending_playback_rate();
            },
            _ if self.current_time().is_none() => {
                self.apply_any_pending_playback_rate();
            },
            AnimationPlayState::Finished => {
                if let (Some(timeline_time), Some(unconstrained_current_time)) =
                    (self.timeline_time(), self.unconstrained_current_time())
                {
                    let start_time = if new_playback_rate == 0.0 {
                        timeline_time
                    } else {
                        timeline_time - unconstrained_current_time / new_playback_rate
                    };
                    self.start_time.set(Some(start_time));
                }
                self.apply_any_pending_playback_rate();
                self.update_finished_state(cx, false, false);
            },
            AnimationPlayState::Running => {
                self.play_an_animation(cx, false)?;
            },
        }
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#reverse-an-animation>
    fn reverse_an_animation(&self, cx: &mut JSContext) -> Fallible<()> {
        if self.timeline_time().is_none() {
            return Err(Error::InvalidState(None));
        }
        let original_pending_playback_rate = self.pending_playback_rate.get();
        self.pending_playback_rate
            .set(Some(-self.effective_playback_rate()));
        if let Err(error) = self.play_an_animation(cx, true) {
            self.pending_playback_rate.set(original_pending_playback_rate);
            return Err(error);
        }
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#finish-an-animation>
    fn finish_an_animation(&self, cx: &mut JSContext) -> Fallible<()> {
        let effective_rate = self.effective_playback_rate();
        let end = self.associated_effect_end();
        if effective_rate == 0.0 || (effective_rate > 0.0 && end.is_infinite()) {
            return Err(Error::InvalidState(None));
        }
        self.apply_any_pending_playback_rate();
        let rate = self.playback_rate.get();
        let limit = if rate > 0.0 { end } else { 0.0 };
        // Cannot fail: the seek time is resolved.
        let _ = self.silently_set_the_current_time(Some(limit));
        if self.start_time.get().is_none() {
            if let Some(timeline_time) = self.timeline_time() {
                self.start_time.set(Some(timeline_time - limit / rate));
            }
        }
        if self.start_time.get().is_some() {
            match self.pending_task.get() {
                Some(PendingTask::Pause) => {
                    self.hold_time.set(None);
                    self.pending_task.set(None);
                    self.resolve_ready_promise(cx);
                },
                Some(PendingTask::Play) => {
                    self.pending_task.set(None);
                    self.resolve_ready_promise(cx);
                },
                None => {},
            }
        }
        self.register_with_document();
        self.update_finished_state(cx, true, true);
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#reset-an-animations-pending-tasks>
    fn reset_pending_tasks(&self, cx: &mut JSContext) {
        if self.pending_task.take().is_none() {
            return;
        }
        self.apply_any_pending_playback_rate();
        let ready = self.ready.borrow().root(cx);
        ready.reject_error(cx, Error::Abort(None));
        ready.set_promise_is_handled(cx);
        let resolved = Promise::new_resolved(cx, &self.global(), self);
        *self.ready.borrow_mut() = resolved.to_traced();
    }

    /// <https://drafts.csswg.org/web-animations-1/#cancel-an-animation>
    fn cancel_an_animation(&self, cx: &mut JSContext) {
        if self.play_state() != AnimationPlayState::Idle {
            self.reset_pending_tasks(cx);
            let finished = self.finished.borrow().root(cx);
            finished.reject_error(cx, Error::Abort(None));
            finished.set_promise_is_handled(cx);
            self.replace_finished_promise(cx);
            let timeline_time = self.timeline_time();
            self.queue_playback_event(PlaybackEventType::Cancel, None, timeline_time, timeline_time);
        }
        self.hold_time.set(None);
        self.start_time.set(None);
    }

    /// <https://drafts.csswg.org/web-animations-1/#update-an-animations-finished-state>
    pub(crate) fn update_finished_state(
        &self,
        cx: &mut JSContext,
        did_seek: bool,
        synchronously_notify: bool,
    ) {
        // Step 1.
        let unconstrained_current_time = if did_seek {
            self.current_time()
        } else {
            self.unconstrained_current_time()
        };

        // Step 2.
        if let (Some(unconstrained_current_time), Some(_), None) = (
            unconstrained_current_time,
            self.start_time.get(),
            self.pending_task.get(),
        ) {
            let rate = self.playback_rate.get();
            let end = self.associated_effect_end();
            if rate > 0.0 && unconstrained_current_time >= end {
                let hold_time = if did_seek {
                    unconstrained_current_time
                } else {
                    self.previous_current_time
                        .get()
                        .map_or(end, |previous| previous.max(end))
                };
                self.hold_time.set(Some(hold_time));
            } else if rate < 0.0 && unconstrained_current_time <= 0.0 {
                let hold_time = if did_seek {
                    unconstrained_current_time
                } else {
                    self.previous_current_time
                        .get()
                        .map_or(0.0, |previous| previous.min(0.0))
                };
                self.hold_time.set(Some(hold_time));
            } else if rate != 0.0 {
                if let Some(timeline_time) = self.timeline_time() {
                    if did_seek {
                        if let Some(hold_time) = self.hold_time.get() {
                            self.start_time.set(Some(timeline_time - hold_time / rate));
                        }
                    }
                    self.hold_time.set(None);
                }
            }
        }

        // Step 3.
        self.previous_current_time.set(self.current_time());

        // Steps 4 and 5.
        let current_finished_state = self.play_state() == AnimationPlayState::Finished;
        if current_finished_state && self.finished.borrow().is_pending() {
            if synchronously_notify {
                self.finish_notification_queued.set(false);
                self.finish_notification_steps(cx);
            } else if !self.finish_notification_queued.replace(true) {
                self.global().enqueue_microtask(
                    cx,
                    Box::new(FinishNotificationMicrotask {
                        animation: Dom::from_ref(self),
                    }),
                );
            }
        }

        // Step 6.
        if !current_finished_state && self.finished.borrow().is_fulfilled() {
            self.replace_finished_promise(cx);
        }

        // Anything that made the animation need ticks again (a longer
        // effect from updateTiming(), a new effect or timeline, a rate
        // change on a finished animation) must put it back on the manager's
        // list; registering is idempotent.
        if self.needs_ticks() {
            self.register_with_document();
        }
    }

    /// The "finish notification steps" of
    /// <https://drafts.csswg.org/web-animations-1/#update-an-animations-finished-state>.
    fn finish_notification_steps(&self, cx: &mut JSContext) {
        if self.play_state() != AnimationPlayState::Finished {
            return;
        }
        let finished = self.finished.borrow().root(cx);
        finished.resolve_native(cx, self);
        let current_time = self.current_time();
        let timeline_time = self.timeline_time();
        let scheduled_time = self.animation_time_to_timeline_time(self.associated_effect_end());
        self.queue_playback_event(
            PlaybackEventType::Finish,
            current_time,
            timeline_time,
            scheduled_time,
        );
    }

    /// <https://drafts.csswg.org/web-animations-1/#convert-an-animation-time-to-timeline-time>
    ///
    /// The default document timeline has an origin time of zero, so this
    /// is also the origin-relative time the pending event queue sorts by.
    fn animation_time_to_timeline_time(&self, time: f64) -> Option<f64> {
        if time.is_infinite() {
            return None;
        }
        let rate = self.playback_rate.get();
        if rate == 0.0 {
            return None;
        }
        let start_time = self.start_time.get()?;
        self.timeline_time()?;
        Some(time / rate + start_time)
    }

    /// Appends a playback event to the document's pending animation event
    /// queue, or dispatches it from a task when there is no document.
    fn queue_playback_event(
        &self,
        event_type: PlaybackEventType,
        current_time: Option<f64>,
        timeline_time: Option<f64>,
        scheduled_time: Option<f64>,
    ) {
        match self.document_for_timing() {
            Some(document) => document.animation_manager().queue_script_animation_event(
                self,
                event_type,
                current_time,
                timeline_time,
                scheduled_time,
            ),
            None => {
                let animation = Trusted::new(self);
                self.global()
                    .task_manager()
                    .dom_manipulation_task_source()
                    .queue(task!(dispatch_animation_playback_event: move |cx| {
                        let animation = animation.root();
                        animation.dispatch_playback_event(cx, event_type, current_time, timeline_time);
                    }));
            },
        }
    }

    /// Fires a `finish` or `cancel` event at this animation.
    pub(crate) fn dispatch_playback_event(
        &self,
        cx: &mut JSContext,
        event_type: PlaybackEventType,
        current_time: Option<f64>,
        timeline_time: Option<f64>,
    ) {
        let global = self.global();
        let window = global.as_window();
        let init = AnimationPlaybackEventInit {
            parent: EventInit {
                bubbles: false,
                cancelable: false,
                composed: false,
            },
            currentTime: current_time.and_then(Finite::new),
            timelineTime: timeline_time.and_then(Finite::new),
        };
        let event = AnimationPlaybackEvent::new(cx, window, event_type.atom(), &init);
        event.upcast::<Event>().fire(cx, self.upcast());
    }

    /// Advances this animation for a new timeline value: runs the pending
    /// play or pause task (the timeline value is the ready time) and
    /// updates the finished state. Returns false once the manager may
    /// forget the animation: it is idle, or finished without being
    /// relevant any more.
    pub(crate) fn update_for_timeline_tick(
        &self,
        cx: &mut JSContext,
        timeline_time: Option<f64>,
    ) -> bool {
        if let (Some(ready_time), Some(task)) = (timeline_time, self.pending_task.get()) {
            self.pending_task.set(None);
            match task {
                PendingTask::Play => self.run_pending_play_task(cx, ready_time),
                PendingTask::Pause => self.run_pending_pause_task(cx, ready_time),
            }
        }
        self.update_finished_state(cx, false, false);
        match self.play_state() {
            AnimationPlayState::Idle => false,
            AnimationPlayState::Finished => self.is_relevant(),
            _ => true,
        }
    }
}

/// The microtask that runs the finish notification steps.
#[derive(JSTraceable, MallocSizeOf)]
#[cfg_attr(crown, crown::unrooted_must_root_lint::must_root)]
struct FinishNotificationMicrotask {
    animation: Dom<Animation>,
}

impl MicrotaskRunnable for FinishNotificationMicrotask {
    fn handler(&self, cx: &mut JSContext) {
        let animation = DomRoot::from_ref(&*self.animation);
        if !animation.finish_notification_queued.replace(false) {
            // A synchronous notification already ran these steps.
            return;
        }
        let global = animation.global();
        let mut realm = enter_auto_realm(cx, &*global);
        let cx = &mut realm.current_realm();
        animation.finish_notification_steps(cx);
    }
}

impl AnimationMethods<crate::DomTypeHolder> for Animation {
    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-animation>
    fn Constructor(
        cx: &mut JSContext,
        window: &Window,
        proto: Option<HandleObject>,
        effect: Option<&AnimationEffect>,
        timeline: Option<Option<&AnimationTimeline>>,
    ) -> DomRoot<Self> {
        let animation = Animation::new_with_proto(cx, window.upcast(), proto);
        match timeline {
            // An explicit null means no timeline; only a missing argument
            // defaults to the document timeline.
            Some(timeline) => animation.set_the_timeline(cx, timeline),
            None => {
                let document = window.Document();
                let timeline = document.Timeline();
                animation.set_the_timeline(cx, Some(timeline.upcast()));
            },
        }
        animation.set_the_associated_effect(cx, effect);
        animation
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-id>
    fn Id(&self) -> DOMString {
        self.id.borrow().clone()
    }

    fn SetId(&self, id: DOMString) {
        *self.id.borrow_mut() = id;
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-effect>
    fn GetEffect(&self) -> Option<DomRoot<AnimationEffect>> {
        self.associated_effect.get()
    }

    fn SetEffect(&self, cx: &mut JSContext, effect: Option<&AnimationEffect>) {
        self.set_the_associated_effect(cx, effect);
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-timeline>
    fn GetTimeline(&self) -> Option<DomRoot<AnimationTimeline>> {
        self.timeline.get()
    }

    fn SetTimeline(&self, cx: &mut JSContext, timeline: Option<&AnimationTimeline>) {
        self.set_the_timeline(cx, timeline);
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-starttime>
    fn GetStartTime(&self) -> Option<Finite<f64>> {
        self.start_time.get().and_then(Finite::new)
    }

    fn SetStartTime(&self, cx: &mut JSContext, start_time: Option<Finite<f64>>) {
        self.set_the_start_time(cx, start_time.map(|time| *time));
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-currenttime>
    fn GetCurrentTime(&self) -> Option<Finite<f64>> {
        self.current_time().and_then(Finite::new)
    }

    fn SetCurrentTime(&self, cx: &mut JSContext, current_time: Option<Finite<f64>>) -> Fallible<()> {
        self.set_the_current_time(cx, current_time.map(|time| *time))
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-playbackrate>
    fn PlaybackRate(&self) -> Finite<f64> {
        Finite::wrap(self.playback_rate.get())
    }

    fn SetPlaybackRate(&self, cx: &mut JSContext, playback_rate: Finite<f64>) {
        self.set_the_playback_rate(cx, *playback_rate);
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-playstate>
    fn PlayState(&self) -> AnimationPlayState {
        self.play_state()
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-replacestate>
    fn ReplaceState(&self) -> AnimationReplaceState {
        self.replace_state.get()
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-pending>
    fn Pending(&self) -> bool {
        self.pending_task.get().is_some()
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-ready>
    fn Ready(&self, cx: &mut JSContext) -> RootedPromise {
        self.ready.borrow().root(cx)
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-finished>
    fn Finished(&self, cx: &mut JSContext) -> RootedPromise {
        self.finished.borrow().root(cx)
    }

    // <https://drafts.csswg.org/web-animations-1/#dom-animation-onfinish>
    event_handler!(finish, GetOnfinish, SetOnfinish);

    // <https://drafts.csswg.org/web-animations-1/#dom-animation-oncancel>
    event_handler!(cancel, GetOncancel, SetOncancel);

    // <https://drafts.csswg.org/web-animations-1/#dom-animation-onremove>
    event_handler!(remove, GetOnremove, SetOnremove);

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-cancel>
    fn Cancel(&self, cx: &mut JSContext) {
        self.cancel_an_animation(cx);
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-finish>
    fn Finish(&self, cx: &mut JSContext) -> Fallible<()> {
        self.finish_an_animation(cx)
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-play>
    fn Play(&self, cx: &mut JSContext) -> Fallible<()> {
        self.play_an_animation(cx, true)
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-pause>
    fn Pause(&self, cx: &mut JSContext) -> Fallible<()> {
        self.pause_an_animation(cx)
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-updateplaybackrate>
    fn UpdatePlaybackRate(&self, cx: &mut JSContext, playback_rate: Finite<f64>) -> Fallible<()> {
        self.seamlessly_update_the_playback_rate(cx, *playback_rate)
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-animation-reverse>
    fn Reverse(&self, cx: &mut JSContext) -> Fallible<()> {
        self.reverse_an_animation(cx)
    }
}
```

Notes for the implementer, in case the compiler disagrees on a detail:

- `old_timeline.as_deref() == new_timeline` compares `Option<&T>` by
  identity through the `PartialEq` that `dom_struct` derives.
- If `Promise::new_resolved(cx, &self.global(), self)` complains that
  `&Animation` is not `ToJSValConvertible`, pass
  `DomRoot::from_ref(self)` instead.
- `task!` is in scope crate-wide (`#[macro_use]` in
  `components/script/lib.rs`); `PlaybackEventType` and the two
  `Option<f64>` are `Copy + Send`, which the task closure needs.
- `Option::is_none_or` is stable since Rust 1.82.
- The microtask and the pending event queue hold `Dom<Animation>`, so an
  animation stays alive until its finish event is delivered even after
  the manager drops it from the tick list.

**Step 2: Compile**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo check -p servo-script 2>&1 | grep -E "^(error|warning: unused)" -A 5 | head -80
```

Expected: errors only about `AnimationManager::register_script_animation`
and `queue_script_animation_event`, which Task 5 adds. Fix anything else
here.

**Step 3: Commit Tasks 3 and 4 together after Task 5 compiles** (see Task
5, Step 5).
<!-- END_TASK_4 -->

<!-- START_TASK_5 -->
### Task 5: The manager ticks script animations and queues their events

**Files:**
- Modify: `~/vibes/servo/components/script/dom/document/animation_manager.rs`
- Modify: `~/vibes/servo/components/script/dom/document/document.rs`
  (lines 5007-5031, `update_animations_and_send_events`)

**Step 1: `animation_manager.rs`, imports and fields**

Add imports. The file already imports stylo's `Animation`
(`use style::animation::{Animation, ...}` at line 27-29, used at line 565),
so the DOM one is aliased:

```rust
use std::cmp::Ordering;

use js::context::JSContext;

use crate::dom::animation::{Animation as WebAnimation, PlaybackEventType};
```

Add two fields to `AnimationManager` after `pending_events`:

```rust
    /// Script-created animations that have left the idle play state. They
    /// are ticked every frame and dropped once idle, or finished and no
    /// longer relevant.
    script_animations: DomRefCell<Vec<Dom<WebAnimation>>>,

    /// Finish and cancel events waiting for the next "update animations
    /// and send events".
    ///
    /// <https://drafts.csswg.org/web-animations-1/#pending-animation-event-queue>
    pending_script_events: DomRefCell<Vec<ScriptAnimationEvent>>,
```

Initialise both with `Default::default()` in `new()`, and clear both in
`clear()`:

```rust
        self.script_animations.borrow_mut().clear();
        self.pending_script_events.borrow_mut().clear();
```

Add the event struct after `TransitionOrAnimationEvent` (end of file):

```rust
/// A `finish` or `cancel` event for a script animation, with the values
/// captured when it was queued.
#[derive(JSTraceable, MallocSizeOf)]
#[cfg_attr(crown, crown::unrooted_must_root_lint::must_root)]
pub(crate) struct ScriptAnimationEvent {
    animation: Dom<WebAnimation>,
    event_type: PlaybackEventType,
    current_time: Option<f64>,
    timeline_time: Option<f64>,
    /// <https://drafts.csswg.org/web-animations-1/#scheduled-event-time>
    scheduled_time: Option<f64>,
}
```

**Step 2: New methods on `impl AnimationManager`**

Add after `running_animation_count` (line 356):

```rust
    /// Starts ticking a script animation. Idempotent.
    pub(crate) fn register_script_animation(&self, animation: &WebAnimation) {
        let mut animations = self.script_animations.borrow_mut();
        if !animations.iter().any(|existing| **existing == *animation) {
            animations.push(Dom::from_ref(animation));
        }
    }

    /// Appends to the pending animation event queue; dispatched by
    /// `send_pending_events`.
    pub(crate) fn queue_script_animation_event(
        &self,
        animation: &WebAnimation,
        event_type: PlaybackEventType,
        current_time: Option<f64>,
        timeline_time: Option<f64>,
        scheduled_time: Option<f64>,
    ) {
        self.pending_script_events
            .borrow_mut()
            .push(ScriptAnimationEvent {
                animation: Dom::from_ref(animation),
                event_type,
                current_time,
                timeline_time,
                scheduled_time,
            });
    }

    /// Step 1 of "update animations and send events" for script
    /// animations: runs pending play and pause tasks with `timeline_time`
    /// (milliseconds) as the ready time and updates finished states.
    pub(crate) fn update_script_animations(
        &self,
        cx: &mut JSContext,
        window: &Window,
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
            if !animation.update_for_timeline_tick(cx, timeline_time) {
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
            window,
            running_css_animations || self.has_running_script_animations(),
        );
    }

    fn has_running_script_animations(&self) -> bool {
        self.script_animations
            .borrow()
            .iter()
            .any(|animation| animation.needs_ticks())
    }
```

Change `running_animation_count` to include script animations:

```rust
    pub(crate) fn running_animation_count(&self) -> usize {
        let css = self
            .sets
            .sets
            .read()
            .values()
            .map(|state| state.running_animation_and_transition_count())
            .sum::<usize>();
        let script = self
            .script_animations
            .borrow()
            .iter()
            .filter(|animation| animation.needs_ticks())
            .count();
        css + script
    }
```

In `do_post_reflow_update` (line 225) change

```rust
        let have_running_animations = sets.values().any(|state| state.needs_animation_ticks());
```

to

```rust
        let have_running_animations = sets.values().any(|state| state.needs_animation_ticks()) ||
            self.has_running_script_animations();
```

In `handle_animation_presence_or_pending_events_change` (line 334) change
the `has_pending_events` line to

```rust
        let has_pending_events = !self.pending_events.borrow().is_empty() ||
            !self.pending_script_events.borrow().is_empty();
```

**Step 3: `send_pending_events`**

Replace the beginning of the method, lines 613-622 (the signature, the
`take` of `pending_events`, and the whole `if events.is_empty() { return; }`
early return), with:

```rust
    pub(crate) fn send_pending_events(&self, window: &Window, cx: &mut js::context::JSContext) {
        let events = std::mem::take(&mut *self.pending_events.safe_borrow_mut(cx.no_gc()));
        let mut script_events = std::mem::take(&mut *self.pending_script_events.borrow_mut());
        if events.is_empty() && script_events.is_empty() {
            return;
        }
```

(The old early return must go: it would run after the script events were
taken and drop them whenever no CSS event is pending.)

After the `for event in events.into_iter() { ... }` loop and before the
final presence check, add:

```rust
        // Steps 6 and 7 of "update animations and send events": a stable
        // sort by scheduled event time, unresolved first, then dispatch.
        // (Composite order only matters between events with equal times,
        // which queue order already provides for script animations.)
        script_events.sort_by(|a, b| match (a.scheduled_time, b.scheduled_time) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        });
        for event in script_events {
            let animation = DomRoot::from_ref(&*event.animation);
            animation.dispatch_playback_event(
                cx,
                event.event_type,
                event.current_time,
                event.timeline_time,
            );
        }
```

and change the final check to

```rust
        if self.pending_events.borrow().is_empty() &&
            self.pending_script_events.borrow().is_empty()
        {
            self.handle_animation_presence_or_pending_events_change(window);
        }
```

**Step 4: `document.rs`**

Add the import

```rust
use crate::dom::bindings::codegen::Bindings::AnimationTimelineBinding::AnimationTimelineMethods;
```

In `update_animations_and_send_events`, after the
`self.animation_manager.update_for_new_timeline_value(...)` call and before
`maybe_mark_animating_nodes_as_dirty`, add:

```rust
        // Script animations: pending play and pause tasks become ready at
        // this timeline value, and finished states update (step 1 as well).
        let timeline_time = self
            .timeline
            .upcast::<AnimationTimeline>()
            .GetCurrentTime()
            .map(|time| *time);
        self.animation_manager
            .update_script_animations(cx, &self.window, timeline_time);
```

**Step 5: Compile and commit Tasks 3, 4 and 5**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo check -p servo-script 2>&1 | tail -3
UV_NO_SYNC=1 cargo test -p servo-script --lib animations::timing 2>&1 | tail -3
cargo fmt -p servo-script
git add components/script_bindings/webidls/Animation.webidl components/script_bindings/codegen/Bindings.conf components/script/dom/animations/animation.rs components/script/dom/animations/animationeffect.rs components/script/dom/document/animation_manager.rs components/script/dom/document/document.rs
git commit -m "script: implement the Animation timing model

Animation gains start time, hold time, playback rate, pending playback
rate, pending play and pause tasks, the ready and finished promises, id,
replaceState and the play, pause, cancel, finish, reverse,
updatePlaybackRate, currentTime and startTime procedures of Web
Animations Level 1 section 4.5. AnimationEffect::getComputedTiming()
reports real values. The document's AnimationManager ticks script
animations from update_animations_and_send_events, counts them as running
animations so the frame loop keeps going, and holds the pending animation
event queue for finish and cancel events."
```

Expected: `Finished`, `12 passed`, and a clean commit.
<!-- END_TASK_5 -->
<!-- END_SUBCOMPONENT_B -->

<!-- START_SUBCOMPONENT_C (tasks 6-8) -->
<!-- START_TASK_6 -->
### Task 6: `Element.animate()` sets the id and plays

**Files:**
- Modify: `~/vibes/servo/components/script_bindings/webidls/Animatable.webidl`
- Modify: `~/vibes/servo/components/script/dom/element/element.rs:4595-4659`
  (the whole `Animate` method including its closing brace at 4659)

**Step 1: The IDL**

Change the `animate` declaration to

```webidl
    [Pref="dom_web_animations_enabled", Throws]
    Animation           animate(object? keyframes ,
                                optional (unrestricted double or KeyframeAnimationOptions) options = {});
```

**Step 2: `element.rs`**

Add the imports

```rust
use crate::dom::animationeffect::AnimationEffect;
use crate::dom::animations::animationeffect::{
    optional_effect_timing, optional_effect_timing_for_duration,
};
```

and replace the `Animate` method (lines 4595-4659, through its closing
brace) with:

```rust
    /// <https://drafts.csswg.org/web-animations-1/#dom-animatable-animate>
    fn Animate(
        &self,
        cx: &mut JSContext,
        keyframes: *mut JSObject,
        options: UnrestrictedDoubleOrKeyframeAnimationOptions,
    ) -> Fallible<DomRoot<Animation>> {
        let window = self.owner_window();

        // Step 1. Let target be the object on which this method was called.
        let target = self;

        // Step 3. If options is a KeyframeAnimationOptions object, let timeline
        // be the timeline member of options or, if the timeline member of
        // options is missing, be the default document timeline of the node
        // document of the element on which this method was called.
        // (Step 5 reads the id from the same object, and the timing input of
        // the KeyframeEffect constructor comes from it too.)
        let (timeline, id, timing_input) = match &options {
            UnrestrictedDoubleOrKeyframeAnimationOptions::KeyframeAnimationOptions(options) => (
                options.timeline.clone().flatten(),
                Some(options.id.clone()),
                optional_effect_timing(&options.parent.parent),
            ),
            UnrestrictedDoubleOrKeyframeAnimationOptions::UnrestrictedDouble(duration) => {
                (None, None, optional_effect_timing_for_duration(*duration))
            },
        };
        let timeline = timeline.unwrap_or_else(|| {
            let document = self.owner_document();
            DomRoot::upcast(document.Timeline())
        });

        // Step 2. Construct a new KeyframeEffect object, effect, in the relevant
        // Realm of target by using the same procedure as the KeyframeEffect(target,
        // keyframes, options) constructor, passing target as the target argument,
        // and the keyframes and options arguments as supplied.
        let parent_options = match options {
            UnrestrictedDoubleOrKeyframeAnimationOptions::UnrestrictedDouble(value) => {
                UnrestrictedDoubleOrKeyframeEffectOptions::UnrestrictedDouble(value)
            },
            UnrestrictedDoubleOrKeyframeAnimationOptions::KeyframeAnimationOptions(options) => {
                UnrestrictedDoubleOrKeyframeEffectOptions::KeyframeEffectOptions(options.parent)
            },
        };
        let effect =
            KeyframeEffect::Constructor(cx, &window, None, Some(target), keyframes, parent_options);
        // The KeyframeEffect constructor does not apply its options yet (a
        // later patch moves this into it): apply the timing input here so
        // the animation has its duration, iterations and fill.
        effect
            .upcast::<AnimationEffect>()
            .update_the_timing_properties(cx, &timing_input)?;

        // Step 4. Construct a new Animation object, animation, in the relevant
        // Realm of target by using the same procedure as the Animation() constructor,
        // passing effect and timeline as arguments of the same name.
        let animation = Animation::Constructor(
            cx,
            &window,
            None,
            Some(effect.upcast()),
            Some(Some(&timeline)),
        );

        // Step 5. If options is a KeyframeAnimationOptions object, assign the
        // value of the id member of options to animation's id attribute.
        if let Some(id) = id {
            animation.SetId(id);
        }

        // Step 6. Run the procedure to play an animation for animation with
        // the auto-rewind flag set to true.
        animation.play_an_animation(cx, true)?;

        // Step 7. Return animation.
        Ok(animation)
    }
```

`Fallible` is already imported in `element.rs`; `AnimationMethods` (for
`SetId`) is imported at line 38.

**Step 3: Compile, format, commit**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo check -p servo-script 2>&1 | tail -3
cargo fmt -p servo-script
git add components/script_bindings/webidls/Animatable.webidl components/script/dom/element/element.rs
git commit -m "script: finish Element.animate()

Apply the timing options to the effect, assign the id from
KeyframeAnimationOptions and play the animation with auto-rewind
(https://drafts.csswg.org/web-animations-1/#dom-animatable-animate steps 2,
5 and 6). An explicit null timeline is respected as such."
```
<!-- END_TASK_6 -->

<!-- START_TASK_7 -->
### Task 7: Paladin enables the pref and gains the page harness

**Files:**
- Modify: `crates/paladin-engine/src/engine.rs:69-75`
- Create: `crates/paladin-engine/tests/web_animations.rs`
- Create: `crates/paladin-engine/tests/pages/finish-event.html`
- Create: `crates/paladin-engine/tests/pages/finished-promise.html`
- Create: `crates/paladin-engine/tests/pages/cancel-rejects.html`
- Create: `crates/paladin-engine/tests/pages/pause-holds-current-time.html`
- Create: `crates/paladin-engine/tests/pages/playback-rate-scales.html`

**Step 1: Write the harness (fails until the fork is in use)**

`crates/paladin-engine/tests/web_animations.rs`:

```rust
//! Loads the Web Animations test pages in `tests/pages/` through Servo
//! without a window. Each page reports through `document.title`: `PASS`,
//! or `FAIL: <reason>`. One test function on purpose: Servo's options are
//! process-global, so a test binary may build exactly one engine.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use paladin_core::{EngineEvent, TabId};
use paladin_engine::{Engine, PhysicalSize, TabHost, Waker};

struct NoopWaker;

impl Waker for NoopWaker {
    fn wake(&self) {}
}

/// Every page in `tests/pages/`, run in this order.
const PAGES: &[&str] = &[
    "finish-event.html",
    "finished-promise.html",
    "cancel-rejects.html",
    "pause-holds-current-time.html",
    "playback-rate-scales.html",
];

/// How long one page may take to report; the first page also pays for
/// engine start-up.
const PAGE_TIMEOUT: Duration = Duration::from_secs(120);

#[test]
fn web_animations_pages_pass() {
    let dir = tempfile::tempdir().expect("temp dir");
    let engine =
        Engine::new(&dir.path().join("servo"), Arc::new(NoopWaker)).expect("engine starts");
    let mut host =
        TabHost::new_software(&engine, PhysicalSize::new(400, 300)).expect("software host");
    let pages = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages");

    let mut failures = Vec::new();
    for (index, page) in PAGES.iter().enumerate() {
        let tab = TabId(index as u64 + 1);
        let url = url::Url::from_file_path(pages.join(page)).expect("absolute page path");
        host.create_tab(tab, url);

        let deadline = Instant::now() + PAGE_TIMEOUT;
        let verdict = loop {
            assert!(Instant::now() < deadline, "{page}: no verdict within {PAGE_TIMEOUT:?}");
            engine.spin();
            let mut verdict = None;
            for event in engine.drain_events() {
                match event {
                    EngineEvent::TitleChanged { tab: t, title }
                        if t == tab && (title == "PASS" || title.starts_with("FAIL")) =>
                    {
                        verdict = Some(title);
                    }
                    EngineEvent::FrameReady(t) if t == tab => host.paint(t),
                    _ => {}
                }
            }
            if let Some(verdict) = verdict {
                break verdict;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        if verdict != "PASS" {
            failures.push(format!("{page}: {verdict}"));
        }
        host.close_tab(tab);
    }

    assert!(host.is_empty());
    drop(host);
    // Dropping the engine shuts Servo down and waits for it to finish.
    drop(engine);
    assert!(failures.is_empty(), "failing pages:\n{}", failures.join("\n"));
}
```

**Step 2: The pages**

Every page follows the same shape: a `verdict` helper that sets the title
once, an `error` listener, and a 5 s watchdog. Create these five files.

`crates/paladin-engine/tests/pages/finish-event.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>finish-event</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('no finish within 5s'), 5000);

const anim = document.getElementById('box').animate(
  [{ opacity: 1 }, { opacity: 0 }], 100);
if (anim.playState !== 'running') fail('playState after animate() is ' + anim.playState);
if (!anim.pending) fail('animation is not pending right after animate()');
anim.onfinish = (event) => {
  if (anim.playState !== 'finished') return fail('playState in onfinish is ' + anim.playState);
  if (anim.currentTime !== 100) return fail('currentTime after finish is ' + anim.currentTime);
  if (event.currentTime !== 100) return fail('event.currentTime is ' + event.currentTime);
  if (typeof event.timelineTime !== 'number') return fail('event.timelineTime is ' + event.timelineTime);
  if (event.target !== anim) return fail('event.target is not the animation');
  verdict('PASS');
};
</script>
```

`crates/paladin-engine/tests/pages/finished-promise.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>finished-promise</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('no finish within 5s'), 5000);

(async () => {
  const anim = document.getElementById('box').animate(
    [{ opacity: 1 }, { opacity: 0 }], 100);
  const ready = await anim.ready;
  if (ready !== anim) return fail('ready resolved with ' + ready);
  if (anim.pending) return fail('still pending after ready');
  if (typeof anim.startTime !== 'number') return fail('startTime after ready is ' + anim.startTime);
  const finished = await anim.finished;
  if (finished !== anim) return fail('finished resolved with ' + finished);
  if (anim.playState !== 'finished') return fail('playState after finished is ' + anim.playState);
  if (anim.currentTime !== 100) return fail('currentTime after finished is ' + anim.currentTime);
  // Replaying replaces the finished promise.
  const before = anim.finished;
  anim.play();
  if (anim.finished === before) return fail('finished promise was not replaced by play()');
  if (anim.playState !== 'running') return fail('playState after replay is ' + anim.playState);
  await anim.finished;
  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

`crates/paladin-engine/tests/pages/cancel-rejects.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>cancel-rejects</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('no cancel within 5s'), 5000);

(async () => {
  const anim = document.getElementById('box').animate(
    [{ opacity: 1 }, { opacity: 0 }], 10000);
  const finished = anim.finished;
  const ready = anim.ready;
  let cancelEvent = null;
  anim.oncancel = (event) => { cancelEvent = event; };
  anim.cancel();
  if (anim.playState !== 'idle') return fail('playState after cancel is ' + anim.playState);
  if (anim.currentTime !== null) return fail('currentTime after cancel is ' + anim.currentTime);
  if (anim.startTime !== null) return fail('startTime after cancel is ' + anim.startTime);
  if (anim.finished === finished) return fail('finished promise was not replaced');
  try {
    await finished;
    return fail('old finished promise resolved');
  } catch (e) {
    if (e.name !== 'AbortError') return fail('finished rejected with ' + e.name);
  }
  try {
    await ready;
    return fail('pending ready promise resolved after cancel');
  } catch (e) {
    if (e.name !== 'AbortError') return fail('ready rejected with ' + e.name);
  }
  if ((await anim.ready) !== anim) return fail('new ready promise is not resolved');
  // The cancel event is delivered on the next animation frame.
  for (let i = 0; i < 20 && !cancelEvent; i++) {
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
  if (!cancelEvent) return fail('no cancel event');
  if (cancelEvent.currentTime !== null) return fail('cancel event currentTime is ' + cancelEvent.currentTime);
  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

`crates/paladin-engine/tests/pages/pause-holds-current-time.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>pause-holds-current-time</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('did not settle within 5s'), 5000);

(async () => {
  const anim = document.getElementById('box').animate(
    [{ opacity: 1 }, { opacity: 0 }], 10000);
  await anim.ready;
  anim.pause();
  if (anim.playState !== 'paused') return fail('playState right after pause() is ' + anim.playState);
  if (!anim.pending) return fail('not pending after pause()');
  await anim.ready;
  if (anim.pending) return fail('still pending after the pause task');
  const held = anim.currentTime;
  if (typeof held !== 'number') return fail('currentTime while paused is ' + held);
  if (anim.startTime !== null) return fail('startTime while paused is ' + anim.startTime);
  await new Promise((resolve) => setTimeout(resolve, 150));
  if (anim.currentTime !== held) return fail('currentTime moved while paused: ' + held + ' -> ' + anim.currentTime);
  anim.play();
  await anim.ready;
  await new Promise((resolve) => setTimeout(resolve, 100));
  if (anim.currentTime <= held) return fail('currentTime did not advance after play()');
  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

`crates/paladin-engine/tests/pages/playback-rate-scales.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>playback-rate-scales</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('did not settle within 5s'), 5000);

(async () => {
  const anim = document.getElementById('box').animate(
    [{ opacity: 1 }, { opacity: 0 }], 2000);
  anim.playbackRate = 8;
  if (anim.playbackRate !== 8) return fail('playbackRate is ' + anim.playbackRate);
  const started = performance.now();
  await anim.finished;
  const elapsed = performance.now() - started;
  if (elapsed > 1000) return fail('finished after ' + elapsed + 'ms at 8x');
  if (anim.currentTime !== 2000) return fail('currentTime after finish is ' + anim.currentTime);
  // Reversing from the end plays back to zero.
  anim.reverse();
  if (anim.playState !== 'running') return fail('playState after reverse() is ' + anim.playState);
  await anim.finished;
  if (anim.currentTime !== 0) return fail('currentTime after reversed finish is ' + anim.currentTime);
  if (anim.playbackRate !== -8) return fail('playbackRate after reverse is ' + anim.playbackRate);
  // updatePlaybackRate() applies on the next frame without a jump.
  anim.updatePlaybackRate(1);
  await anim.ready;
  if (anim.playbackRate !== 1) return fail('playbackRate after updatePlaybackRate is ' + anim.playbackRate);
  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

**Step 3: Enable the pref**

In `crates/paladin-engine/src/engine.rs` replace line 72
(`.preferences(Preferences::default())`) with:

```rust
                .preferences(Preferences {
                    // Web Animations: implemented in Paladin's Servo fork.
                    dom_web_animations_enabled: true,
                    ..Preferences::default()
                })
```

**Step 4: Run the harness against the fork**

With `~/.cargo/config.toml` pointing `servo` at `~/vibes/servo` (see
"Working in the fork" above):

```bash
cd /Users/thomas/vibes/paladin
cargo test -p paladin-engine --test web_animations 2>&1 | tail -15
```

Expected: `test web_animations_pages_pass ... ok`. The first run rebuilds
every Servo crate from the path source. On failure the assertion lists the
failing pages with their `FAIL: ...` reasons; fix the fork code, rerun. The
old headless test must still pass:

```bash
cargo test -p paladin-engine --test headless 2>&1 | tail -3
```

**Step 5: Do not commit yet** (the lock currently has path sources). Task
8 publishes the fork and commits Paladin.
<!-- END_TASK_7 -->

<!-- START_TASK_8 -->
### Task 8: Publish the fork and pin Paladin to it

**Files:**
- Modify: `Cargo.lock`
- Modify: `CHANGELOG.md`
- Modify: `docs/upstream-issues.md` ("Carried in the fork")

**Step 1: Push the dev branch and publish dist**

```bash
cd ~/vibes/servo
git log --oneline v0.7.0..HEAD
git push origin paladin/0.7.0
etc/paladin/publish-dist.sh dist/paladin-0.7.0 | tee /tmp/servo-dist-sha
```

Expected: five commits above `v0.7.0` (publisher, timing, playback event,
timing model, animate) and a new dist sha.

**Step 2: Switch Paladin back to the dist branch**

```bash
rm ~/.cargo/config.toml          # or delete the [patch.crates-io] table
cd /Users/thomas/vibes/paladin
git checkout Cargo.lock
cargo update -p servo --precise $(cat /tmp/servo-dist-sha) 2>&1 | tail -3
grep -c "$(cat /tmp/servo-dist-sha)" Cargo.lock
```

Expected: cargo reports updating the servo crates, and the count is `60`.

**Step 3: Full verification against the dist source**

```bash
cargo test --workspace 2>&1 | grep -E '^test result|FAILED|panicked'
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -2
cargo fmt --all --check && echo fmt-ok
```

Expected: all `ok`, including `web_animations_pages_pass`; `fmt-ok`.

**Step 4: Docs**

`CHANGELOG.md`, under `### Added`, first bullet:

```markdown
- Web Animations: `element.animate()` now plays its animation, and
  `Animation` implements the timing model (`play`, `pause`, `cancel`,
  `finish`, `reverse`, `playbackRate`, `updatePlaybackRate`, `currentTime`,
  `startTime`, `playState`, `pending`, `ready`, `finished`, `onfinish`,
  `oncancel`). Animations do not change rendered style yet. Carried as a
  patch in Paladin's Servo fork.
```

`docs/upstream-issues.md`, replace `None yet.` under "Carried in the fork"
with:

```markdown
- **S13 (Web Animations API), partial.** `thomasmost/servo`
  `paladin/0.7.0`: "script: add the Web Animations timing calculations",
  "script: add AnimationPlaybackEvent", "script: implement the Animation
  timing model", "script: finish Element.animate()". Timing model only;
  sampling follows in later patches. Upstream: servo/servo#36950.
```

**Step 5: Commit**

```bash
git add Cargo.lock CHANGELOG.md docs/upstream-issues.md crates/paladin-engine/src/engine.rs crates/paladin-engine/tests/web_animations.rs crates/paladin-engine/tests/pages
git commit -m "feat(engine): Web Animations timing model from the Servo fork

Enable dom_web_animations_enabled and pin the fork's dist branch that
implements Animation playback. Adds the headless page harness and its
first five pages."
git push origin vnext
```

Expected: the push succeeds. Pushes to `vnext` do not run CI (the Phase 1
pull request was closed or merged by the user); the full CI run happens
from the pull request Phase 6 opens.
<!-- END_TASK_8 -->
<!-- END_SUBCOMPONENT_C -->

## Phase 2 done when

- `cargo test -p servo-script --lib animations::timing` passes in the fork
  (Task 1).
- `cargo test --workspace` in Paladin is green against the published dist
  commit, including the five pages (Task 8).
- Clippy and fmt are clean; CHANGELOG and upstream-issues updated (Task 8).
