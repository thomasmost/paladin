# Servo Fork and Web Animations Implementation Plan

**Goal:** Every input `animate()` and `new KeyframeEffect()` accept is
parsed, validated and retained: timing options, `composite`, `target`,
array keyframes with offsets, easings and composites, property-indexed
keyframes, computed offsets, and the spec's `TypeError`s.

**Architecture:** `KeyframeEffect` keeps its keyframes as a `Vec<Keyframe>`
with a nullable specified offset, a parsed keyframe easing, a composite and
the parsed declarations. `process_a_keyframes_argument` gains the
property-indexed branch (implemented with the JS API directly, so no
WebIDL union support is needed), the loosely-sorted and range checks and
the easing parse; `compute_missing_keyframe_offsets` is a pure, unit-tested
function used by `getKeyframes()`. The constructor applies the timing
options through the existing `update_the_timing_properties` and becomes
`[Throws]`.

**Tech Stack:** Servo 0.7.0 `servo-script` (`components/script/dom/
animations/keyframeeffect.rs`), `servo-script-bindings` WebIDL, SpiderMonkey
`for_of` iteration.

**Scope:** 6 phases from the design plan; this file is Phase 3 of 6.

**Codebase verified:** 2026-10-15 against
`servo-script-0.7.0/dom/animations/keyframeeffect.rs` (581 lines, read in
full) and `KeyframeEffect.webidl`.

---

## Facts from verification that shape this phase

- In 0.7.0 `KeyframeEffect::Constructor` ignores `options` entirely (steps
  3 to 6 are TODO comments). Phase 2 worked around that by giving
  `AnimationEffect` the spec defaults and applying the timing input from
  `Element::Animate`; this phase moves the option handling into the
  constructor where it belongs. Still untouched from 0.7.0: `SetKeyframes`
  swallows errors, property-indexed keyframes return `Error::Operation`,
  and `getKeyframes()` reports `computedOffset = offset`.
- The keyframe type is private: `Keyframe { offset: Option<Finite<f64>>,
  easing_function: DOMString, composite: CompositeOperationOrAuto,
  declarations: Vec<KeyframePropertyDeclaration> }`, where each
  `KeyframePropertyDeclaration { property_id: PropertyId, block:
  PropertyDeclarationBlock }` holds the parsed value of one property (a
  shorthand expands to several longhands inside the block).
- Property reading already diverges from the spec in a sound way: it walks
  the object's own keys and keeps those that name an animatable property
  (`lookup_css_property_by_idl_attribute_name`). This phase keeps that and
  adds list support.
- **Deviation from the design:** the design restores the
  `BasePropertyIndexedKeyframe` dictionary in the IDL. Its members are
  unions of a nullable scalar and a sequence
  (`(double? or sequence<double?>)`), which Servo's codegen left commented
  out upstream. Rather than risk the codegen, this phase reads `offset`,
  `easing` and `composite` from the property-indexed object with the JS
  API and the generated `FromJSValConvertible` impls, which is what the
  spec's conversion amounts to. The dictionary stays commented out.
- `pseudoElement` and the copy constructor stay out (design: pseudo-element
  targets are out of scope).
- `Phase 2` made `update_the_timing_properties` take `cx` and
  `Element::Animate` call `KeyframeEffect::Constructor`; that call site
  gains a `?` here.

Working in the fork, commit conventions and the local path-patch loop are
as described at the top of `phase_02.md`.

---

<!-- START_SUBCOMPONENT_A (tasks 1-3) -->
<!-- START_TASK_1 -->
### Task 1: `compute_missing_keyframe_offsets`

**Files:**
- Modify: `~/vibes/servo/components/script/dom/animations/keyframeeffect.rs`
  (append the function and its tests; the rest of the file is rewritten
  in Task 2, which also changes the IDL so the old code keeps compiling
  here)

**Step 1: (none; the IDL changes in Task 2)**

**Step 2: Write the failing tests**

Append to `keyframeeffect.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::compute_missing_keyframe_offsets;

    #[test]
    fn spreads_null_offsets_evenly_between_their_neighbours() {
        let offsets = [None, Some(0.5), None, Some(0.8), None];
        assert_eq!(
            compute_missing_keyframe_offsets(&offsets),
            vec![0.0, 0.5, 0.65, 0.8, 1.0]
        );
    }

    #[test]
    fn a_single_keyframe_sits_at_the_end() {
        assert_eq!(compute_missing_keyframe_offsets(&[None]), vec![1.0]);
        assert_eq!(compute_missing_keyframe_offsets(&[Some(0.25)]), vec![0.25]);
    }

    #[test]
    fn two_and_three_null_keyframes() {
        assert_eq!(compute_missing_keyframe_offsets(&[None, None]), vec![0.0, 1.0]);
        assert_eq!(
            compute_missing_keyframe_offsets(&[None, None, None]),
            vec![0.0, 0.5, 1.0]
        );
    }

    #[test]
    fn explicit_endpoints_are_kept() {
        assert_eq!(
            compute_missing_keyframe_offsets(&[Some(0.2), None, Some(0.6)]),
            vec![0.2, 0.4, 0.6]
        );
        assert_eq!(compute_missing_keyframe_offsets(&[Some(0.2), None]), vec![0.2, 1.0]);
    }

    #[test]
    fn no_keyframes_no_offsets() {
        assert!(compute_missing_keyframe_offsets(&[]).is_empty());
    }
}
```

**Step 3: Run them to see them fail**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo test -p servo-script --lib keyframeeffect::tests 2>&1 | grep -E "^error" -A 3 | head
```

Expected: `error[E0432]: unresolved import` for
`compute_missing_keyframe_offsets`.

**Step 4: Implement the function**

Add above the `tests` module:

```rust
/// <https://drafts.csswg.org/web-animations-1/#compute-missing-keyframe-offsets>
///
/// Returns one computed offset per input keyframe.
pub(crate) fn compute_missing_keyframe_offsets(offsets: &[Option<f64>]) -> Vec<f64> {
    // Step 1. Start from the specified offsets.
    let mut computed: Vec<Option<f64>> = offsets.to_vec();
    let count = computed.len();

    // Step 2. More than one keyframe and the first has no offset: 0.
    if count > 1 && computed[0].is_none() {
        computed[0] = Some(0.0);
    }

    // Step 3. The last keyframe has no offset: 1.
    if let Some(last) = computed.last_mut() {
        if last.is_none() {
            *last = Some(1.0);
        }
    }

    // Step 4. Spread the gaps between consecutive resolved offsets evenly.
    let mut a = 0;
    while a < count {
        let Some(offset_a) = computed[a] else {
            a += 1;
            continue;
        };
        let Some(b) = (a + 1..count).find(|&index| computed[index].is_some()) else {
            break;
        };
        let offset_b = computed[b].expect("found above");
        let n = (b - a) as f64;
        for k in a + 1..b {
            computed[k] = Some(offset_a + (offset_b - offset_a) * (k - a) as f64 / n);
        }
        a = b;
    }

    computed
        .into_iter()
        .map(|offset| offset.unwrap_or(0.0))
        .collect()
}
```

**Step 5: Run the tests**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo test -p servo-script --lib keyframeeffect::tests 2>&1 | tail -3
```

