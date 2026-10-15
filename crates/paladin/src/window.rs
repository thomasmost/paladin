//! A browser window: a winit window, the engine's tab host for it, and an
//! egui context sharing the engine's GL context.

use std::collections::HashSet;
use std::sync::Arc;

use egui::{LayerId, PaintCallback};
use egui_glow::{CallbackFn, EguiGlow};
use keyboard_types::Code;
use paladin_core::{Browser, Command, WindowId};
use paladin_engine::{
    BlitRect, Engine, EngineError, EngineInput, GlSurface, PhysicalSize, TabHost,
};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use crate::chrome::{self, ChromeState};
use crate::{icon, input};

/// Who receives pointer events: the chrome (egui) or the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PointerTarget {
    Egui,
    Page,
}

pub struct BrowserWindow {
    id: WindowId,
    chrome: ChromeState,
    modifiers: ModifiersState,
    cursor: Option<PhysicalPosition<f64>>,
    /// Physical-pixel height of the chrome above the page, from the last
    /// frame. Events below it go to the page, above it to egui.
    content_top_px: f32,
    /// True while egui draws the page area itself (the crash panel).
    content_is_egui: bool,
    pointer_in_content: bool,
    /// Set on a button press, cleared on release: the side that took the
    /// press gets every pointer event until then, so a drag that crosses
    /// the toolbar edge still ends where it started.
    pointer_capture: Option<PointerTarget>,
    /// Keys whose press went to the page. Only their repeats and releases
    /// go to the page too, so it never sees half a key stroke, for example
    /// from a key pressed in another window before focus moved here.
    page_keys: HashSet<Code>,
    egui: EguiGlow,
    host: TabHost,
    // Dropped last: the tab host and egui hold GL state tied to it.
    window: Window,
}

