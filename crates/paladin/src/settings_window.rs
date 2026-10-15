//! The settings window: a winit window with egui only (no tabs). It edits
//! `Settings` in place and reports when they should be saved. Phase 2 adds
//! the hotkey and profile editors here.

use egui_glow::EguiGlow;
use paladin_core::{CommandKind, Keymap, Settings};
use paladin_engine::{Engine, EngineError, GlSurface, PhysicalSize};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use crate::icon;
use crate::input::{self, KeyRoute};

pub enum SettingsOutcome {
    Nothing,
    /// An edited field was committed (focus left it, or a checkbox
    /// toggled); the caller should persist the settings.
    Changed,
    /// The user closed the window; the caller should drop it, saving first
    /// if `save` (an edit was still uncommitted).
    Close {
        save: bool,
    },
    /// A keymap chord fired while this window had focus. The caller
    /// handles app-level kinds (quit, new window); window kinds close this
    /// window.
    Shortcut(CommandKind),
}

pub struct SettingsWindow {
    /// A text field was edited and has not lost focus yet.
    dirty: bool,
    modifiers: ModifiersState,
    egui: EguiGlow,
    surface: GlSurface,
    // Dropped last, after the GL state that depends on it.
    window: Window,
}

/// What one frame of the form did.
#[derive(Default)]
struct FormActivity {
    /// Some text changed under the cursor.
    edited: bool,
    /// A field lost focus or a checkbox toggled: time to save.
    committed: bool,
}

impl SettingsWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        _engine: &Engine,
    ) -> Result<SettingsWindow, EngineError> {
        let attributes = Window::default_attributes()
            .with_title("Paladin Settings")
            .with_inner_size(LogicalSize::new(480.0, 320.0))
            .with_min_inner_size(LogicalSize::new(320.0, 200.0))
            .with_window_icon(icon::window_icon());
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| EngineError(format!("creating settings window: {e}")))?;
        let size = window.inner_size();
        let size = PhysicalSize::new(size.width.max(1), size.height.max(1));
        let display_handle = event_loop
            .display_handle()
            .map_err(|e| EngineError(format!("display handle: {e}")))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| EngineError(format!("window handle: {e}")))?;
        let surface = GlSurface::new(display_handle, window_handle, size)?;
        surface.make_current()?;
        let egui = EguiGlow::new(event_loop, surface.gl(), None, None, false);
        Ok(SettingsWindow {
            dirty: false,
            modifiers: ModifiersState::default(),
            egui,
            surface,
            window,
        })
    }

    pub fn winit_id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    pub fn focus(&self) {
        self.window.focus_window();
    }

    /// True if an edit has not been saved yet.
    pub fn needs_save(&self) -> bool {
        self.dirty
    }

    pub fn handle_event(
        &mut self,
        event: &WindowEvent,
        settings: &mut Settings,
        keymap: &Keymap,
    ) -> SettingsOutcome {
        match event {
            WindowEvent::CloseRequested => {
                return SettingsOutcome::Close { save: self.dirty };
            }
            WindowEvent::Resized(size) => {
                self.surface
                    .resize(PhysicalSize::new(size.width.max(1), size.height.max(1)));
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let activity = self.frame(settings);
                if activity.edited {
                    self.dirty = true;
                }
                return if activity.committed && self.dirty {
                    self.dirty = false;
                    SettingsOutcome::Changed
                } else {
                    SettingsOutcome::Nothing
                };
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                let key_input = crate::keyutils::key_input_from_winit(key, self.modifiers);
                let route = input::route_key(
                    self.egui.egui_ctx.egui_wants_keyboard_input(),
                    key.state == ElementState::Pressed,
                    key.repeat,
                    key_input.modifiers,
                    key_input.code,
                    keymap,
                );
                if let KeyRoute::Command(kind) = route {
                    return SettingsOutcome::Shortcut(kind);
                }
            }
            _ => {}
        }
        let response = self.egui.on_window_event(&self.window, event);
        if response.repaint {
            self.window.request_redraw();
        }
        SettingsOutcome::Nothing
    }

    fn frame(&mut self, settings: &mut Settings) -> FormActivity {
        let mut activity = FormActivity::default();
        self.egui.run(&self.window, |ui| {
            activity = draw_settings(ui, settings);
        });
        self.surface.begin_frame();
        self.egui.paint(&self.window);
        self.surface.present();
        if self.egui.egui_ctx.has_requested_repaint() {
            self.window.request_redraw();
        }
        activity
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        let _ = self.surface.make_current();
        self.egui.destroy();
    }
}

/// The settings form.
fn draw_settings(ui: &mut egui::Ui, settings: &mut Settings) -> FormActivity {
    let mut activity = FormActivity::default();
    ui.heading("Settings");
    ui.add_space(8.0);
    egui::Grid::new("settings-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Homepage");
            let homepage =
                ui.add(egui::TextEdit::singleline(&mut settings.homepage).desired_width(300.0));
            activity.edited |= homepage.changed();
            activity.committed |= homepage.lost_focus();
            ui.end_row();

            ui.label("New tab URL");
            let new_tab =
                ui.add(egui::TextEdit::singleline(&mut settings.new_tab_url).desired_width(300.0));
            activity.edited |= new_tab.changed();
            activity.committed |= new_tab.lost_focus();
            ui.end_row();

            ui.label("On launch");
            let restore = ui.checkbox(
                &mut settings.restore_windows_on_launch,
                "Restore previous windows (not yet implemented)",
            );
            if restore.changed() {
                activity.edited = true;
                activity.committed = true;
            }
            ui.end_row();
        });
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new("Keyboard shortcuts and profiles arrive in Phase 2.")
            .small()
            .weak(),
    );
    activity
}