Expected: `test result: ok. 5 passed` (a dead-code warning for the function
until Task 2 uses it).

**Step 6: Commit with Task 2.**
<!-- END_TASK_1 -->

<!-- START_TASK_2 -->
### Task 2: `KeyframeEffect` processes everything `animate()` accepts

**Files:**
- Modify: `~/vibes/servo/components/script_bindings/webidls/KeyframeEffect.webidl`
- Modify: `~/vibes/servo/components/script/dom/animations/keyframeeffect.rs`
  (replace from the top of the file through the end of the original
  `get_property_declarations` function; keep the three helpers that follow
  it and the `compute_missing_keyframe_offsets` function and tests from
  Task 1)
- Modify: `~/vibes/servo/components/script/dom/element/element.rs`
  (the `KeyframeEffect::Constructor` call in `Animate`)

**Step 0: The IDL**

Replace the interface block in `KeyframeEffect.webidl` with:

```webidl
[Exposed=Window, Pref="dom_web_animations_enabled"]
interface KeyframeEffect : AnimationEffect {
  [Throws] constructor(Element?       target,
                       object?        keyframes,
                       optional (unrestricted double or KeyframeEffectOptions) options = {});
  // constructor(KeyframeEffect source);
  attribute Element?           target;
  // attribute CSSOMString?       pseudoElement;
  attribute CompositeOperation composite;
  [Throws] sequence<object> getKeyframes();
  [Throws] undefined        setKeyframes(object? keyframes);
};
```

Leave the dictionaries and enums below it unchanged (including the
commented-out `BasePropertyIndexedKeyframe` members).

**Step 1: Rewrite the top of `keyframeeffect.rs`**

Replace the file from its first line through the closing brace of the
original `get_property_declarations` (0.7.0 lines 1-488) with the code
below. Everything after that point stays: `parse_single_property_declaration`,
`lookup_css_property_by_idl_attribute_name`,
`animation_property_name_to_idl_attribute_name`, and Task 1's
`compute_missing_keyframe_offsets` with its tests.

