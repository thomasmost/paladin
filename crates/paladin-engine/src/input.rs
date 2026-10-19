//! Conversion of the browser's input events (`osmena_core::EngineInput`)
//! to Servo's. The shell builds the events from winit; this keeps winit
//! out of the engine and Servo out of the shell.

use osmena_core::{ButtonAction, EngineInput, MouseButton};
use servo::{
    CompositionEvent, CompositionState, DevicePoint, ImeEvent, InputEvent, KeyboardEvent,
    MouseButton as ServoMouseButton, MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent,
    MouseMoveEvent, WheelDelta, WheelEvent, WheelMode,
};

pub(crate) fn to_servo(input: EngineInput) -> InputEvent {
    match input {
        EngineInput::MouseMove { x, y } => {
            InputEvent::MouseMove(MouseMoveEvent::new(DevicePoint::new(x, y).into()))
        }
        EngineInput::MouseButton {
            action,
            button,
            x,
            y,
        } => InputEvent::MouseButton(MouseButtonEvent::new(
            match action {
                ButtonAction::Down => MouseButtonAction::Down,
                ButtonAction::Up => MouseButtonAction::Up,
            },
            match button {
                MouseButton::Primary => ServoMouseButton::Primary,
                MouseButton::Secondary => ServoMouseButton::Secondary,
                MouseButton::Auxiliary => ServoMouseButton::Auxiliary,
                MouseButton::Back => ServoMouseButton::Back,
                MouseButton::Forward => ServoMouseButton::Forward,
                MouseButton::Other(n) => ServoMouseButton::Other(n),
            },
            DevicePoint::new(x, y).into(),
        )),
        EngineInput::MouseLeft => InputEvent::MouseLeftViewport(MouseLeftViewportEvent::default()),
        EngineInput::Wheel { dx, dy, x, y } => InputEvent::Wheel(WheelEvent::new(
            WheelDelta {
                x: dx,
                y: dy,
                z: 0.0,
                mode: WheelMode::DeltaPixel,
            },
            DevicePoint::new(x, y).into(),
        )),
        EngineInput::Key(k) => InputEvent::Keyboard(KeyboardEvent::new_without_event(
            k.state,
            k.key,
            k.code,
            k.location,
            k.modifiers,
            k.repeat,
            false,
        )),
        EngineInput::ImeStart => composition(CompositionState::Start, String::new()),
        EngineInput::ImePreedit(text) => composition(CompositionState::Update, text),
        EngineInput::ImeCommit(text) => composition(CompositionState::End, text),
        EngineInput::ImeDismissed => InputEvent::Ime(ImeEvent::Dismissed),
    }
}

fn composition(state: CompositionState, data: String) -> InputEvent {
    InputEvent::Ime(ImeEvent::Composition(CompositionEvent { state, data }))
}

#[cfg(test)]
mod tests {
    use keyboard_types::{Code, Key, KeyState, Location, Modifiers, NamedKey};
    use servo::{
        CompositionState, ImeEvent, InputEvent, MouseButton as ServoMouseButton, MouseButtonAction,
        WebViewPoint, WheelMode,
    };

    use osmena_core::{ButtonAction, EngineInput, KeyInput, MouseButton};

    use super::to_servo;

    /// The device-pixel position a mapped event carries.
    fn device_point(point: &WebViewPoint) -> (f32, f32) {
        match point {
            WebViewPoint::Device(p) => (p.x, p.y),
            _ => panic!("expected a device-pixel point"),
        }
    }

    fn composition_of(event: InputEvent) -> (CompositionState, String) {
        let InputEvent::Ime(ImeEvent::Composition(composition)) = event else {
            panic!("expected a composition event");
        };
        (composition.state, composition.data)
    }

    #[test]
    fn mouse_move_carries_the_position() {
        let InputEvent::MouseMove(event) = to_servo(EngineInput::MouseMove { x: 12.5, y: 40.0 })
        else {
            panic!("expected MouseMove");
        };
        assert_eq!(device_point(&event.point), (12.5, 40.0));
        assert!(!event.is_compatibility_event_for_touch);
    }

