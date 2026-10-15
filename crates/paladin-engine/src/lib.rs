//! Servo glue. This is the only crate in the workspace that imports the
//! `servo` crate. Everything that crosses into the model or the UI is
//! expressed in Paladin's own types: core ids, [`EngineEvent`]s, and
//! [`EngineInput`]. Servo's `WebView`s, ids, and delegate traits never leave
//! this crate.
//!
//! - [`Engine`] builds Servo once per process and owns the event queue.
//! - [`GlSurface`] wraps Servo's GL context on a native window; the app's
//!   egui shares it.
//! - [`TabHost`] owns the tabs of one window, painting them into an
//!   offscreen target that egui blits under the toolbar.
//!
//! [`EngineEvent`]: paladin_core::EngineEvent

mod delegate;
mod engine;
mod host;
mod input;

pub use dpi::PhysicalSize;
pub use keyboard_types;

pub use crate::engine::{Engine, EngineError, Waker};
pub use crate::host::{BlitFn, BlitRect, GlSurface, TabHost};
pub use crate::input::{ButtonAction, EngineInput, KeyInput, MouseButton};