```rust
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::Cell;
use std::cmp::Ordering;
use std::ffi::{CStr, CString};
use std::ops::ControlFlow;
use std::ptr::{self, NonNull};
use std::sync::LazyLock;

use cssparser::Parser;
use dom_struct::dom_struct;
use js::context::JSContext;
use js::conversions::{
    ConversionResult, FromJSValConvertible, ToJSValConvertible, jsstr_to_string,
};
use js::gc::{HandleValue, RootedVec};
use js::jsapi::{
    HandleId, Heap, JS_GetProperty, JS_GetPropertyById, JSITER_OWNONLY, JSObject,
    JSPROP_ENUMERATE,
};
use js::jsval::{ObjectValue, UndefinedValue};
use js::rust::wrappers2::{GetPropertyKeys, JS_DefineProperty, JS_IdToValue, JS_NewObject};
use js::rust::{
    ForOfIterationFailure, HandleObject, IdVector, IntoHandle, IntoMutableHandle, for_of,
};
use rustc_hash::FxHashMap;
use script_bindings::cell::DomRefCell;
use script_bindings::codegen::GenericBindings::KeyframeEffectBinding::{
    BaseKeyframe, CompositeOperation, CompositeOperationOrAuto,
};
use script_bindings::codegen::GenericBindings::WindowBinding::WindowMethods;
use script_bindings::codegen::GenericUnionTypes::UnrestrictedDoubleOrKeyframeEffectOptions;
use script_bindings::conversions::StringificationBehavior;
use script_bindings::error::{Error, Fallible};
use script_bindings::inheritance::Castable;
use script_bindings::num::Finite;
use script_bindings::reflector::reflect_dom_object_with_proto;
use script_bindings::root::DomRoot;
use script_bindings::str::DOMString;
use style::parser::{Parse, ParserContext};
use style::properties::generated::PropertyDeclaration;
use style::properties::{
    Importance, LonghandId, NonCustomPropertyId, PropertyDeclarationBlock, PropertyId,
    SourcePropertyDeclaration,
};
use style::stylesheets::CssRuleType;
use style::values::specified::TimingFunction;
use style_traits::{CssWriter, ParsingMode, ToCss};

use crate::css::css::parser_context_for_document;
use crate::dom::Document;
use crate::dom::animationeffect::AnimationEffect;
use crate::dom::animations::animationeffect::{
    optional_effect_timing, optional_effect_timing_for_duration,
};
use crate::dom::bindings::codegen::Bindings::KeyframeEffectBinding::{
    BaseComputedKeyframe, KeyframeEffectMethods,
};
use crate::dom::bindings::root::MutNullableDom;
use crate::dom::element::Element;
use crate::dom::window::Window;

/// <https://drafts.csswg.org/web-animations-1/#keyframeeffect>
#[dom_struct]
pub(crate) struct KeyframeEffect {
    animationeffect: AnimationEffect,

    /// <https://drafts.csswg.org/web-animations-1/#effect-target-target-element>
    ///
    /// Pseudo-element targets are not supported: the target pseudo-selector
    /// is always null.
    target_element: MutNullableDom<Element>,

    /// <https://drafts.csswg.org/web-animations-1/#effect-composite-operation>
    composite: Cell<CompositeOperation>,

    /// <https://drafts.csswg.org/web-animations-1/#keyframe>
    keyframes: DomRefCell<Vec<Keyframe>>,
}

impl KeyframeEffect {
    pub(crate) fn new_inherited(window: &Window) -> Self {
        Self {
            animationeffect: AnimationEffect::new_inherited(window),
            target_element: Default::default(),
            composite: Cell::new(CompositeOperation::Replace),
            keyframes: Default::default(),
        }
    }

    fn new_with_proto(
        cx: &mut JSContext,
        window: &Window,
        proto: Option<HandleObject>,
    ) -> DomRoot<Self> {
        reflect_dom_object_with_proto(cx, Box::new(Self::new_inherited(window)), window, proto)
    }

    /// The effect target, when it is an element.
    #[allow(dead_code)] // Used from Phase 5 on; remove the attribute then.
    pub(crate) fn target(&self) -> Option<DomRoot<Element>> {
        self.target_element.get()
    }
}

impl KeyframeEffectMethods<crate::DomTypeHolder> for KeyframeEffect {
    /// <https://drafts.csswg.org/web-animations-1/#dom-keyframeeffect-keyframeeffect>
    fn Constructor(
        cx: &mut JSContext,
        window: &Window,
        proto: Option<HandleObject>,
        target: Option<&Element>,
        keyframes: *mut JSObject,
        options: UnrestrictedDoubleOrKeyframeEffectOptions,
    ) -> Fallible<DomRoot<KeyframeEffect>> {
        // Step 1. Create a new KeyframeEffect object, effect.
        let effect = KeyframeEffect::new_with_proto(cx, window, proto);

        // Step 2. Set the target element of effect to target.
        effect.target_element.set(target);

        // Step 3. The target pseudo-selector is always null: pseudo-element
        // targets are not supported.

        // Step 4. Let timing input be the result corresponding to the first
        // matching condition below: a KeyframeEffectOptions object is used as
        // is; a number is a duration with every other member at its default.
        let (timing_input, composite) = match options {
            UnrestrictedDoubleOrKeyframeEffectOptions::UnrestrictedDouble(duration) => {
                (optional_effect_timing_for_duration(duration), None)
            },
            UnrestrictedDoubleOrKeyframeEffectOptions::KeyframeEffectOptions(options) => (
                optional_effect_timing(&options.parent),
                Some(options.composite),
            ),
        };

        // Step 5. Call the procedure to update the timing properties of an
        // animation effect of effect from timing input. If that procedure
        // causes an exception to be thrown, propagate the exception and abort
        // this procedure.
        effect
            .upcast::<AnimationEffect>()
            .update_the_timing_properties(cx, &timing_input)?;

        // Step 6. If options is a KeyframeEffectOptions object, assign the
        // composite property of effect to the corresponding value from options.
        if let Some(composite) = composite {
            effect.composite.set(composite);
        }

        // Step 7. Initialize the set of keyframes by performing the procedure
        // defined for setKeyframes() passing keyframes as the input.
        effect.SetKeyframes(cx, keyframes)?;

        Ok(effect)
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-keyframeeffect-target>
    fn GetTarget(&self) -> Option<DomRoot<Element>> {
        self.target_element.get()
    }

    fn SetTarget(&self, target: Option<&Element>) {
        self.target_element.set(target);
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-keyframeeffect-composite>
    fn Composite(&self) -> CompositeOperation {
        self.composite.get()
    }

    fn SetComposite(&self, composite: CompositeOperation) {
        self.composite.set(composite);
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-keyframeeffect-getkeyframes>
    #[expect(unsafe_code)]
    fn GetKeyframes(
        &self,
        cx: &mut JSContext,
        result: &mut RootedVec<'_, Box<Heap<*mut JSObject>>>,
    ) -> Fallible<()> {
        let mut layout = self.upcast::<AnimationEffect>().window().layout_mut();
        let stylist = layout.stylist_mut();

        // Step 1. Let result be an empty sequence of objects.
        debug_assert!(result.is_empty());

        // Step 2. Let keyframes be the result of applying the procedure
        // compute missing keyframe offsets to the keyframes for this keyframe
        // effect. (There are no CSSAnimation objects here.)
        let keyframes = self.keyframes.borrow();
        let computed_offsets = compute_missing_keyframe_offsets(
            &keyframes
                .iter()
                .map(|keyframe| keyframe.offset)
                .collect::<Vec<_>>(),
        );

        // Step 3. For each keyframe in keyframes perform the following steps:
        for (keyframe, computed_offset) in keyframes.iter().zip(computed_offsets) {
            // Step 3.1 Initialize a dictionary object, output keyframe.
            // Step 3.2 Set the offset, computedOffset, easing, and composite
            // members of output keyframe to the respective keyframe offset,
            // computed keyframe offset, keyframe-specific easing function, and
            // keyframe-specific composite operation values of keyframe.
            let base_keyframe = BaseComputedKeyframe {
                composite: keyframe.composite,
                offset: keyframe.offset.and_then(Finite::new),
                // `computedOffset` has no default in the IDL, so the
                // generated field is an Option.
                computedOffset: Some(Finite::wrap(computed_offset)),
                easing: keyframe.easing.to_css_string().into(),
            };
            rooted!(&in(cx) let mut output_keyframe = unsafe { JS_NewObject(cx, ptr::null()) });
            base_keyframe.to_jsobject(cx, output_keyframe.handle_mut());

            // Step 3.3 For each animation property-value pair declaration in
            // keyframe, perform the following steps:
            for property_value_pair in &keyframe.declarations {
                debug_assert!(property_value_pair.property_id.is_animatable());

                // Step 3.3.1 Let property name be the result of applying the
                // animation property name to IDL attribute name algorithm to the
                // property name of declaration.
                let mut property_name = String::new();
                let mut writer = CssWriter::new(&mut property_name);
                if property_value_pair.property_id.to_css(&mut writer).is_err() {
                    continue;
                }
                let property_name = animation_property_name_to_idl_attribute_name(&property_name);

                // Step 3.3.2 Let IDL value be the result of serializing the
                // property value of declaration by passing declaration to the
                // algorithm to serialize a CSS value [CSSOM].
                let mut value_string = String::new();
                if property_value_pair
                    .block
                    .single_value_to_css(
                        &property_value_pair.property_id,
                        &mut value_string,
                        None,
                        stylist,
                    )
                    .is_err()
                {
                    continue;
                }

                // Step 3.3.3 Let value be the result of converting IDL value to
                // an ECMAScript String value.
                rooted!(&in(cx) let mut value = UndefinedValue());
                value_string.to_jsval(cx, value.handle_mut());

                // Step 3.3.4 Call the [[DefineOwnProperty]] internal method on
                // output keyframe with property name property name, Property
                // Descriptor { [[Writable]]: true, [[Enumerable]]: true,
                // [[Configurable]]: true, [[Value]]: value } and Boolean flag
                // false.
                let Ok(property_name) = CString::new(property_name) else {
                    continue;
                };

                let success = unsafe {
                    JS_DefineProperty(
                        cx,
                        output_keyframe.handle(),
                        property_name.as_ptr(),
                        value.handle(),
                        JSPROP_ENUMERATE as u32,
                    )
                };
                if !success {
                    if cfg!(debug_assertions) {
                        unreachable!("Setting a property on output_keyframe should never fail");
                    }
                    return Err(Error::Operation(None));
                }
            }

            // Step 3.4 Append output keyframe to result.
            result.push(Heap::boxed(output_keyframe.get()))
        }

        // Step 4. Return result.
        Ok(())
    }

    /// <https://drafts.csswg.org/web-animations-1/#dom-keyframeeffect-setkeyframes>
    fn SetKeyframes(&self, cx: &mut JSContext, keyframes: *mut JSObject) -> Fallible<()> {
        // > This effect’s set of keyframes is replaced with the result of
        // > performing the procedure to process a keyframes argument. If that
        // > procedure throws an exception, this effect’s keyframes are not
        // > modified.
        let document = self.upcast::<AnimationEffect>().window().Document();
        let keyframes = process_a_keyframes_argument(cx, &document, keyframes)?;
        *self.keyframes.safe_borrow_mut(cx.no_gc()) = keyframes;
        Ok(())
    }
}

/// <https://drafts.csswg.org/web-animations-1/#keyframe>
#[derive(JSTraceable, MallocSizeOf)]
struct Keyframe {
    /// <https://drafts.csswg.org/web-animations-1/#keyframe-offset>
    offset: Option<f64>,
    /// <https://drafts.csswg.org/web-animations-1/#keyframe-specific-easing-function>
    #[no_trace]
    easing: TimingFunction,
    /// <https://drafts.csswg.org/web-animations-1/#keyframe-specific-composite-operation>
    composite: CompositeOperationOrAuto,
    declarations: Vec<KeyframePropertyDeclaration>,
}

/// A keyframe whose easing has been read but not yet parsed: the spec
/// parses easings only after every property of the input was read.
struct RawKeyframe {
    offset: Option<f64>,
    easing: DOMString,
    composite: CompositeOperationOrAuto,
    declarations: Vec<KeyframePropertyDeclaration>,
}

#[derive(JSTraceable, MallocSizeOf)]
struct KeyframePropertyDeclaration {
    #[no_trace]
    property_id: PropertyId,
    /// The block is known to only contain declarations for `property_id`. There might
    /// be more than one value if `property_id` is a shorthand.
    #[no_trace]
    block: PropertyDeclarationBlock,
}

fn type_error(message: &'static CStr) -> Error {
    Error::Type(message.to_owned())
}

/// <https://drafts.csswg.org/web-animations-1/#process-a-keyframes-argument>
fn process_a_keyframes_argument(
    cx: &mut JSContext,
    document: &Document,
    keyframes: *mut JSObject,
) -> Fallible<Vec<Keyframe>> {
    // Step 1. If object is null, return an empty sequence of keyframes.
    if keyframes.is_null() {
        return Ok(Vec::new());
    }

    // Step 2. Let processed keyframes be an empty sequence of keyframes.
    let mut processed_keyframes: Vec<RawKeyframe> = Vec::new();
    let mut unused_easings: Vec<DOMString> = Vec::new();

    // Step 3. Let method be the result of GetMethod(object, @@iterator).
    // Step 4. Check the completion record of method.
    // Step 5. Perform the steps corresponding to the first matching condition
    // below. `for_of` reports a missing @@iterator as ValueIsNotIterable.
    rooted!(&in(cx) let iterable = ObjectValue(keyframes));
    let result = for_of(cx, iterable.handle(), |cx, iterator_element| {
        // Step 5.3.4 Let nextItem be IteratorValue(next).
        // Step 5.3.5 Check the completion record of nextItem.
        // Note: This happens inside the "for_of" call.

        // Step 5.3.6 If Type(nextItem) is not Undefined, Null or Object, then
        // throw a TypeError and abort these steps.
        if !iterator_element.is_null_or_undefined() && !iterator_element.is_object() {
            return Err(ForOfIterationFailure::Other(type_error(
                c"Keyframe must be an object, null or undefined",
            )));
        }

        // Step 5.3.7 Append to processed keyframes the result of running the
        // procedure to process a keyframe-like object passing nextItem as the
        // keyframe input with the allow lists flag set to false.
        processed_keyframes.push(process_a_keyframe_like_object(
            cx,
            document,
            iterator_element,
        )?);

        Ok(ControlFlow::Continue(()))
    });
    match result {
        Ok(()) => {},
        Err(ForOfIterationFailure::ValueIsNotIterable) => {
            rooted!(&in(cx) let object = keyframes);
            unused_easings = process_a_property_indexed_keyframes_argument(
                cx,
                document,
                object.handle(),
                &mut processed_keyframes,
            )?;
        },
        Err(ForOfIterationFailure::JSFailed) => return Err(Error::JSFailed),
        Err(ForOfIterationFailure::Other(error)) => return Err(error),
    }

    // Step 6. If processed keyframes is not loosely sorted by offset, throw a
    // TypeError and abort these steps.
    let mut previous_offset: Option<f64> = None;
    for keyframe in &processed_keyframes {
        let Some(offset) = keyframe.offset else {
            continue;
        };
        if previous_offset.is_some_and(|previous| offset < previous) {
            return Err(type_error(c"Keyframes are not loosely sorted by offset"));
        }
        previous_offset = Some(offset);
    }

    // Step 7. If there exist any keyframe in processed keyframes whose keyframe
    // offset is non-null and less than zero or greater than one, throw a
    // TypeError and abort these steps.
    if processed_keyframes
        .iter()
        .any(|keyframe| keyframe.offset.is_some_and(|offset| !(0.0..=1.0).contains(&offset)))
    {
        return Err(type_error(c"Keyframe offsets must be between 0 and 1"));
    }

    // Step 8. For each frame in processed keyframes, perform the following steps:
    // Step 8.1 happened while reading: invalid property values were discarded.
    // Step 8.2 Let the easing function of frame be the result of parsing the
    // "easing" property on frame using the CSS syntax defined for the easing
    // member of the EffectTiming dictionary. If parsing the "easing" property
    // fails, throw a TypeError and abort this procedure.
    let urlextradata = document.url().into_url().into();
    let parser_context = parser_context_for_document(
        document,
        CssRuleType::Style,
        ParsingMode::DEFAULT,
        &urlextradata,
    );
    let mut keyframes = Vec::with_capacity(processed_keyframes.len());
    for keyframe in processed_keyframes {
        let easing = parse_easing(&keyframe.easing, &parser_context)
            .ok_or_else(|| type_error(c"Keyframe easing is not a valid timing function"))?;
        keyframes.push(Keyframe {
            offset: keyframe.offset,
            easing,
            composite: keyframe.composite,
            declarations: keyframe.declarations,
        });
    }

    // Step 9. Parse each of the values in unused easings using the CSS syntax
    // defined for easing member of the EffectTiming interface, and if any of
    // the values fail to parse, throw a TypeError and abort this procedure.
    for easing in unused_easings {
        parse_easing(&easing, &parser_context)
            .ok_or_else(|| type_error(c"Keyframe easing is not a valid timing function"))?;
    }

    Ok(keyframes)
}

/// Parses `input` with the `<easing-function>` production, whole.
fn parse_easing(input: &DOMString, parser_context: &ParserContext<'_>) -> Option<TimingFunction> {
    let input = input.str();
    let mut parser = Parser::new(&input);
    parser
        .parse_entirely(|parser| TimingFunction::parse(parser_context, parser))
        .ok()
}

/// <https://drafts.csswg.org/web-animations-1/#process-a-keyframe-like-object>
/// with the allow lists flag false (the array form).
fn process_a_keyframe_like_object(
    cx: &mut JSContext,
    document: &Document,
    keyframe_input: HandleValue,
) -> Fallible<RawKeyframe> {
    // Step 1. Run the procedure to convert an ECMAScript value to a dictionary
    // type [WEBIDL] with keyframe input as the ECMAScript value, and the
    // dictionary type BaseKeyframe. Store the result of this procedure as
    // keyframe output.
    let Ok(keyframe_output) = BaseKeyframe::from_jsval(cx, keyframe_input, ()) else {
        return Err(Error::JSFailed);
    };
    let ConversionResult::Success(keyframe_output) = keyframe_output else {
        return Err(type_error(c"Keyframe has an invalid offset, easing or composite"));
    };
    let mut keyframe_output = RawKeyframe {
        offset: keyframe_output.offset.map(|offset| *offset),
        easing: keyframe_output.easing,
        composite: keyframe_output.composite,
        declarations: Vec::new(),
    };

    // Step 2. If keyframe input is null or undefined, return keyframe output.
    if keyframe_input.is_null_or_undefined() {
        return Ok(keyframe_output);
    }

    // Steps 3 to 7: read the animatable properties of the object.
    let urlextradata = document.url().into_url().into();
    let parser_context = parser_context_for_document(
        document,
        CssRuleType::Style,
        ParsingMode::DEFAULT,
        &urlextradata,
    );
    rooted!(&in(cx) let object = keyframe_input.to_object());
    keyframe_output.declarations =
        get_property_declarations(cx, object.handle(), &parser_context, false)?
            .into_iter()
            .flat_map(|(_, declarations)| declarations)
            .flatten()
            .collect();

    // Step 8. Return keyframe output.
    Ok(keyframe_output)
}

/// The "Otherwise" branch of step 5 of
/// <https://drafts.csswg.org/web-animations-1/#process-a-keyframes-argument>:
/// the property-indexed form. Fills `processed_keyframes` and returns the
/// unused easings for step 9.
fn process_a_property_indexed_keyframes_argument(
    cx: &mut JSContext,
    document: &Document,
    object: HandleObject,
    processed_keyframes: &mut Vec<RawKeyframe>,
) -> Fallible<Vec<DOMString>> {
    let urlextradata = document.url().into_url().into();
    let parser_context = parser_context_for_document(
        document,
        CssRuleType::Style,
        ParsingMode::DEFAULT,
        &urlextradata,
    );

    // Step 5.1 Let property-indexed keyframe be the result of running the
    // procedure to process a keyframe-like object passing object as the
    // keyframe input and with the allow lists flag set to true.
    let properties = get_property_declarations(cx, object, &parser_context, true)?;

    // Step 5.2 For each member, m, in property-indexed keyframe (the
    // "composite", "easing" and "offset" members are handled below):
    struct PendingKeyframe {
        computed_offset: f64,
        declarations: Vec<KeyframePropertyDeclaration>,
    }
    let mut pending: Vec<PendingKeyframe> = Vec::new();
    for (_, values) in properties {
        if values.is_empty() {
            continue;
        }
        // Steps 5.2.4 to 5.2.6: one keyframe per value, with a null offset,
        // then compute missing keyframe offsets on that list.
        let offsets = compute_missing_keyframe_offsets(&vec![None; values.len()]);
        for (declaration, computed_offset) in values.into_iter().zip(offsets) {
            pending.push(PendingKeyframe {
                computed_offset,
                declarations: declaration.into_iter().collect(),
            });
        }
    }

    // Step 5.3 Sort processed keyframes by the computed keyframe offset of each
    // keyframe in increasing order.
    pending.sort_by(|a, b| {
        a.computed_offset
            .partial_cmp(&b.computed_offset)
            .unwrap_or(Ordering::Equal)
    });

    // Step 5.4 Merge adjacent keyframes in processed keyframes when they have
    // equal computed keyframe offsets.
    let mut merged: Vec<PendingKeyframe> = Vec::new();
    for keyframe in pending {
        match merged.last_mut() {
            Some(last) if last.computed_offset == keyframe.computed_offset => {
                last.declarations.extend(keyframe.declarations);
            },
            _ => merged.push(keyframe),
        }
    }
    for keyframe in merged {
        processed_keyframes.push(RawKeyframe {
            offset: None,
            easing: DOMString::from("linear"),
            composite: CompositeOperationOrAuto::Auto,
            declarations: keyframe.declarations,
        });
    }

    // Steps 5.5 and 5.6 Assign each value in offsets to the keyframe offset of
    // the keyframe with the corresponding position in processed keyframes until
    // the end of either sequence is reached.
    let offsets = read_member_list(cx, object, c"offset", convert_nullable_double)?;
    for (keyframe, offset) in processed_keyframes.iter_mut().zip(offsets) {
        keyframe.offset = offset;
    }

    // Steps 5.7 to 5.11 Easings: default to "linear", repeat to the length of
    // processed keyframes, keep the excess for step 9.
    let mut easings = read_member_list(cx, object, c"easing", convert_domstring)?;
    if easings.is_empty() {
        easings.push(DOMString::from("linear"));
    }
    let unused_easings = if easings.len() > processed_keyframes.len() {
        easings.split_off(processed_keyframes.len())
    } else {
        Vec::new()
    };
    for (index, keyframe) in processed_keyframes.iter_mut().enumerate() {
        keyframe.easing = easings[index % easings.len()].clone();
    }

    // Step 5.12 If the composite member of property-indexed keyframe is not an
    // empty sequence, repeat it likewise and assign every value that is not
    // "auto".
    let composites = read_member_list(cx, object, c"composite", convert_composite)?;
    if !composites.is_empty() {
        for (index, keyframe) in processed_keyframes.iter_mut().enumerate() {
            let composite = composites[index % composites.len()];
            if composite != CompositeOperationOrAuto::Auto {
                keyframe.composite = composite;
            }
        }
    }

    Ok(unused_easings)
}

/// Reads `name` from `object` as `(T or sequence<T>)`: a missing member is
/// an empty list, an iterable object is converted element by element, and
/// anything else is a single value.
#[expect(unsafe_code)]
fn read_member_list<T>(
    cx: &mut JSContext,
    object: HandleObject,
    name: &CStr,
    convert: fn(&mut JSContext, HandleValue) -> Fallible<T>,
) -> Fallible<Vec<T>> {
    rooted!(&in(cx) let mut value = UndefinedValue());
    if !unsafe {
        JS_GetProperty(
            cx.raw_cx(),
            object.into_handle(),
            name.as_ptr(),
            value.handle_mut().into_handle_mut(),
        )
    } {
        return Err(Error::JSFailed);
    }
    if value.is_undefined() {
        return Ok(Vec::new());
    }
    convert_single_or_list(cx, value.handle(), true, convert)
}

/// Converts `value` to a list of `T`: an iterable object is walked when
/// `allow_lists` is set; otherwise the value converts to one element.
fn convert_single_or_list<T>(
    cx: &mut JSContext,
    value: HandleValue,
    allow_lists: bool,
    convert: fn(&mut JSContext, HandleValue) -> Fallible<T>,
) -> Fallible<Vec<T>> {
    // Strings are iterable in JavaScript but convert as a single DOMString;
    // only objects may be sequences.
    if allow_lists && value.is_object() {
        let mut items = Vec::new();
        let result = for_of(cx, value, |cx, item| {
            items.push(convert(cx, item).map_err(ForOfIterationFailure::Other)?);
            Ok(ControlFlow::Continue(()))
        });
        match result {
            Ok(()) => return Ok(items),
            Err(ForOfIterationFailure::ValueIsNotIterable) => {},
            Err(ForOfIterationFailure::JSFailed) => return Err(Error::JSFailed),
            Err(ForOfIterationFailure::Other(error)) => return Err(error),
        }
    }
    Ok(vec![convert(cx, value)?])
}

/// WebIDL `double?`: null and undefined are null, NaN and infinities throw.
fn convert_nullable_double(cx: &mut JSContext, value: HandleValue) -> Fallible<Option<f64>> {
    if value.is_null_or_undefined() {
        return Ok(None);
    }
    match f64::from_jsval(cx, value, ()) {
        Ok(ConversionResult::Success(double)) if double.is_finite() => Ok(Some(double)),
        Ok(_) => Err(type_error(c"Keyframe offset must be a finite number or null")),
        Err(_) => Err(Error::JSFailed),
    }
}

/// WebIDL `DOMString`.
fn convert_domstring(cx: &mut JSContext, value: HandleValue) -> Fallible<DOMString> {
    match DOMString::from_jsval(cx, value, StringificationBehavior::Default) {
        Ok(ConversionResult::Success(string)) => Ok(string),
        Ok(ConversionResult::Failure(_)) => Err(type_error(c"Value cannot be converted to a string")),
        Err(_) => Err(Error::JSFailed),
    }
}

/// WebIDL `CompositeOperationOrAuto`.
fn convert_composite(
    cx: &mut JSContext,
    value: HandleValue,
) -> Fallible<CompositeOperationOrAuto> {
    match CompositeOperationOrAuto::from_jsval(cx, value, ()) {
        Ok(ConversionResult::Success(composite)) => Ok(composite),
        Ok(ConversionResult::Failure(_)) => {
            Err(type_error(c"composite must be \"replace\", \"add\", \"accumulate\" or \"auto\""))
        },
        Err(_) => Err(Error::JSFailed),
    }
}

/// Steps 3 to 7 of
/// <https://drafts.csswg.org/web-animations-1/#process-a-keyframe-like-object>.
///
/// Returns, per animatable property found on the object in codepoint order of
/// the property name, the parsed declaration of each value; `None` marks a
/// value that did not parse (discarded per step 8.1 of "process a keyframes
/// argument", but still counted as a keyframe in the property-indexed form).
#[expect(unsafe_code)]
fn get_property_declarations(
    cx: &mut JSContext,
    object: HandleObject,
    parser_context: &ParserContext<'_>,
    allow_lists: bool,
) -> Fallible<Vec<(NonCustomPropertyId, Vec<Option<KeyframePropertyDeclaration>>)>> {
    // The spec tells us to iterate over all animatable properties and see if
    // they're defined on the object. Instead we can iterate over the own
    // properties of the object and see if they're animated properties, that's
    // easier.
    let mut ids = IdVector::new(cx);
    if !unsafe { GetPropertyKeys(cx, object, JSITER_OWNONLY, ids.handle_mut()) } {
        return Ok(Vec::new());
    }

    let mut properties = Vec::with_capacity(ids.len());
    for id in ids.iter() {
        rooted!(&in(cx) let id = *id);

        // See if the id for the current property on the object represents a
        // animatable CSS property.
        if !id.is_string() {
            continue;
        }
        rooted!(&in(cx) let mut key_value = UndefinedValue());
        let raw_id: HandleId = id.handle().into();
        if !unsafe { JS_IdToValue(cx, *raw_id.ptr, key_value.handle_mut()) } {
            continue;
        }
        rooted!(&in(cx) let js_string = key_value.to_string());
        let Some(js_string) = NonNull::new(js_string.get()) else {
            continue;
        };
        let property_name = unsafe { jsstr_to_string(cx, js_string) };

        let Some(property_id) = lookup_css_property_by_idl_attribute_name(&property_name) else {
            continue;
        };
        debug_assert!(property_id.is_animatable());

        // Step 7.1 Let raw value be the result of calling the [[Get]] internal
        // method on keyframe input, with property name as the property key and
        // keyframe input as the receiver.
        // Step 7.2 Check the completion record of raw value.
        rooted!(&in(cx) let mut property_value = UndefinedValue());
        if !unsafe {
            JS_GetPropertyById(
                cx.raw_cx(),
                object.into_handle(),
                id.handle().into_handle(),
                property_value.handle_mut().into_handle_mut(),
            )
        } {
            continue;
        }

        // Step 7.3 Convert raw value to a DOMString or to a sequence of
        // DOMStrings property values, depending on allow lists.
        let values =
            convert_single_or_list(cx, property_value.handle(), allow_lists, convert_domstring)?;

        // Step 7.4 The normalized property name is already known (see the
        // comment at the top of this function).
        // Parse each value as a value for the given animatable CSS property.
        // The specification continues to treat the value as a plain string,
        // but there's not much point.
        let declarations = values
            .iter()
            .map(|value| {
                parse_single_property_declaration(property_id, &value.str(), parser_context)
            })
            .collect();

        // Step 7.5 Add a property to keyframe output with normalized property
        // name as the property name, and property values as the property value.
        properties.push((property_id, declarations));
    }

    // Step 6. Sort animation properties in ascending order by the Unicode
    // codepoints that define each property name.
    properties.sort_by(|a, b| a.0.name().cmp(b.0.name()));

    Ok(properties)
}
```

