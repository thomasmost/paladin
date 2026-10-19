//! Servo glue. This is the only crate in the workspace that imports the
//! `servo` crate. It implements the `osmena_engine` traits; everything that
//! crosses into the shell is expressed in osmena's types: core ids,
//! `EngineEvent`s, and `EngineInput`. Servo's `WebView`s, ids, and delegate
//! traits never leave this crate.
//!
//! - [`Engine`] builds Servo once per process and owns the event queue.
//! - [`GlSurface`] wraps Servo's GL context on a native window; the shell's
//!   egui shares it.
//! - [`TabHost`] owns the tabs of one window, painting them into an
//!   offscreen target that the shell blits under the toolbar
//!   (`PagePaint::Blit`).

mod delegate;
mod engine;
mod host;
mod input;

pub use osmena_engine::{EngineError, PhysicalSize, Waker};

pub use crate::engine::{Engine, INFO, run_content_process};
pub use crate::host::{GlSurface, TabHost};
