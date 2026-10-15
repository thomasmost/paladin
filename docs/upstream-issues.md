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
    (bsky.app): [S13](#s13-web-animations-api-unimplemented).
  - Clean stderr on exit: [S6](#s6-memory-profiler-warnings-on-exit).
  - Quiet `cargo build` on macOS:
    [T1](#t1-__eh_frame-section-too-large-linker-warning-on-macos).
  - Style-thread stack overflow on wikipedia.org: resolved by
    [S5](#s5-style-threads-overflow-their-512-kb-stack-resolved-in-paladin).
  - Packaging on both hosts: worked around in
    [T2](#t2-cargo-packager-0118-fails-on-formats-the-host-cannot-build).
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

### S13: Web Animations API unimplemented

- **Upstream:** [servo/servo#36950][36950] (open, "Implement
  WebAnimations"; a contributor has an in-progress branch since 2026-04);
  [servo/servo#44174][44174] (open) is the bsky.app report.
- **What it is:** `Element.animate()`, `Animation`, and `KeyframeEffect`
  sit behind the `dom_web_animations_enabled` pref, which is off by
  default (servo-config-0.7.0, `prefs.rs`). With it off, `element.animate`
  is undefined. With it on, the interface is a skeleton:
  `Animation.webidl` comments out `play()`, `cancel()`, `finish()`,
  `finished`, `onfinish`, and `currentTime`, and `Element::Animate` skips
  the step that plays the animation (servo-script-0.7.0,
  `dom/element/element.rs`). `getAnimations()` is missing too.
- **Effect:** pages that call `element.animate()` throw `t.animate is not a
  function`, which Paladin logs as an ERROR from
  `paladin_engine::delegate`. bsky.app breaks in a critical path; the
  upstream issues also name airbnb.com and beatleader.com.
- **Workaround:** none. Turning the pref on only moves the exception to the
  next missing member and defeats sites that feature-detect `animate` and
  fall back to CSS. Injecting the web-animations-js polyfill through
  `UserContentManager` is possible (suggested in #44174) but is feature
  work for one API.
- **Blocks:** nothing on the roadmap; a README Known limitations entry.
- **When it lands:** check whether Servo enables the pref by default; if
  not, decide whether to set it in `Engine`'s `Preferences`. Load bsky.app
  and remove the README entry.

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

## Carried in the fork

Patches on `thomasmost/servo` (`paladin/0.7.0`) and `thomasmost/stylo`
(`paladin/0.22.0`), each listed against the upstream issue it addresses.

- **S13 (Web Animations API), partial.** `thomasmost/servo`
  `paladin/0.7.0`: "script: add the Web Animations timing calculations",
  "script: add AnimationPlaybackEvent", "script: implement the Animation
  timing model", "script: finish Element.animate()", "script: request a
  rendering update for queued animation events", "script: tick script
  animations on their own timeline", "script: use expect for the unused
  sequence number", "script: tidy the simple iteration progress check and
  event queue note", "script: clarify why the animation sequence number is
  unused". Timing model only; sampling follows in later patches.
  Upstream: servo/servo#36950.

## Resolved

Entries move here once upstream has shipped the fix and Paladin has picked
it up. None yet.

[13263]: https://github.com/servo/servo/issues/13263
[13702]: https://github.com/servo/servo/issues/13702
[13925]: https://github.com/servo/servo/issues/13925
[36950]: https://github.com/servo/servo/issues/36950
[40444]: https://github.com/servo/servo/issues/40444
[43888]: https://github.com/servo/servo/pull/43888
[44174]: https://github.com/servo/servo/issues/44174
[44426]: https://github.com/servo/servo/issues/44426
[46562]: https://github.com/servo/servo/issues/46562
[47126]: https://github.com/servo/servo/issues/47126
[47370]: https://github.com/servo/servo/issues/47370
[159105]: https://github.com/rust-lang/rust/issues/159105