Then keep, unchanged, the three helpers that followed in the original
file: `parse_single_property_declaration`,
`lookup_css_property_by_idl_attribute_name` and
`animation_property_name_to_idl_attribute_name` (they sit between the code
above and `compute_missing_keyframe_offsets`).

**Step 2: `element.rs`**

The constructor now applies the options, so `Animate` stops doing it. In
`Animate` (from Phase 2):

- change the `(timeline, id, timing_input)` match back to a two-tuple:
  `(options.timeline.clone().flatten(), Some(options.id.clone()))` for the
  dictionary arm and `(None, None)` for the number arm, binding it as
  `let (timeline, id) = match &options { ... }`;
- delete the `update_the_timing_properties` call and its comment that
  followed the effect construction;
- add `?` to the effect construction:

```rust
        let effect =
            KeyframeEffect::Constructor(cx, &window, None, Some(target), keyframes, parent_options)?;
```

- remove the now-unused imports of `AnimationEffect`,
  `optional_effect_timing` and `optional_effect_timing_for_duration` from
  `element.rs`.

**Step 3: Compile and test**

```bash
cd ~/vibes/servo && UV_NO_SYNC=1 cargo check -p servo-script 2>&1 | grep -E "^error" -A 8 | head -60
UV_NO_SYNC=1 cargo test -p servo-script --lib keyframeeffect::tests 2>&1 | tail -3
```