    #[test]
    fn mouse_button_maps_action_button_and_position() {
        let buttons = [
            (MouseButton::Primary, ServoMouseButton::Primary),
            (MouseButton::Secondary, ServoMouseButton::Secondary),
            (MouseButton::Auxiliary, ServoMouseButton::Auxiliary),
            (MouseButton::Back, ServoMouseButton::Back),
            (MouseButton::Forward, ServoMouseButton::Forward),
            (MouseButton::Other(7), ServoMouseButton::Other(7)),
        ];
        let actions = [
            (ButtonAction::Down, MouseButtonAction::Down),
            (ButtonAction::Up, MouseButtonAction::Up),
        ];
        for (button, expected_button) in buttons {
            for (action, expected_action) in actions {
                let input = EngineInput::MouseButton {
                    action,
                    button,
                    x: 3.0,
                    y: 4.0,
                };
                let InputEvent::MouseButton(event) = to_servo(input) else {
                    panic!("expected MouseButton");
                };
                assert_eq!(event.action, expected_action, "{button:?} {action:?}");
                assert_eq!(event.button, expected_button, "{button:?} {action:?}");
                assert_eq!(device_point(&event.point), (3.0, 4.0));
            }
        }
    }

    #[test]
    fn mouse_left_maps_to_leaving_the_viewport() {
        let InputEvent::MouseLeftViewport(event) = to_servo(EngineInput::MouseLeft) else {
            panic!("expected MouseLeftViewport");
        };
        assert!(!event.focus_moving_to_another_iframe);
    }

    #[test]
    fn wheel_maps_pixel_deltas_and_position() {
        let input = EngineInput::Wheel {
            dx: -2.0,
            dy: 120.0,
            x: 5.0,
            y: 6.0,
        };
        let InputEvent::Wheel(event) = to_servo(input) else {
            panic!("expected Wheel");
        };
        assert_eq!(event.delta.x, -2.0);
        assert_eq!(event.delta.y, 120.0);
        assert_eq!(event.delta.z, 0.0);
        assert!(matches!(event.delta.mode, WheelMode::DeltaPixel));
        assert_eq!(device_point(&event.point), (5.0, 6.0));
    }

    #[test]
    fn key_maps_every_field() {
        let input = KeyInput {
            state: KeyState::Down,
            key: Key::Character("a".into()),
            code: Code::KeyA,
            location: Location::Standard,
            modifiers: Modifiers::SHIFT,
            repeat: true,
        };
        let InputEvent::Keyboard(event) = to_servo(EngineInput::Key(input)) else {
            panic!("expected Keyboard");
        };
        let key = &event.event;
        assert_eq!(key.state, KeyState::Down);
        assert_eq!(key.key, Key::Character("a".into()));
        assert_eq!(key.code, Code::KeyA);
        assert_eq!(key.location, Location::Standard);
        assert_eq!(key.modifiers, Modifiers::SHIFT);
        assert!(key.repeat);
        assert!(!key.is_composing);
    }

    #[test]
    fn key_up_keeps_state_and_clears_repeat() {
        let input = KeyInput {
            state: KeyState::Up,
            key: Key::Named(NamedKey::Enter),
            code: Code::Enter,
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            repeat: false,
        };
        let InputEvent::Keyboard(event) = to_servo(EngineInput::Key(input)) else {
            panic!("expected Keyboard");
        };
        assert_eq!(event.event.state, KeyState::Up);
        assert_eq!(event.event.key, Key::Named(NamedKey::Enter));
        assert!(!event.event.repeat);
    }

    #[test]
    fn ime_start_preedit_and_commit_map_to_composition_states() {
        assert_eq!(
            composition_of(to_servo(EngineInput::ImeStart)),
            (CompositionState::Start, String::new())
        );
        assert_eq!(
            composition_of(to_servo(EngineInput::ImePreedit("ni".into()))),
            (CompositionState::Update, "ni".to_owned())
        );
        assert_eq!(
            composition_of(to_servo(EngineInput::ImeCommit("你".into()))),
            (CompositionState::End, "你".to_owned())
        );
    }

    #[test]
    fn ime_dismissed_maps_to_dismissed() {
        assert!(matches!(
            to_servo(EngineInput::ImeDismissed),
            InputEvent::Ime(ImeEvent::Dismissed)
        ));
    }
}
