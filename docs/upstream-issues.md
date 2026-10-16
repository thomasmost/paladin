# Upstream issues

Paladin is a shell around Servo, egui, and winit. Some things Paladin cannot
do, or does badly, are upstream limits rather than Paladin bugs. This file
tracks each one: what it is, which Paladin features or improvements depend on
it, what Paladin does in the meantime, and what to do when it lands.

_State recorded as of 2026-10-15, against Servo 0.7.0, egui 0.34, winit 0.30._

## How to maintain this file

- Add an entry when a bug or a planned feature turns out to depend on an
  upstream change. Give it the next id in its group (`S` Servo, `E` egui and
  winit, `T` toolchain and packaging). Link the upstream issue if one exists;
  say "no upstream issue found" otherwise, so the next person knows to file
  or search again.
- On every Servo bump, re-check each `S` entry: the state, whether the
  workaround can go, and whether the dependent feature is now unblocked.
- An issue fixed by a patch in Paladin's Servo or stylo fork moves to the
  "Carried in the fork" section below with the patch commit's subject; on
  every Servo bump re-check whether upstream has merged it and, if so, drop
  the patch during the rebase and move the entry to "Resolved".
- When an entry is resolved upstream and Paladin has picked the fix up, move
  it to the Resolved section rather than deleting it: the history explains
  workarounds that remain in the code.
- `README.md` lists the user-visible limitations in one sentence each and
  points here for the detail.

## Dependency tree

Paladin work, grouped by roadmap phase, with the upstream entry each item
depends on. An item under an entry does not start until that entry resolves,
or starts with the workaround noted in the entry.