Expected: no errors; `5 passed`. Likely compiler nits and their fixes:
`JS_GetProperty` may live in `js::rust::wrappers2` with a safe signature
in this mozjs (then drop `raw_cx()`/`into_handle*()` like the
`JS_IdToValue` call); `NonCustomPropertyId::name()` returns `&'static
str` (used by the lookup table already); if `CompositeOperationOrAuto` is
not `Copy`, clone it in the composite loop.

**Step 4: Commit Tasks 1 and 2**

```bash
cd ~/vibes/servo
cargo fmt -p servo-script
git add components/script_bindings/webidls/KeyframeEffect.webidl components/script/dom/animations/keyframeeffect.rs components/script/dom/element/element.rs
git commit -m "script: complete KeyframeEffect construction

Apply the timing options and composite from the constructor, expose target
and composite, accept property-indexed keyframes, compute missing keyframe
offsets for getKeyframes(), parse keyframe easings and throw the TypeErrors
of https://drafts.csswg.org/web-animations-1/#process-a-keyframes-argument."
```
<!-- END_TASK_2 -->

<!-- START_TASK_3 -->
### Task 3: Test pages, publish, pin

**Files:**
- Create: `crates/paladin-engine/tests/pages/keyframes-round-trip.html`
- Create: `crates/paladin-engine/tests/pages/computed-timing.html`
- Create: `crates/paladin-engine/tests/pages/keyframes-errors.html`
- Modify: `crates/paladin-engine/tests/web_animations.rs` (the `PAGES`
  list)
