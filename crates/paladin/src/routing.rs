//! The window's routing decisions as pure functions and small state
//! machines: which side gets a pointer event, which side gets a key event,
//! and where the page area sits in physical pixels. The window reads egui
//! and winit state and feeds it in here, so every rule has a unit test.

use std::collections::HashSet;

use egui::Rect;
use keyboard_types::Code;
use paladin_engine::PhysicalSize;
use winit::event::MouseButton;

/// Who receives a pointer event: the chrome (egui) or the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerTarget {
    Egui,
    Page,
}

/// What the window knows about the pointer's position and egui's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerContext {
    /// The pointer is above the page area, or its position is unknown.
    pub over_chrome: bool,
    /// egui draws the page area itself (the crash panel).
    pub content_is_egui: bool,
    /// The pointer is over an interactable egui layer (the error overlay).
    pub over_egui: bool,
    /// A popup is open, so a click outside it must reach egui to close it.
    pub popup_open: bool,
}

/// The side that gets a pointer event. A capture (set by a press) wins over
/// everything else; otherwise egui owns the pointer over the chrome and over
/// any egui layer or open popup, and the page owns the rest.
pub fn route_pointer(captured: Option<PointerTarget>, ctx: PointerContext) -> PointerTarget {
    if let Some(captured) = captured {
        return captured;
    }
    if ctx.over_chrome || ctx.content_is_egui || ctx.over_egui || ctx.popup_open {
        PointerTarget::Egui
    } else {
        PointerTarget::Page
    }
}

/// Pointer capture: the side that took the first button press gets every
/// pointer event until the last pressed button is released, so a drag that
/// crosses the toolbar edge ends where it started, and holding a second
/// button does not hand the pointer over in the middle.
#[derive(Debug, Default)]
pub struct PointerCapture {
    pressed: HashSet<MouseButton>,
    target: Option<PointerTarget>,
}

impl PointerCapture {
    /// The side holding the capture, if any.
    pub fn captured(&self) -> Option<PointerTarget> {
        self.target
    }

    /// A button went down while `current` was the pointer's side. Returns
    /// the side that gets this press.
    pub fn press(&mut self, button: MouseButton, current: PointerTarget) -> PointerTarget {
        if self.pressed.is_empty() {
            self.target = Some(current);
        }
        self.pressed.insert(button);
        self.target.unwrap_or(current)
    }

    /// A button went up while `current` was the pointer's side. Returns the
    /// side that gets this release. The capture ends with the last button.
    pub fn release(&mut self, button: MouseButton, current: PointerTarget) -> PointerTarget {
        let target = self.target.unwrap_or(current);
        self.pressed.remove(&button);
        if self.pressed.is_empty() {
            self.target = None;
        }
        target
    }

    /// Forget all buttons, for example when the window loses focus and the
    /// releases will arrive elsewhere.
    pub fn clear(&mut self) {
        self.pressed.clear();
        self.target = None;
    }
}

/// Which side a key event goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyTarget {
    Egui,
    Page,
}

/// Decides where key events go and remembers which side received each
/// press, so that side also receives the repeats and the release. The page
/// never sees half a key stroke (for example a key pressed in another window
/// before focus moved here), and egui never misses the release of a key it
/// saw go down (after Enter submits the address bar, the field loses focus
/// before the Enter release arrives).
#[derive(Debug, Default)]
pub struct KeyRouter {
    page_keys: HashSet<Code>,
    egui_keys: HashSet<Code>,
}

impl KeyRouter {
    /// Route one key event. `None` means neither side sees it.
    pub fn route(
        &mut self,
        code: Code,
        pressed: bool,
        repeat: bool,
        egui_wants_keyboard: bool,
    ) -> Option<KeyTarget> {
        if !pressed {
            // A release goes to the side that got the press, whoever has
            // focus now.
            if self.page_keys.remove(&code) {
                return Some(KeyTarget::Page);
            }
            if self.egui_keys.remove(&code) {
                return Some(KeyTarget::Egui);
            }
            // A release nobody here saw pressed is only useful to egui while
            // it has focus, so egui gets it then and nothing otherwise.
            return egui_wants_keyboard.then_some(KeyTarget::Egui);
        }
        // A press or repeat of a key the page holds stays with the page.
        if self.page_keys.contains(&code) {
            return Some(KeyTarget::Page);
        }
        if egui_wants_keyboard || self.egui_keys.contains(&code) {
            self.egui_keys.insert(code);
            return Some(KeyTarget::Egui);
        }
        // A repeat with no press from this window is dropped: the page must
        // not start a key stroke midway.
        if repeat {
            return None;
        }
        self.page_keys.insert(code);
        Some(KeyTarget::Page)
    }

    /// Forget every key, for example when the window loses focus. egui does
    /// the same for its own held keys on `WindowFocused(false)`.
    pub fn clear(&mut self) {
        self.page_keys.clear();
        self.egui_keys.clear();
    }
}