- **Phase 1 polish**
  - Enable the Stop button: [S1](#s1-no-way-to-stop-a-load).
  - Google's search box clips descenders: [S2](#s2-textarea-rows-sizing-uses-the-font-size).
  - Fades and masks (Google's search box edges; wikipedia.org logs the same
    warning): [S3](#s3-mask-image-unsupported).
  - Ellipsis on overflowing titles and labels:
    [S4](#s4-text-overflow-unsupported).
  - Sites that call `element.animate()` without feature detection
    (bsky.app): [Carried in the fork: Web Animations](#carried-in-the-fork).
  - Clean stderr on exit: [S6](#s6-memory-profiler-warnings-on-exit).
  - Quiet `cargo build` on macOS:
    [T1](#t1-__eh_frame-section-too-large-linker-warning-on-macos).
  - Style-thread stack overflow on wikipedia.org: resolved by
    [S5](#s5-style-threads-overflow-their-512-kb-stack-resolved-in-paladin).
  - Windows release builds crash on wikipedia.org (the `<select>` value
    setter): [Carried in the fork: select value setter](#carried-in-the-fork).
  - Packaging on both hosts: worked around in
    [T2](#t2-cargo-packager-0118-fails-on-formats-the-host-cannot-build).
  - Debug build links on Windows: worked around in
    [T3](#t3-the-debug-script-rlib-exceeds-4-gb-on-windows).
  - IME composition in page text fields: not upstream after all; see
    [Checked and not upstream](#checked-and-not-upstream).
- **Phase 2 (hotkeys, profiles, tab moves)**
  - Named profiles with separate cookies and storage:
    [S7](#s7-no-profile-concept-site-data-is-per-process) or
    [S12](#s12-one-servo-instance-per-process), whichever lands first.
  - Moving a tab to another window: [S8](#s8-a-webview-is-bound-to-its-rendering-context).
  - Remappable hotkeys: no upstream dependency.
- **Phase 3 (extensions)**
  - Content scripts: [S9](#s9-isolated-worlds).
  - Extension-to-browser messaging: [S10](#s10-host-to-dom-message-port).
  - The WebExtension API surface itself: [S11](#s11-no-webextension-support).
  - Building blocks that already exist in Servo 0.7.0: `UserContentManager`
    (user scripts and styles injected into every page) and
    `WebView::evaluate_javascript`. Both run in the page's own world, so they
    cover user styles and simple user scripts, not isolated content scripts.

## Servo

### S1: No way to stop a load

- **Upstream:** no issue found. `WebView` in servo 0.7.0 has `load`,
  `reload`, `go_back`, and `go_forward` but no stop. The closest controls
  are `NavigationRequest::deny`, which refuses a navigation before it
  starts, and `InterceptedWebResourceLoad::cancel`, which fails one
  intercepted resource load; neither stops a load in progress.
- **Effect:** the toolbar's Stop button is drawn disabled with a tooltip, and
  the Stop command logs a warning.
- **Blocks:** enabling Stop (Phase 1 polish).
- **When it lands:** wire `Command::Stop` to the new method in
  `paladin-engine`, enable the button, update the README checklist item 3.

### S2: Textarea `rows` sizing uses the font size

- **Upstream:** adjacent issue [servo/servo#46562][46562] (open) covers `rows`
  being lost under `height: auto`; a maintainer there says textarea sizing
  should move from presentational hints into layout, which would fix this as
  well, and points at [servo/servo#44426][44426] as the counterpart on the
  inline axis (form-control width hints handled outside layout). The
  mismatch itself has no issue yet.
- **What it is:** `components/script/dom/element/element.rs` synthesizes
  `height: <rows>em` for a textarea. The HTML spec defines the height as
  rows times the line height, and `line-height: normal` is already about
  1.15 to 1.2 times the font size, so every textarea sized by `rows` is
  shorter than in other browsers: `<textarea rows="5">` shows about four
  lines. A one-row field gets a content box shorter than its own line and
  `overflow` clips the glyphs. Google's search box is `rows="1"` with
  `line-height: 22px` and 16px text: the clip lands on the baseline and cuts
  every descender. Author-set heights are unaffected.
- **Effect:** cosmetic but prominent; typing on google.com looks broken.
- **Workaround:** none in Paladin. A user stylesheet forcing `height: auto`
  would fix Google and shrink every empty multi-row textarea elsewhere.
- **Blocks:** nothing; a README Known limitations entry.
- **When it lands:** remove the README entry; re-run checklist item 6 (typing
  in a page text field) on google.com.

### S3: `mask-image` unsupported

- **Upstream:** [servo/servo#13925][13925] (open since 2016).
- **What it is:** `mask-image` and `-webkit-mask-image` are parsed as
  unknown properties, so masks are dropped. Google fades the edges of its
  search box with one; Wikipedia logs the same warning.
- **Effect:** cosmetic; the logs show `Unsupported property declaration:
  'mask-image'` on many sites.
- **Blocks:** nothing.

### S4: `text-overflow` unsupported

- **Upstream:** [servo/servo#13702][13702] (open); [servo/servo#40444][40444]
  (open) proposes basic ellipsis support as a first step.
- **What it is:** `text-overflow: ellipsis` is ignored, so overflowing text is
  cut rather than elided. Seen in the logs on wikipedia.org.
- **Effect:** cosmetic on pages.
- **Blocks:** nothing.

### S5: Style threads overflow their 512 KB stack (resolved in Paladin)

- **Upstream:** [servo/servo#43888][43888] raised the stack Servo's own
  builds use to 8 MiB; the stylo default stays 512 KB.
- **What it was:** loading wikipedia.org in a debug build aborted with
  `thread 'StyleThread#1' has overflowed its stack`.
- **Paladin's fix:** `.cargo/config.toml` exports
  `SERVO_STYLE_THREAD_STACK_SIZE_KB=8192` to every cargo invocation (commit
  581c245). Keep it on every Servo bump; see `CLAUDE.md`.

### S6: Memory profiler warnings on exit

- **Upstream:** no issue found.
- **What it is:** on a clean exit Servo logs several lines of
  `profile_traits::mem` `Error communicating with the memory profiler
  thread: Disconnected`. Exit code is 0 and nothing is lost.
- **Effect:** noise on stderr that the README checklist's "no Servo error
  output" items have to tolerate; they are WARN lines, not errors.
- **Blocks:** nothing.

### S7: No profile concept; site data is per process

- **Upstream:** no issue found. Cookies and storage live in Servo's per-process
  data directory. [servo/servo#47126][47126] (open) extends the embedder
  cookie API (`SiteDataManager`) and is the closest active work.
- **Effect:** Paladin cannot run two profiles in one process; see also
  [S12](#s12-one-servo-instance-per-process).
- **Workaround (ROADMAP, Phase 2):** one Paladin process per profile, with a
  `profiles.toml` registry and a launcher.
- **Blocks:** nothing; it shapes Phase 2. In-process profiles would need
  per-`WebView` site data upstream, or S12 resolved so each profile can have
  its own instance.

### S8: A `WebView` is bound to its rendering context

- **Upstream:** no issue found. `WebViewBuilder::new(servo, rendering_context)`
  ties a webview to one rendering context, and in Paladin each window owns
  its own GL context.
- **Effect:** a tab cannot be moved to another window with its live page.
- **Workaround (ROADMAP, Phase 2):** moving a tab is a close plus a create
  with the same URL. Servo 0.7.0 has no API to restore session history, so
  the moved tab starts with an empty history.
- **Blocks:** moving a live tab, with its scroll position, form state, and
  session history, between windows.

### S9: Isolated worlds

- **Upstream:** [servo/servo#13263][13263] (open since 2016, labelled
  E-very-complex, last activity 2026-10-14).
- **What it is:** no way to run a script in a page with access to the DOM but
  its own global, which is what WebExtension content scripts require.
- **Effect:** content scripts cannot be implemented faithfully. Scripts
  injected through `UserContentManager` run in the page's world, where the
  page can see and tamper with them.
- **Blocks:** Phase 3 content scripts, and so most real extensions.

### S10: Host-to-DOM message port

- **Upstream:** [servo/servo#47370][47370], a roadmap proposal for an
  embedder IPC primitive, closed as a duplicate in 2026-08 without a pointer
  to the surviving issue. Search again before Phase 3 starts.
- **What it is:** a structured, low-latency channel between page JavaScript
  and the embedding application, beyond `evaluate_javascript` round trips.
- **Blocks:** extension messaging (`runtime.sendMessage` and ports) and any
  Paladin chrome that needs to talk to page scripts.

### S11: No WebExtension support

- **Upstream:** no issue for the API surface itself; the only related issue
  is S9's [servo/servo#13263][13263], which the design plan refers to.
  Servo has no WebExtension work scheduled.
- **Effect:** Phase 3 depends on building the API surface in Paladin on top of
  S9 and S10, or on a different engine. `ROADMAP.md` records this as the
  likeliest reason to revisit the engine choice.
- **Blocks:** Phase 3.

### S12: One Servo instance per process

- **Upstream:** no issue found. Servo's options and preferences are
  process-global, and its accessibility layer assumes a single instance; the
  design plan lists this under the Servo facts that constrain the design.
- **Effect:** Paladin creates one `Engine` per process, and the engine test
  binary holds a single test function (see `CLAUDE.md`).
- **Blocks:** in-process profiles, as an alternative route to resolving
  [S7](#s7-no-profile-concept-site-data-is-per-process).

## egui and winit

None yet. The IME item looked like one; see Checked and not upstream.

## Checked and not upstream

Things that looked like upstream limits and turned out to be Paladin work.
Kept so nobody waits on upstream for them.

- **IME composition in page text fields.** egui-winit only enables the
  window's IME when one of its own widgets wants text input (egui-winit
  0.34.3, `src/lib.rs` around line 1105), so page fields get none. Servo
  0.7.0 already tells the embedder when an editable element gains or loses
  focus: `WebViewDelegate::show_embedder_control` with
  `EmbedderControl::InputMethod` and `hide_embedder_control`
  (servo-0.7.0, `webview_delegate.rs`). Paladin has to forward that to
  winit's `set_ime_allowed`, taking egui-winit's per-frame debounce into
  account: it recomputes its own flag every frame and calls
  `set_ime_allowed` whenever that differs from its stored value, so Paladin
  must leave the stored flag to egui and re-assert `set_ime_allowed(true)`
  after each `handle_platform_output` while a page control is shown. On
  Windows egui-winit also uses that stored flag to drop key events the IME
  has already consumed (winit bug #4508), so with the flag left false
  Paladin's own key routing has to drop them before they reach the page.
  Listed in the README todo.

## Toolchain and packaging

### T1: `__eh_frame section too large` linker warning on macOS

- **Upstream:** [rust-lang/rust#159105][159105] (open, "false positive?").
- **What it is:** `cargo build` prints a `linker_messages` warning because
  of Servo's size. CI's build step prints it too, but only clippy runs with
  `-D warnings`, and clippy does not link, so CI passes.
- **Effect:** noise in local builds.
- **Blocks:** nothing.

### T2: cargo-packager 0.11.8 fails on formats the host cannot build

- **Upstream:** no issue found.
- **What it is:** listing `formats` that include NSIS fails the macOS run.
- **Paladin's fix:** the packaging metadata lists no `formats` and relies on
  the per-host default (app and dmg on macOS, NSIS on Windows); see
  `CLAUDE.md`.

### T3: The debug `script` rlib exceeds 4 GB on Windows

- **Upstream:** [rust-lang/rust#151184][151184] (open). An rlib over 4 GB
  gets a broken archive index on Windows; `link.exe` then reports hundreds
  of `unresolved external symbol` errors from that crate (`LNK1120`).
- **What it was:** `cargo build --workspace --all-targets` on Windows
  linked the debug `paladin.exe` against a 4.45 GB
  `libscript-*.rlib` and failed with 983 unresolved externals. Servo's
  workspace avoids it with `[profile.dev.package.servo-script]
  codegen-units = 16` in its root `Cargo.toml`, which does not apply to a
  workspace that depends on Servo.
- **Paladin's workaround:** the same override in the root `Cargo.toml`.
  Drop it when the rustc issue is fixed.
- **Blocks:** nothing.

## Carried in the fork

Patches on `thomasmost/servo` (`paladin/0.7.0`) and `thomasmost/stylo`
(`paladin/0.22.0`), each listed against the upstream issue it addresses.

- **Web Animations API (was S13).** Upstream [servo/servo#36950][36950] and
  [#44174][44174]: Servo 0.7.0 ships the API behind
  `dom_web_animations_enabled` with the playback and sampling halves
  unimplemented. Carried as `thomasmost/servo`
  `paladin/0.7.0` (17 patch commits, oldest first): "script: add the Web
  Animations timing calculations", "script: add AnimationPlaybackEvent",
  "script: implement the Animation timing model", "script: finish
  Element.animate()", "script: request a rendering update for queued
  animation events", "script: tick script animations on their own timeline",
  "script: use expect for the unused sequence number", "script: tidy the
  simple iteration progress check and event queue note", "script: clarify
  why the animation sequence number is unused", "script: complete
  KeyframeEffect construction", "script: propagate exceptions and follow the
  spec order when reading keyframe properties", "script: sample script
  animations through stylo", "script: resample when an animation's effect,
  target or time changes", "script: resample after a playback rate change
  and correct the unrendered-target note", "script: implement
  Element.getAnimations() and Document.getAnimations()", "script: document
  the scope of getAnimations()", "script: note when a moved animation leaves
  getAnimations()"; and
  `thomasmost/stylo` `paladin/0.22.0` (2 patch commits, oldest first):
  "Add script-driven animations to ElementAnimationSet", "Test equal-offset
  script keyframes and the cancel dirty flag". Paladin enables the pref in
  `Engine`. Not covered: `CSSAnimation`/`CSSTransition` objects,
  `commitStyles`, `persist`, replace-state removal, pseudo-element targets,
  shadow-root `getAnimations`. Animations whose timeline's document differs
  from their target's document (same-origin iframes, targets adopted into
  another document) are not returned by `getAnimations()`, because the
  registry is per timeline document, and animations on a null timeline are
  never registered, so they are not returned either. An animation moved off
  a document's timeline stays listed by that document's `getAnimations()`
  until the document's next tick, because the filter does not check the
  timeline. On each Servo bump check whether upstream has landed any of
  #36950's follow-ups and drop the overlapping patches. The series carries
  its review fix-ups as separate commits (about half of the 17); squash
  them into one commit per logical change (timing calculations, playback
  event, timing model, `animate()`, event and rendering-update plumbing,
  `KeyframeEffect` construction, stylo sampling, `getAnimations()`) during
  the 0.8.0 rebase, before republishing, so later rebases stay mechanical.
  Known gaps in this patch set, for a later phase to pick up:
  - **Keyframes resolve once.** stylo resolves a script animation's
    keyframes against the element's style on the first restyle after they
    are pushed and not again; later changes to the base style (`em` or
    `var()` values, the implicit 0 and 1 keyframes, the underlying value
    for `add`) are not picked up until the keyframes are pushed again. CSS
    animations in stylo re-resolve only on `animation-*` changes, so the
    precision matches for the common case.
  - **Overshooting easings are clamped.** The script side pushes the
    transformed progress; an effect easing that overshoots (for example
    `cubic-bezier(.5, -.5, .5, 1.5)`) yields progress outside 0..1, which
    the spec extrapolates from the first or last keyframe interval. stylo's
    sampler clamps to the end keyframes instead.
  - **The first keyframe's easing covers the implicit 0 interval.** When
    the first keyframe omits a property, its value comes from the base
    style but the interval up to the next keyframe that sets the property
    keeps the first keyframe's easing; the spec gives the implicit 0
    keyframe a linear easing. stylo's CSS animation path behaves the same
    way.
  - **Replaced animations are never removed.** "Remove replaced animations"
    (update animations and send events, step 2) is not implemented, so a
    finished `fill: forwards` animation stays in the manager's list and in
    stylo's animation set for the life of the document. Only animations
    that are cancelled or finish without a fill leave the list.
  - **Sampling is per tick.** The value of an animation changes only in a
    rendering update. A synchronous `getComputedStyle` right after
    `animate()`, `currentTime`, `effect.setKeyframes()`, `effect.target`,
    `effect.composite` or `updateTiming()` reads the sample of the previous
    frame. Pages wait a frame or a timer before they read the new value.
  - **Unrendered targets.** A target that is not rendered is not dirtied for
    its sampled value, so hidden animations on targets that are not rooted
    do not restart restyles. Layout cancels a stylo set only for a rooted
    node (one that has had CSS animations or transitions while its set
    existed; it stays rooted while its set is non-empty or an event for it
    is pending) that has stopped rendering. A script-only target is never
    rooted, so it keeps its entry and only its progress is set on each
    tick. A rooted target loses its set, script animations included, in the
    reflow after it stops rendering. The next tick pushes its keyframes
    again, and once its cancel events are sent it is unrooted and behaves
    like a script-only target. A set that still holds a finished transition
    stays non-empty, so that target stays rooted, is dirtied by the
    animation manager and pays one keyframe rebuild per tick while hidden.
    Accepted: a running animation on a `display: none` target that is not
    rooted keeps its progress set each tick, without a restyle, until it
    finishes. Showing the target again is a display change, which triggers
    a rendering update, and that update samples script animations before
    reflow, so the value is current after the next frame. A synchronous
    `getComputedStyle` read still sees the previous tick (see "Sampling is
    per tick"). The script side has no hook for display changes.

- **`<select>` value setter reads the wrong node.** Upstream
  [servo/servo#48109][48109] ("servoshell release build segfaults
  (0xC0000005) on Wikipedia"), open, with Paladin's analysis in the
  comments. In a Windows x86_64 release build without LTO (rustc 1.97.1,
  Servo 0.7.0), `HTMLSelectElement::SetValue` calls
  `HTMLOptionElement::Value` on each option's *next sibling* instead of the
  option: the inlined `list_of_options` (`children_unrooted` + `flat_map` +
  `Choice3` + `iter::once`) loads the sibling before the type check and
  keeps that register as the item. When the sibling is a whitespace text
  node the read of `Element::attrs` through it is an access violation and
  the process dies; when it is the next option the assignment silently does
  nothing. The `value` getter and `selectedIndex` setter inline the same
  iterator and are compiled correctly, and a standalone program with the
  same shape is also correct, so whether this is an LLVM bug or UB in the
  `UnrootedDom`/`NoGC` path is unknown. wikipedia.org sets
  `searchLanguage.value` on DOMContentLoaded, which is why it crashed on
  Windows a few seconds after load; macOS builds were not tested for it.
  Carried as `thomasmost/servo` `paladin/0.7.0` "script: iterate a select's
  options without flat_map", which replaces the adapter chain with a
  hand-written iterator. Reproduce with a page holding a `<select>` with two
  options on separate lines and `s.value = 'b'` on DOMContentLoaded. On each
  Servo bump re-test that page on Windows before dropping the patch, even
  if upstream has rewritten `list_of_options`. Building with Servo's
  `production` profile settings (`lto = true`, `codegen-units = 1`,
  `opt-level = "s"`) also hides the bug on the unpatched 0.7.0 (a 105 MB
  instead of 142 MB binary, 30 minutes instead of 12 to build on a 16-core
  desktop); Paladin keeps the plain release profile and the patch.

## Resolved

Entries move here once upstream has shipped the fix and Paladin has picked
it up. None yet.

[36950]: https://github.com/servo/servo/issues/36950
[44174]: https://github.com/servo/servo/issues/44174
[13263]: https://github.com/servo/servo/issues/13263
[13702]: https://github.com/servo/servo/issues/13702
[13925]: https://github.com/servo/servo/issues/13925
[40444]: https://github.com/servo/servo/issues/40444
[43888]: https://github.com/servo/servo/pull/43888
[44426]: https://github.com/servo/servo/issues/44426
[46562]: https://github.com/servo/servo/issues/46562
[47126]: https://github.com/servo/servo/issues/47126
[47370]: https://github.com/servo/servo/issues/47370
[48109]: https://github.com/servo/servo/issues/48109
[159105]: https://github.com/rust-lang/rust/issues/159105
[151184]: https://github.com/rust-lang/rust/issues/151184