- Modify: `Cargo.lock`, `CHANGELOG.md`, `docs/upstream-issues.md`

**Step 1: The pages**

`crates/paladin-engine/tests/pages/keyframes-round-trip.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>keyframes-round-trip</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
const check = (condition, what) => { if (!condition) throw new Error(what); };

try {
  const box = document.getElementById('box');

  // Array form.
  const effect = new KeyframeEffect(box, [
    { opacity: 1, easing: 'ease-in', composite: 'add' },
    { opacity: 0.5 },
    { opacity: 0, offset: 1 },
  ], { duration: 100 });
  check(effect.target === box, 'target');
  check(effect.composite === 'replace', 'default composite is ' + effect.composite);
  effect.composite = 'accumulate';
  check(effect.composite === 'accumulate', 'composite setter');
  const frames = effect.getKeyframes();
  check(frames.length === 3, 'array form keyframe count ' + frames.length);
  check(frames[0].offset === null, 'frames[0].offset ' + frames[0].offset);
  check(frames[0].computedOffset === 0, 'frames[0].computedOffset ' + frames[0].computedOffset);
  check(frames[1].computedOffset === 0.5, 'frames[1].computedOffset ' + frames[1].computedOffset);
  check(frames[2].offset === 1 && frames[2].computedOffset === 1, 'frames[2] offsets');
  check(frames[0].easing === 'ease-in', 'frames[0].easing ' + frames[0].easing);
  check(frames[1].easing === 'linear', 'frames[1].easing ' + frames[1].easing);
  check(frames[0].composite === 'add', 'frames[0].composite ' + frames[0].composite);
  check(frames[1].composite === 'auto', 'frames[1].composite ' + frames[1].composite);
  check(frames[0].opacity === '1', 'frames[0].opacity ' + frames[0].opacity);
  check(frames[1].opacity === '0.5', 'frames[1].opacity ' + frames[1].opacity);

  // Property-indexed form.
  const indexed = new KeyframeEffect(box, {
    opacity: [1, 0.5, 0],
    transform: ['scale(1)', 'scale(2)'],
    easing: ['ease-in', 'ease-out'],
    offset: [0, 0.25],
    composite: 'replace',
  }, 100);
  const f = indexed.getKeyframes();
  check(f.length === 3, 'indexed keyframe count ' + f.length);
  check(f[0].opacity === '1' && f[0].transform === 'scale(1)', 'f[0] values');
  check(f[1].opacity === '0.5' && f[1].transform === undefined, 'f[1] values');
  check(f[2].opacity === '0' && f[2].transform === 'scale(2)', 'f[2] values');
  check(f[0].offset === 0 && f[1].offset === 0.25 && f[2].offset === null, 'indexed offsets');
  check(f[1].computedOffset === 0.25 && f[2].computedOffset === 1, 'indexed computed offsets');
  check(f[0].easing === 'ease-in' && f[1].easing === 'ease-out' && f[2].easing === 'ease-in', 'indexed easings repeat');
  check(f.every((k) => k.composite === 'replace'), 'indexed composite');

  // setKeyframes replaces, null clears, animate() keeps the keyframes.
  effect.setKeyframes({ opacity: '0.25' });
  check(effect.getKeyframes().length === 1 && effect.getKeyframes()[0].computedOffset === 1, 'single keyframe');
  effect.setKeyframes(null);
  check(effect.getKeyframes().length === 0, 'null keyframes');
  const anim = box.animate([{ opacity: 0.2 }, { opacity: 0.8 }], 100);
  check(anim.effect.getKeyframes().length === 2, 'animate() keyframes');
  check(anim.effect.target === box, 'animate() target');

  verdict('PASS');
} catch (e) {
  fail(e.message);
}
</script>
```