/// The page area in physical pixels: its top edge, which pointer routing
/// compares against, and its size. Both edges are rounded to whole pixels
/// first and the size is the difference, so the size covers exactly the
/// pixel rows and columns the blit paints.
pub fn content_pixels(rect: Rect, pixels_per_point: f32) -> (f32, PhysicalSize<u32>) {
    let left = (rect.min.x * pixels_per_point).round();
    let right = (rect.max.x * pixels_per_point).round();
    let top = (rect.min.y * pixels_per_point).round();
    let bottom = (rect.max.y * pixels_per_point).round();
    let size = PhysicalSize::new(
        (right - left).max(1.0) as u32,
        (bottom - top).max(1.0) as u32,
    );
    (top, size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const LEFT: MouseButton = MouseButton::Left;
    const RIGHT: MouseButton = MouseButton::Right;

    fn ctx() -> PointerContext {
        PointerContext {
            over_chrome: false,
            content_is_egui: false,
            over_egui: false,
            popup_open: false,
        }
    }

    #[test]
    fn pointer_over_page_goes_to_page() {
        assert_eq!(route_pointer(None, ctx()), PointerTarget::Page);
    }

    #[test]
    fn pointer_over_chrome_goes_to_egui() {
        let c = PointerContext {
            over_chrome: true,
            ..ctx()
        };
        assert_eq!(route_pointer(None, c), PointerTarget::Egui);
    }

    #[test]
    fn egui_layers_popups_and_crash_panel_take_the_pointer() {
        for c in [
            PointerContext {
                over_egui: true,
                ..ctx()
            },
            PointerContext {
                popup_open: true,
                ..ctx()
            },
            PointerContext {
                content_is_egui: true,
                ..ctx()
            },
        ] {
            assert_eq!(route_pointer(None, c), PointerTarget::Egui, "{c:?}");
        }
    }

    #[test]
    fn a_capture_overrides_the_position() {
        let over_chrome = PointerContext {
            over_chrome: true,
            ..ctx()
        };
        assert_eq!(
            route_pointer(Some(PointerTarget::Page), over_chrome),
            PointerTarget::Page
        );
        assert_eq!(
            route_pointer(Some(PointerTarget::Egui), ctx()),
            PointerTarget::Egui
        );
    }

    #[test]
    fn press_and_release_keep_one_side_for_the_whole_click() {
        let mut capture = PointerCapture::default();
        assert_eq!(
            capture.press(LEFT, PointerTarget::Page),
            PointerTarget::Page
        );
        // Dragging onto the toolbar: the pointer is now egui's, but the
        // capture keeps the release with the page.
        let now = route_pointer(
            capture.captured(),
            PointerContext {
                over_chrome: true,
                ..ctx()
            },
        );
        assert_eq!(now, PointerTarget::Page);
        assert_eq!(capture.release(LEFT, now), PointerTarget::Page);
        assert_eq!(capture.captured(), None);
    }

    /// The reviewer's scenario: left pressed over the page, right pressed
    /// and released, pointer moved over the toolbar, left released over the
    /// toolbar. The page must see the left release, so right's release must
    /// not end the capture.
    #[test]
    fn releasing_one_button_does_not_end_a_capture_held_by_another() {
        let mut capture = PointerCapture::default();
        assert_eq!(
            capture.press(LEFT, PointerTarget::Page),
            PointerTarget::Page
        );

        // Right press and release while the pointer is still over the page.
        assert_eq!(
            capture.press(RIGHT, PointerTarget::Page),
            PointerTarget::Page
        );
        assert_eq!(
            capture.release(RIGHT, PointerTarget::Page),
            PointerTarget::Page
        );
        assert_eq!(capture.captured(), Some(PointerTarget::Page));

        // The pointer moves over the toolbar with left still held.
        let over_toolbar = PointerContext {
            over_chrome: true,
            ..ctx()
        };
        assert_eq!(
            route_pointer(capture.captured(), over_toolbar),
            PointerTarget::Page
        );

        // Left released over the toolbar still goes to the page.
        let now = route_pointer(capture.captured(), over_toolbar);
        assert_eq!(capture.release(LEFT, now), PointerTarget::Page);
        assert_eq!(capture.captured(), None);
        // After the last release the toolbar owns the pointer again.
        assert_eq!(
            route_pointer(capture.captured(), over_toolbar),
            PointerTarget::Egui
        );
    }

    #[test]
    fn a_second_button_pressed_over_the_chrome_joins_the_page_capture() {
        let mut capture = PointerCapture::default();
        capture.press(LEFT, PointerTarget::Page);
        // A right press while the pointer is over the chrome still belongs
        // to the page capture held by the left button.
        assert_eq!(
            capture.press(RIGHT, PointerTarget::Egui),
            PointerTarget::Page
        );
        assert_eq!(
            capture.release(RIGHT, PointerTarget::Egui),
            PointerTarget::Page
        );
        assert_eq!(
            capture.release(LEFT, PointerTarget::Egui),
            PointerTarget::Page
        );
        assert_eq!(capture.captured(), None);
    }

    #[test]
    fn a_release_without_a_press_here_uses_the_current_side() {
        let mut capture = PointerCapture::default();
        assert_eq!(
            capture.release(LEFT, PointerTarget::Egui),
            PointerTarget::Egui
        );
        assert_eq!(capture.captured(), None);
    }

    #[test]
    fn clearing_forgets_every_button() {
        let mut capture = PointerCapture::default();
        capture.press(LEFT, PointerTarget::Page);
        capture.clear();
        assert_eq!(capture.captured(), None);
        // A later press starts a fresh capture on whichever side it lands.
        assert_eq!(
            capture.press(RIGHT, PointerTarget::Egui),
            PointerTarget::Egui
        );
    }

    #[test]
    fn page_key_gets_its_repeats_and_release() {
        let mut keys = KeyRouter::default();
        assert_eq!(
            keys.route(Code::KeyA, true, false, false),
            Some(KeyTarget::Page)
        );
        assert_eq!(
            keys.route(Code::KeyA, true, true, false),
            Some(KeyTarget::Page)
        );
        assert_eq!(
            keys.route(Code::KeyA, false, false, false),
            Some(KeyTarget::Page)
        );
        // Released: a later release is no longer the page's.
        assert_eq!(keys.route(Code::KeyA, false, false, false), None);
    }

    #[test]
    fn key_pressed_while_egui_has_focus_goes_to_egui() {
        let mut keys = KeyRouter::default();
        assert_eq!(
            keys.route(Code::KeyA, true, false, true),
            Some(KeyTarget::Egui)
        );
        assert_eq!(
            keys.route(Code::KeyA, true, true, true),
            Some(KeyTarget::Egui)
        );
        assert_eq!(
            keys.route(Code::KeyA, false, false, true),
            Some(KeyTarget::Egui)
        );
    }

    /// The reviewer's Enter case: egui gets Enter, the address bar loses
    /// focus on that press, and the release must still reach egui.
    #[test]
    fn egui_gets_the_release_after_it_lost_focus() {
        let mut keys = KeyRouter::default();
        assert_eq!(
            keys.route(Code::Enter, true, false, true),
            Some(KeyTarget::Egui)
        );
        assert_eq!(
            keys.route(Code::Enter, false, false, false),
            Some(KeyTarget::Egui)
        );
        // Nothing left to deliver.
        assert_eq!(keys.route(Code::Enter, false, false, false), None);
    }

    #[test]
    fn repeat_of_an_egui_key_still_goes_to_egui_after_focus_moves() {
        let mut keys = KeyRouter::default();
        keys.route(Code::Backspace, true, false, true);
        assert_eq!(
            keys.route(Code::Backspace, true, true, false),
            Some(KeyTarget::Egui)
        );
    }

    #[test]
    fn page_never_sees_a_repeat_or_release_it_did_not_start() {
        let mut keys = KeyRouter::default();
        // Repeat with no press from this window.
        assert_eq!(keys.route(Code::KeyB, true, true, false), None);
        // A release for a key pressed elsewhere, with no egui focus.
        assert_eq!(keys.route(Code::KeyB, false, false, false), None);
    }

    #[test]
    fn a_key_held_by_the_page_stays_with_the_page_even_if_egui_gets_focus() {
        let mut keys = KeyRouter::default();
        keys.route(Code::KeyC, true, false, false);
        // Focus moved to egui while C is held: the repeat and release stay
        // with the page so it never sees half a stroke.
        assert_eq!(
            keys.route(Code::KeyC, true, true, true),
            Some(KeyTarget::Page)
        );
        assert_eq!(
            keys.route(Code::KeyC, false, false, true),
            Some(KeyTarget::Page)
        );
    }

    #[test]
    fn clearing_forgets_both_sides() {
        let mut keys = KeyRouter::default();
        keys.route(Code::KeyA, true, false, false);
        keys.route(Code::Enter, true, false, true);
        keys.clear();
        assert_eq!(keys.route(Code::KeyA, false, false, false), None);
        assert_eq!(keys.route(Code::Enter, false, false, false), None);
    }

    #[test]
    fn content_size_covers_the_same_pixel_edges_as_the_blit() {
        // Edges 20.4 and 30.6 round to 20 and 31: eleven rows, starting at 20.
        let rect = Rect::from_min_max(pos2(0.5, 20.4), pos2(10.5, 30.6));
        let (top, size) = content_pixels(rect, 1.0);
        assert_eq!(top, 20.0);
        assert_eq!(size, PhysicalSize::new(10, 11));
    }

    #[test]
    fn content_scales_with_pixels_per_point() {
        // 40.2 * 2 = 80.4 -> 80; 300.6 * 2 = 601.2 -> 601.
        let rect = Rect::from_min_max(pos2(0.0, 40.2), pos2(100.0, 300.6));
        let (top, size) = content_pixels(rect, 2.0);
        assert_eq!(top, 80.0);
        assert_eq!(size, PhysicalSize::new(200, 521));
    }

    #[test]
    fn content_is_never_zero_sized() {
        let rect = Rect::from_min_max(pos2(5.0, 5.0), pos2(5.0, 5.0));
        let (_, size) = content_pixels(rect, 1.0);
        assert_eq!(size, PhysicalSize::new(1, 1));
    }
}
