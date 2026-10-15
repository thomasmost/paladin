//! winit input to engine input. Coordinates are converted by the window
//! (toolbar offset, physical pixels); this module maps the enums.

use paladin_engine::{ButtonAction, EngineInput, MouseButton};
use winit::event::{ElementState, Ime, MouseScrollDelta};

/// Pixels per wheel "line", the value Servo's own shell uses.
const LINE_HEIGHT_PX: f32 = 76.0;

pub fn mouse_button(button: winit::event::MouseButton) -> MouseButton {
    match button {
        winit::event::MouseButton::Left => MouseButton::Primary,
        winit::event::MouseButton::Right => MouseButton::Secondary,
        winit::event::MouseButton::Middle => MouseButton::Auxiliary,
        winit::event::MouseButton::Back => MouseButton::Back,
        winit::event::MouseButton::Forward => MouseButton::Forward,
        winit::event::MouseButton::Other(n) => MouseButton::Other(n),
    }
}

pub fn button_action(state: ElementState) -> ButtonAction {
    match state {
        ElementState::Pressed => ButtonAction::Down,
        ElementState::Released => ButtonAction::Up,
    }
}

/// Wheel delta in pixels, `(dx, dy)`.
pub fn wheel_delta(delta: MouseScrollDelta) -> (f64, f64) {
    match delta {
        MouseScrollDelta::LineDelta(dx, dy) => {
            ((dx * LINE_HEIGHT_PX) as f64, (dy * LINE_HEIGHT_PX) as f64)
        }
        MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
    }
}

pub fn ime(ime: &Ime) -> Option<EngineInput> {
    match ime {
        Ime::Enabled => Some(EngineInput::ImeStart),
        Ime::Preedit(text, _) => Some(EngineInput::ImePreedit(text.clone())),
        Ime::Commit(text) => Some(EngineInput::ImeCommit(text.clone())),
        Ime::Disabled => Some(EngineInput::ImeDismissed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_deltas_scale_to_pixels_and_pixel_deltas_pass_through() {
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, -1.0)),
            (0.0, -76.0)
        );
        assert_eq!(
            wheel_delta(MouseScrollDelta::PixelDelta(
                winit::dpi::PhysicalPosition::new(3.0, 4.0)
            )),
            (3.0, 4.0)
        );
    }

    #[test]
    fn buttons_map_one_to_one() {
        assert_eq!(
            mouse_button(winit::event::MouseButton::Left),
            MouseButton::Primary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Right),
            MouseButton::Secondary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Middle),
            MouseButton::Auxiliary
        );
        assert_eq!(
            mouse_button(winit::event::MouseButton::Other(7)),
            MouseButton::Other(7)
        );
        assert_eq!(button_action(ElementState::Pressed), ButtonAction::Down);
    }
}