`crates/paladin-engine/tests/pages/computed-timing.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>computed-timing</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
setTimeout(() => fail('did not settle within 5s'), 5000);
const check = (condition, what) => { if (!condition) throw new Error(what); };

(async () => {
  const box = document.getElementById('box');

  // Specified vs computed timing with no animation.
  const bare = new KeyframeEffect(box, null, 500);
  check(bare.getTiming().duration === 500, 'bare duration');
  check(bare.getTiming().fill === 'auto', 'bare fill is ' + bare.getTiming().fill);
  check(bare.getComputedTiming().fill === 'none', 'bare computed fill');
  check(bare.getComputedTiming().localTime === null, 'bare localTime');
  check(bare.getComputedTiming().progress === null, 'bare progress');
  check(bare.getComputedTiming().endTime === 500, 'bare endTime');
  const auto = new KeyframeEffect(box, null);
  check(auto.getTiming().duration === 'auto', 'auto duration');
  check(auto.getComputedTiming().duration === 0, 'auto computed duration');

  // Every option reaches the effect, and the computed values follow.
  const anim = box.animate([{ opacity: 1 }, { opacity: 0 }], {
    duration: 1000, delay: 100, endDelay: 50, iterations: 2, iterationStart: 0.25,
    direction: 'alternate', easing: 'ease-in-out', fill: 'both',
  });
  anim.pause();
  anim.currentTime = 600;
  await anim.ready;
  const t = anim.effect.getComputedTiming();
  check(t.duration === 1000, 'duration ' + t.duration);
  check(t.delay === 100, 'delay ' + t.delay);
  check(t.endDelay === 50, 'endDelay ' + t.endDelay);
  check(t.iterations === 2, 'iterations ' + t.iterations);
  check(t.iterationStart === 0.25, 'iterationStart ' + t.iterationStart);
  check(t.direction === 'alternate', 'direction ' + t.direction);
  check(t.easing === 'ease-in-out', 'easing ' + t.easing);
  check(t.fill === 'both', 'fill ' + t.fill);
  check(t.activeDuration === 2000, 'activeDuration ' + t.activeDuration);
  check(t.endTime === 2150, 'endTime ' + t.endTime);
  check(t.localTime === 600, 'localTime ' + t.localTime);
  check(t.currentIteration === 0, 'currentIteration ' + t.currentIteration);
  // active time 500 of 1000, plus iterationStart 0.25: directed progress
  // 0.75, through cubic-bezier(0.42, 0, 0.58, 1) is about 0.871.
  check(Math.abs(t.progress - 0.871) < 0.01, 'progress ' + t.progress);

  // updateTiming() validates and applies.
  let threw = false;
  try { anim.effect.updateTiming({ iterations: -1 }); } catch (e) { threw = e instanceof TypeError; }
  check(threw, 'negative iterations did not throw a TypeError');
  anim.effect.updateTiming({ duration: 200, fill: 'none' });
  check(anim.effect.getTiming().duration === 200, 'updated duration');
  check(anim.effect.getComputedTiming().progress === null, 'fill none past the end');
  // Paused wins over finished in the play state calculation.
  check(anim.playState === 'paused', 'playState after shortening is ' + anim.playState);
  anim.play();
  check(anim.playState === 'running', 'playState after replay is ' + anim.playState);
  await anim.finished;
  check(anim.playState === 'finished', 'playState after the shortened run is ' + anim.playState);

  verdict('PASS');
})().catch((e) => fail(e.message));
</script>
```

