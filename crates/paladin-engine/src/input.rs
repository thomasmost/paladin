//! Paladin's input events and their conversion to Servo's. The app builds
//! `EngineInput` from winit events; this keeps winit out of the engine and
//! Servo out of the app.

use keyboard_types::{Code, Key, KeyState, Location, Modifiers};
use servo::{
    CompositionEvent, CompositionState, DevicePoint, ImeEvent, InputEvent, KeyboardEvent,
    MouseButton as ServoMouseButton, MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent,
    MouseMoveEvent, WheelDelta, WheelEvent, WheelMode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Primary,
    Secondary,
    Auxiliary,
    Back,
    Forward,
    Other(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    Down,
    Up,
}

/// A key event in `keyboard-types` terms (the W3C UI Events model).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInput {
    pub state: KeyState,
    pub key: Key,
    pub code: Code,
    pub location: Location,
    pub modifiers: Modifiers,
    pub repeat: bool,
}

/// Input for a tab. Positions are physical pixels relative to the top-left
/// of the page area (below the toolbar).
#[derive(Clone, Debug, PartialEq)]
pub enum EngineInput {
    MouseMove {
        x: f32,
        y: f32,
    },
    MouseButton {
        action: ButtonAction,
        button: MouseButton,
        x: f32,
        y: f32,
    },
    /// The pointer left the page area.
    MouseLeft,
    /// Scroll by `(dx, dy)` pixels with the pointer at `(x, y)`.
    Wheel {
        dx: f64,
        dy: f64,
        x: f32,
        y: f32,
    },
    Key(KeyInput),
    ImeStart,
    ImePreedit(String),
    ImeCommit(String),
    ImeDismissed,
}

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