impl BrowserWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        engine: &Engine,
        id: WindowId,
    ) -> Result<BrowserWindow, EngineError> {
        let attributes = Window::default_attributes()
            .with_title("Paladin")
            .with_inner_size(LogicalSize::new(1024.0, 768.0))
            .with_min_inner_size(LogicalSize::new(400.0, 300.0))
            .with_window_icon(icon::window_icon());
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| EngineError(format!("creating window: {e}")))?;

        let size = non_zero(window.inner_size());
        let display_handle = event_loop
            .display_handle()
            .map_err(|e| EngineError(format!("display handle: {e}")))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| EngineError(format!("window handle: {e}")))?;
        let surface = GlSurface::new(display_handle, window_handle, size)?;
        surface.make_current()?;
        let host = TabHost::new(engine, surface, window.scale_factor() as f32)?;
        let gl = host.surface().expect("headed tab host has a surface").gl();
        let egui = EguiGlow::new(event_loop, gl, None, None, false);
        egui.egui_ctx.options_mut(|options| {
            options.zoom_with_keyboard = false;
        });

        Ok(BrowserWindow {
            id,
            chrome: ChromeState::default(),
            modifiers: ModifiersState::default(),
            cursor: None,
            content_top_px: 0.0,
            content_is_egui: false,
            pointer_in_content: false,
            pointer_capture: None,
            page_keys: HashSet::new(),
            egui,
            host,
            window,
        })
    }

    pub fn core_id(&self) -> WindowId {
        self.id
    }

    pub fn winit_id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    pub fn host(&self) -> &TabHost {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut TabHost {
        &mut self.host
    }

    pub fn hosts_tab(&self, tab: paladin_core::TabId) -> bool {
        self.host.has_tab(tab)
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn focus_address_bar(&mut self) {
        self.chrome.focus_address = true;
        self.window.request_redraw();
    }

    fn surface(&self) -> &GlSurface {
        self.host.surface().expect("headed tab host has a surface")
    }

    /// Who the pointer belongs to right now: a captured side during a
    /// drag; otherwise egui when the pointer is over the chrome, over an
    /// interactable egui layer (the error overlay), while a popup is open
    /// (so a click outside closes it), or while the crash panel replaces
    /// the page; otherwise the page.
    fn pointer_target(&self) -> PointerTarget {
        if let Some(captured) = self.pointer_capture {
            return captured;
        }
        let over_chrome = match self.cursor {
            Some(p) => (p.y as f32) < self.content_top_px,
            None => true,
        };
        let egui_owns = over_chrome
            || self.content_is_egui
            || self.egui.egui_ctx.is_pointer_over_egui()
            || egui::Popup::is_any_open(&self.egui.egui_ctx);
        if egui_owns {
            PointerTarget::Egui
        } else {
            PointerTarget::Page
        }
    }

    /// Cursor position in physical pixels relative to the page area.
    fn content_point(&self) -> Option<(f32, f32)> {
        let p = self.cursor?;
        Some((p.x as f32, p.y as f32 - self.content_top_px))
    }

    /// Handle one winit event. Returns the commands the user issued.
    pub fn handle_event(&mut self, browser: &Browser, event: &WindowEvent) -> Vec<Command> {
        let active = browser.window(self.id).map(|w| w.active);
        let commands = Vec::new();
        let mut forward_to_egui = true;

        match event {
            WindowEvent::Resized(size) => {
                self.surface().resize(non_zero(*size));
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.host.set_hidpi(*scale_factor as f32);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                return self.frame(browser);
            }
            WindowEvent::CloseRequested => {
                return vec![Command::CloseWindow(self.id)];
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    // Releases of keys held while focus moved away go to
                    // the other window; do not wait for them here.
                    self.page_keys.clear();
                    self.pointer_capture = None;
                }
                if let Some(tab) = active {
                    self.host.set_focused(tab, *focused);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                // egui always sees cursor movement so it can hide tooltips
                // and hover state when the pointer leaves the chrome.
                self.cursor = Some(*position);
                if self.pointer_target() == PointerTarget::Page {
                    if let (Some(tab), Some((x, y))) = (active, self.content_point()) {
                        self.pointer_in_content = true;
                        self.host.input(tab, EngineInput::MouseMove { x, y });
                    }
                } else if self.pointer_in_content {
                    self.pointer_in_content = false;
                    if let Some(tab) = active {
                        self.host.input(tab, EngineInput::MouseLeft);
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.pointer_in_content {
                    self.pointer_in_content = false;
                    if let Some(tab) = active {
                        self.host.input(tab, EngineInput::MouseLeft);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let target = self.pointer_target();
                self.pointer_capture = match state {
                    ElementState::Pressed => Some(target),
                    ElementState::Released => None,
                };
                if target == PointerTarget::Page {
                    forward_to_egui = false;
                    self.surrender_egui_focus();
                    if let (Some(tab), Some((x, y))) = (active, self.content_point()) {
                        self.host.input(
                            tab,
                            EngineInput::MouseButton {
                                action: input::button_action(*state),
                                button: input::mouse_button(*button),
                                x,
                                y,
                            },
                        );
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.pointer_target() == PointerTarget::Page {
                    forward_to_egui = false;
                    if let (Some(tab), Some((x, y))) = (active, self.content_point()) {
                        let (dx, dy) = input::wheel_delta(*delta);
                        self.host.input(tab, EngineInput::Wheel { dx, dy, x, y });
                    }
                }
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                let key_input = crate::keyutils::key_input_from_winit(key, self.modifiers);
                let pressed = key.state == ElementState::Pressed;
                // A release of a key the page is holding goes to the page
                // whoever has focus now, so it never sees half a stroke.
                if !pressed && self.page_keys.remove(&key_input.code) {
                    forward_to_egui = false;
                    if let Some(tab) = active {
                        self.host.input(tab, EngineInput::Key(key_input));
                    }
                } else if !self.egui.egui_ctx.egui_wants_keyboard_input() {
                    forward_to_egui = false;
                    // Releases were handled above; a repeat is delivered
                    // only while the page holds the key.
                    let deliver = if !pressed {
                        false
                    } else if key.repeat {
                        self.page_keys.contains(&key_input.code)
                    } else {
                        self.page_keys.insert(key_input.code);
                        true
                    };
                    if deliver && let Some(tab) = active {
                        self.host.input(tab, EngineInput::Key(key_input));
                    }
                }
            }
            WindowEvent::Ime(ime) if !self.egui.egui_ctx.egui_wants_keyboard_input() => {
                forward_to_egui = false;
                if let (Some(tab), Some(input)) = (active, input::ime(ime)) {
                    self.host.input(tab, input);
                }
            }
            _ => {}
        }

        if forward_to_egui {
            let response = self.egui.on_window_event(&self.window, event);
            if response.repaint {
                self.window.request_redraw();
            }
        }
        commands
    }

    fn surrender_egui_focus(&self) {
        self.egui.egui_ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    /// Draw the chrome, paint the active tab offscreen, blit it under the
    /// toolbar, and present.
    fn frame(&mut self, browser: &Browser) -> Vec<Command> {
        let Some(model) = browser.window(self.id) else {
            return Vec::new();
        };
        let active = model.active;
        let crashed = model.active_tab().crashed.clone();
        let engine_error = browser.engine_error.as_deref();
        let mut commands = Vec::new();
        let mut content_top_px = 0.0;

        let Self {
            egui,
            host,
            chrome: chrome_state,
            window,
            ..
        } = self;
        egui.run(window, |ui| {
            commands = chrome::draw(ui, model, engine_error, chrome_state);
            let rect = ui.available_rect_before_wrap();
            let ppp = ui.pixels_per_point();
            content_top_px = rect.min.y * ppp;
            let size = PhysicalSize::new(
                (rect.width() * ppp).round().max(1.0) as u32,
                (rect.height() * ppp).round().max(1.0) as u32,
            );
            host.resize_content(active, size);
            match &crashed {
                Some(reason) => {
                    if chrome::draw_crash_panel(ui, rect, reason) {
                        commands.push(Command::Reload(active));
                    }
                }
                None => {
                    host.paint(active);
                    // Fetched after resize and paint: the callback captures
                    // the offscreen framebuffer, which a resize recreates.
                    if let Some(blit) = host.blit_callback() {
                        ui.ctx()
                            .layer_painter(LayerId::background())
                            .add(PaintCallback {
                                rect,
                                callback: Arc::new(CallbackFn::new(move |info, painter| {
                                    let v = info.viewport_in_pixels();
                                    blit(
                                        painter.gl(),
                                        BlitRect {
                                            x: v.left_px,
                                            y_from_bottom: v.from_bottom_px,
                                            width: v.width_px,
                                            height: v.height_px,
                                        },
                                    );
                                })),
                            });
                    }
                }
            }
        });
        self.content_top_px = content_top_px;
        self.content_is_egui = crashed.is_some();

        let surface = self.host.surface().expect("headed tab host has a surface");
        surface.begin_frame();
        self.egui.paint(&self.window);
        surface.present();
        if self.egui.egui_ctx.has_requested_repaint() {
            self.window.request_redraw();
        }
        commands
    }
}

impl Drop for BrowserWindow {
    fn drop(&mut self) {
        if let Some(surface) = self.host.surface() {
            let _ = surface.make_current();
        }
        self.egui.destroy();
    }
}

/// GL surfaces reject zero-sized targets; a minimized window reports 0x0.
fn non_zero(size: winit::dpi::PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}