`crates/paladin-engine/tests/pages/keyframes-errors.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>keyframes-errors</title>
<div id="box" style="width:50px;height:50px;background:teal"></div>
<script>
let settled = false;
const verdict = (title) => { if (!settled) { settled = true; document.title = title; } };
const fail = (reason) => verdict('FAIL: ' + reason);
addEventListener('error', (event) => fail(event.message));
const check = (condition, what) => { if (!condition) throw new Error(what); };
const throwsTypeError = (what, f) => {
  try { f(); } catch (e) { check(e instanceof TypeError, what + ' threw ' + e.name); return; }
  throw new Error(what + ' did not throw');
};

try {
  const box = document.getElementById('box');
  throwsTypeError('unsorted offsets', () =>
    box.animate([{ offset: 0.5, opacity: 1 }, { offset: 0.2, opacity: 0 }], 100));
  throwsTypeError('offset above one', () => box.animate([{ offset: 2, opacity: 1 }], 100));
  throwsTypeError('negative offset', () => box.animate([{ offset: -1, opacity: 1 }], 100));
  throwsTypeError('bad keyframe easing', () =>
    box.animate([{ opacity: 1, easing: 'not-an-easing' }], 100));
  throwsTypeError('bad property-indexed easing', () =>
    box.animate({ opacity: [1, 0], easing: ['bad'] }, 100));
  throwsTypeError('bad unused easing', () =>
    box.animate({ opacity: [1, 0], easing: ['linear', 'linear', 'bad'] }, 100));
  throwsTypeError('non-object keyframe', () => box.animate([5], 100));
  throwsTypeError('negative duration', () => box.animate([{ opacity: 1 }], -100));
  throwsTypeError('negative iterations', () => box.animate([{ opacity: 1 }], { iterations: -1 }));
  throwsTypeError('bad effect easing', () => box.animate([{ opacity: 1 }], { easing: 'nope' }));
  throwsTypeError('bad composite', () => box.animate([{ opacity: 1, composite: 'blend' }], 100));

  // Invalid property values are discarded, not thrown.
  const anim = box.animate([{ opacity: 'not-a-value', width: '10px' }], 100);
  const frame = anim.effect.getKeyframes()[0];
  check(frame.opacity === undefined, 'invalid opacity kept: ' + frame.opacity);
  check(frame.width === '10px', 'valid width lost');

  // A failing setKeyframes() leaves the keyframes alone.
  const effect = new KeyframeEffect(box, [{ opacity: 1 }, { opacity: 0 }], 100);
  try { effect.setKeyframes([{ offset: 3 }]); } catch (e) {}
  check(effect.getKeyframes().length === 2, 'keyframes changed after a failed setKeyframes');

  verdict('PASS');
} catch (e) {
  fail(e.message);
}
</script>
```

**Step 2: Register the pages**

In `crates/paladin-engine/tests/web_animations.rs` extend `PAGES`:

```rust
const PAGES: &[&str] = &[
    "finish-event.html",
    "finished-promise.html",
    "cancel-rejects.html",
    "pause-holds-current-time.html",
    "playback-rate-scales.html",
    "keyframes-round-trip.html",
    "computed-timing.html",
    "keyframes-errors.html",
];
```

**Step 3: Run against the path-patched fork**

Phase 2 removed `~/.cargo/config.toml` when it pinned the dist branch;
recreate it (same content as in `phase_02.md`, "Working in the fork"):

```bash
cat > ~/.cargo/config.toml <<'EOF'
[patch.crates-io]
servo = { path = "/Users/thomas/vibes/servo/components/servo" }
EOF
cd /Users/thomas/vibes/paladin
cargo test -p paladin-engine --test web_animations 2>&1 | tail -15
```

Expected: `ok`. If `frames[0].opacity` serialises differently from `'1'`
(stylo may print `1` as `1`), adjust the page to what `getComputedStyle`
would also return; the other checks are exact per spec.

**Step 4: Publish and pin**

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

**Step 5: Docs and commit**

`CHANGELOG.md`, extend the Web Animations bullet from Phase 2 with:
"`KeyframeEffect` applies its timing options and `composite`, exposes
`target`, accepts property-indexed keyframes and reports computed offsets
from `getKeyframes()`; `getComputedTiming()` returns real values."

`docs/upstream-issues.md`, add to the S13 fork entry: `"script: complete
KeyframeEffect construction"`.

```bash
git add Cargo.lock CHANGELOG.md docs/upstream-issues.md crates/paladin-engine/tests
git commit -m "feat(engine): KeyframeEffect options, property-indexed keyframes, computed timing"
git push origin vnext
```
<!-- END_TASK_3 -->
<!-- END_SUBCOMPONENT_A -->

## Phase 3 done when

- `keyframeeffect::tests` pass in the fork; all eight pages pass in
  Paladin's `web_animations` test against the published dist commit;
  `cargo test --workspace`, clippy and fmt are clean.
