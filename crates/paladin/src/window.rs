//! A browser window: a winit window, the engine's tab host for it, and an
//! egui context sharing the engine's GL context.

use std::sync::Arc;

use egui::{LayerId, PaintCallback};
use egui_glow::{CallbackFn, EguiGlow};
use paladin_core::{Browser, Command, Keymap, TabId, WindowId};
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
use crate::input;
use crate::routing::{
    KeyRouter, KeyStroke, KeyTarget, PointerCapture, PointerContext, PointerTarget, content_pixels,
    route_pointer,
};
use crate::{icon, keyutils};

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
    /// Which side holds the pointer during a drag or click. See
    /// [`PointerCapture`].
    capture: PointerCapture,
    /// Which side each held key went to, and which presses are keymap
    /// commands. See [`KeyRouter`].
    keys: KeyRouter,
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
            capture: PointerCapture::default(),
            keys: KeyRouter::default(),
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

    /// The tabs a drag that ended in this window carried, with the drop
    /// point in physical screen pixels (the pointer may have been outside
    /// this window). Taken once. `None` if the window's position is not
    /// known.
    pub fn take_tab_drop(&mut self) -> Option<(Vec<TabId>, PhysicalPosition<f64>)> {
        let drop = self.chrome.take_tab_drop()?;
        let origin = self.window.inner_position().ok()?;
        let scale = self.window.scale_factor();
        Some((
            drop.tabs,
            PhysicalPosition::new(
                f64::from(origin.x) + f64::from(drop.pointer.x) * scale,
                f64::from(origin.y) + f64::from(drop.pointer.y) * scale,
            ),
        ))
    }

    /// Whether `point` (physical screen pixels) is over this window, frame
    /// included. False if the window's position is not known.
    pub fn contains_screen_point(&self, point: PhysicalPosition<f64>) -> bool {
        let Ok(origin) = self.window.outer_position() else {
            return false;
        };
        let size = self.window.outer_size();
        point.x >= f64::from(origin.x)
            && point.y >= f64::from(origin.y)
            && point.x < f64::from(origin.x) + f64::from(size.width)
            && point.y < f64::from(origin.y) + f64::from(size.height)
    }

    fn surface(&self) -> &GlSurface {
        self.host.surface().expect("headed tab host has a surface")
    }

    /// Who the pointer belongs to right now: the side holding a capture,
    /// otherwise whatever [`route_pointer`] decides from the position and
    /// egui's state.
    fn pointer_target(&self) -> PointerTarget {
        let ctx = PointerContext {
            over_chrome: match self.cursor {
                Some(p) => (p.y as f32) < self.content_top_px,
                None => true,
            },
            content_is_egui: self.content_is_egui,
            over_egui: self.egui.egui_ctx.is_pointer_over_egui(),
            popup_open: egui::Popup::is_any_open(&self.egui.egui_ctx),
        };
        route_pointer(self.capture.captured(), ctx)
    }

    /// Cursor position in physical pixels relative to the page area.
    fn content_point(&self) -> Option<(f32, f32)> {
        let p = self.cursor?;
        Some((p.x as f32, p.y as f32 - self.content_top_px))
    }

    /// Handle one winit event. Returns the commands the user issued.
    pub fn handle_event(
        &mut self,
        browser: &Browser,
        keymap: &Keymap,
        event: &WindowEvent,
    ) -> Vec<Command> {
        let active = browser.window(self.id).map(|w| w.active);
        let mut commands = Vec::new();
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
                return self.frame(browser, keymap);
            }
            WindowEvent::CloseRequested => {
                return vec![Command::CloseWindow(self.id)];
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    // Releases of keys and buttons held while focus moved
                    // away arrive elsewhere; do not wait for them here.
                    self.keys.clear();
                    self.capture.clear();
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
                let current = self.pointer_target();
                let target = match state {
                    ElementState::Pressed => self.capture.press(*button, current),
                    ElementState::Released => self.capture.release(*button, current),
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
                let key_input = keyutils::key_input_from_winit(key, self.modifiers);
                let egui_wants_keyboard = self.egui.egui_ctx.egui_wants_keyboard_input();
                let stroke = KeyStroke {
                    code: key_input.code,
                    modifiers: key_input.modifiers,
                    pressed: key.state == ElementState::Pressed,
                    repeat: key.repeat,
                };
                let live = |tab| {
                    browser
                        .window(self.id)
                        .is_some_and(|w| w.tab(tab).is_some())
                };
                let target = self
                    .keys
                    .route(stroke, egui_wants_keyboard, active, live, keymap);
                match target {
                    Some(KeyTarget::Page(tab)) => {
                        forward_to_egui = false;
                        self.host.input(tab, EngineInput::Key(key_input));
                    }
                    Some(KeyTarget::Command(kind)) => {
                        forward_to_egui = false;
                        if let Some(tab) = active {
                            commands.push(kind.into_command(self.id, tab));
                        }
                    }
                    Some(KeyTarget::Egui) => {}
                    None => forward_to_egui = false,
                }
            }
            // Phase 1 limitation: egui-winit calls `set_ime_allowed(false)`
            // whenever no egui text field has focus, so winit sends no Ime
            // events for page content and this arm is not reached for page
            // text fields. It stays so composed input reaches the page once
            // IME is enabled for page fields (see the README Todo list).
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
    fn frame(&mut self, browser: &Browser, keymap: &Keymap) -> Vec<Command> {
        let Some(model) = browser.window(self.id) else {
            return Vec::new();
        };
        let active = model.active;
        let crashed = model.active_tab().crashed.clone();
        let engine_error = browser.engine_error.as_deref();
        let others: Vec<(WindowId, String)> = browser
            .windows
            .iter()
            .filter(|w| w.id != self.id)
            .map(|w| (w.id, w.active_tab().display_title().to_string()))
            .collect();
        let profile = &browser.profile;
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
            commands = chrome::draw(
                ui,
                model,
                &others,
                profile,
                engine_error,
                chrome_state,
                keymap,
            );
            let rect = ui.available_rect_before_wrap();
            let (top, size) = content_pixels(rect, ui.pixels_per_point());
            content_top_px = top;
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
        // egui_glow paints over whatever the framebuffer holds, so clear it
        // first: a region egui does not draw (or a reused buffer) would
        // otherwise show stale pixels.
        let size = self.window.inner_size();
        self.egui.painter.clear(
            [size.width, size.height],
            chrome::background(&self.egui.egui_ctx),
        );
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
