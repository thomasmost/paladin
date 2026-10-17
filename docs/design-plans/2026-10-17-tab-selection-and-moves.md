# Tab selection, context menu, and moves

_2026-10-17. Status: implemented._

## Goal

Select several tabs in a window (Ctrl-click to add or remove one, Shift-click
to extend from the active tab), then act on the selection: close the tabs,
move them to another window or a new one by dragging or from a context menu,
or open them in another profile.

## Model

`Window` gains `selected: Vec<TabId>` with the invariant that the active tab
is always selected. The commands that change it:

| Command | Effect |
| --- | --- |
| `ActivateTab(tab)` | `tab` becomes active and the only selected tab (a plain click; also what a new tab and a closed active tab's replacement do). |
| `ToggleTabSelection(tab)` | Adds or removes `tab`; the active tab cannot be removed. |
| `SelectTabRange(tab)` | Selects every tab between the active tab and `tab`, nothing else; the active tab stays. |
| `CloseTabs(tabs)` | `CloseTab` for each. |
| `MoveTabsToNewWindow(tabs)` | Opens a window on the first tab's URL, recreates the rest there, activates the one that was active, closes the originals. |
| `MoveTabsToWindow { tabs, window }` | Recreates the tabs at the end of `window`, same activation rule, closes the originals. Tabs already in `window` are ignored. |
| `OpenTabsInProfile { tabs, profile }` | `EngineAction::OpenInProfile { profile, urls }` then closes the tabs. The current profile is refused. |

Moves recreate rather than move because a Servo `WebView` cannot leave its
rendering context (`docs/upstream-issues.md`, S8): the moved page reloads
and loses its history, scroll position, and form state. `window.rs` and
`app.rs` never touch the selection; `dispatch` owns it, with unit tests in
`command.rs`. Closing a tab drops it from the selection; the engine closing
one (`EngineEvent::TabClosed`) goes through the same `close_tab`.

## Chrome

The tab strip (`chrome.rs`) reads the modifiers on a click and emits the
command above. Selected tabs that are not active get a tinted fill. The
context menu acts on the selection when the right-clicked tab is part of it,
otherwise on that tab alone, which it first activates. Its items: Close
tab(s), Close other tabs, Move to new window, Move to window (a submenu of
the other windows, labelled by their active tab's title), Open in profile (a
submenu of the other profile directories, read from disk when the menu
opens). Items that make no sense are disabled with a hover hint.

A tab is `Sense::click_and_drag()`. Dragging an unselected tab first
activates it, so the drag carries the selection. While dragging, an overlay
near the pointer says how many tabs move. When the drag ends, the chrome
records a `TabDrop` (tabs and the pointer in egui points); the window turns
it into physical screen coordinates and the app decides:

- over another browser window (frame included): `MoveTabsToWindow`;
- outside every browser window: `MoveTabsToNewWindow`;
- still over the source window: nothing (no reordering by drag yet).

The pointer stays captured by the source window during a drag, so no other
window sees the drop; resolving it from window rectangles in the app is what
makes a cross-window drop work without any egui-level drag and drop.

## Profiles

Another profile is another `paladin` process (S7). `Profile::others()` lists
the sibling directories of the current profile; `process::open_in_profile`
runs the current executable with `--profile <name> <urls>`. On Windows the
new browser breaks away from the job object that ties content processes to
the browser that started it, so the two browsers are independent. There is
no single-instance logic: a profile that is already running gets a second
process sharing its data directory.

## Not in scope

Reordering tabs by dragging within a strip, moving a tab with its live page
(S8), dragging between processes (profiles), and a "new profile" entry in the
menu (profiles are created by starting `paladin --profile <name>`).
