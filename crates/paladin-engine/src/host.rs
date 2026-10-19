//! GL surfaces and the per-window tab host.
//!
//! Rendering follows Servo's reference shell: a `WindowRenderingContext`
//! owns the window's GL context; an `OffscreenRenderingContext` derived
//! from it is what every tab in the window paints into; the app's egui
//! shares the window context and blits the offscreen framebuffer below the
//! toolbar through the blit callback `TabHost::paint_page` hands it.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use dpi::PhysicalSize;
use euclid::{Point2D, Scale, Size2D};
use osmena_core::{EngineEvent, EngineInput, LoadState, PendingTab, TabId};
use osmena_engine::{BlitFn, BlitRect, EngineError, PagePaint, Surface, TabHost as TabHostApi};
use raw_window_handle::{DisplayHandle, WindowHandle};
use servo::{
    LoadStatus, OffscreenRenderingContext, RenderingContext, Servo, SoftwareRenderingContext,
    WebView, WebViewBuilder, WindowRenderingContext,
};
use url::Url;

use crate::delegate::{DelegateState, favicon_from};
use crate::engine::Engine;
use crate::input::to_servo;

/// The GL context and surface of one native window.
pub struct GlSurface {
    context: Rc<WindowRenderingContext>,
}

impl GlSurface {
    pub fn new(
        display: DisplayHandle<'_>,
        window: WindowHandle<'_>,
        size: PhysicalSize<u32>,
    ) -> Result<GlSurface, EngineError> {
        let context = WindowRenderingContext::new(display, window, size)
            .map_err(|e| EngineError(format!("creating the window GL context: {e:?}")))?;
        Ok(GlSurface {
            context: Rc::new(context),
        })
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        self.context.size()
    }
}

impl Surface for GlSurface {
    fn make_current(&self) -> Result<(), EngineError> {
        self.context
            .make_current()
            .map_err(|e| EngineError(format!("making the GL context current: {e:?}")))
    }

    /// Call on every window resize, in physical pixels, never zero.
    fn resize(&self, size: PhysicalSize<u32>) {
        self.context.resize(size);
    }

    /// The glow handle egui renders through.
    fn gl(&self) -> Arc<glow::Context> {
        self.context.glow_gl_api()
    }

    /// Make current and bind the window's framebuffer before egui paints.
    fn begin_frame(&self) {
        if let Err(e) = self.make_current() {
            log::error!("{e}");
        }
        self.context.prepare_for_rendering();
    }

    /// Swap buffers.
    fn present(&self) {
        self.context.present();
    }
}

/// The engine side of one browser window: its tabs and render targets.
/// A `WebView` is bound to the rendering context it was created with, so
/// tabs never move between hosts.
pub struct TabHost {
    // Dropped first: WebViews must go before their rendering context.
    tabs: HashMap<TabId, WebView>,
    delegate: Rc<DelegateState>,
    servo: Servo,
    /// What tabs paint into: the offscreen context in a window, or a
    /// software context in tests.
    content: Rc<dyn RenderingContext>,
    offscreen: Option<Rc<OffscreenRenderingContext>>,
    hidpi: Cell<f32>,
}

impl TabHost {
    /// A host rendering into `surface`, which the window keeps. Tabs start
    /// at the surface's size and shrink to the content area via
    /// `resize_content`.
    pub fn new(engine: &Engine, surface: &GlSurface, hidpi: f32) -> Result<TabHost, EngineError> {
        surface.make_current()?;
        let offscreen = Rc::new(surface.context.offscreen_context(surface.size()));
        let content: Rc<dyn RenderingContext> = offscreen.clone();
        Ok(TabHost {
            tabs: HashMap::new(),
            delegate: engine.delegate().clone(),
            servo: engine.servo().clone(),
            content,
            offscreen: Some(offscreen),
            hidpi: Cell::new(hidpi),
        })
    }

    /// A host with no window, rendering in software. For tests.
    pub fn new_software(engine: &Engine, size: PhysicalSize<u32>) -> Result<TabHost, EngineError> {
        let context = SoftwareRenderingContext::new(size)
            .map_err(|e| EngineError(format!("creating a software GL context: {e:?}")))?;
        context
            .make_current()
            .map_err(|e| EngineError(format!("making the software context current: {e:?}")))?;
        Ok(TabHost {
            tabs: HashMap::new(),
            delegate: engine.delegate().clone(),
            servo: engine.servo().clone(),
            content: Rc::new(context),
            offscreen: None,
            hidpi: Cell::new(1.0),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    fn with(&self, tab: TabId, f: impl FnOnce(&WebView)) {
        match self.tabs.get(&tab) {
            Some(webview) => f(webview),
            None => log::warn!("no engine tab for {tab:?}"),
        }
    }

    /// Paint the tab into the host's render target.
    pub fn paint(&self, tab: TabId) {
        self.with(tab, |w| {
            if let Err(e) = self.content.make_current() {
                log::error!("making the content context current: {e:?}");
            }
            w.paint();
        });
    }

    /// A callback that blits the offscreen target into the window; `None`
    /// for a software host. A window host always returns `Some`, because
    /// its framebuffer exists from construction. The callback captures the
    /// framebuffer id and size when it is made, and `resize_content` deletes
    /// and recreates that framebuffer, so callers must fetch a fresh callback
    /// each frame rather than keep one across a resize.
    pub fn blit_callback(&self) -> Option<BlitFn> {
        let render = self.offscreen.as_ref()?.render_to_parent_callback()?;
        Some(Box::new(move |gl, rect: BlitRect| {
            render(
                gl,
                euclid::default::Rect::new(
                    Point2D::new(rect.x, rect.y_from_bottom),
                    Size2D::new(rect.width, rect.height),
                ),
            );
        }))
    }

    pub fn url(&self, tab: TabId) -> Option<Url> {
        self.tabs.get(&tab).and_then(|w| w.url())
    }

    pub fn page_title(&self, tab: TabId) -> Option<String> {
        self.tabs.get(&tab).and_then(|w| w.page_title())
    }

    pub fn load_complete(&self, tab: TabId) -> bool {
        self.tabs
            .get(&tab)
            .is_some_and(|w| w.load_status() == LoadStatus::Complete)
    }
}

impl TabHostApi for TabHost {
    /// Create a visible tab loading `url`.
    fn create_tab(&mut self, tab: TabId, url: Url) {
        let webview = WebViewBuilder::new(&self.servo, self.content.clone())
            .url(url)
            .hidpi_scale_factor(Scale::new(self.hidpi.get()))
            .delegate(self.delegate.clone())
            .build();
        self.delegate.register(&webview, tab);
        webview.show();
        self.tabs.insert(tab, webview);
    }

    /// Take over a tab that page content opened. Returns false if the
    /// engine no longer holds it.
    fn adopt_tab(&mut self, pending: PendingTab, tab: TabId) -> bool {
        let Some(webview) = self.delegate.take_pending(pending) else {
            return false;
        };
        self.delegate.register(&webview, tab);
        // Events before adoption had no tab id and were dropped; report
        // the current state so the model catches up.
        if let Some(url) = webview.url() {
            self.delegate.push(EngineEvent::UrlChanged { tab, url });
        }
        if let Some(title) = webview.page_title() {
            self.delegate.push(EngineEvent::TitleChanged { tab, title });
        }
        let state = match webview.load_status() {
            LoadStatus::Started | LoadStatus::HeadParsed => LoadState::Loading,
            LoadStatus::Complete => LoadState::Complete,
        };
        self.delegate
            .push(EngineEvent::LoadStateChanged { tab, state });
        self.delegate.push(EngineEvent::HistoryChanged {
            tab,
            can_go_back: webview.can_go_back(),
            can_go_forward: webview.can_go_forward(),
        });
        if let Some(favicon) = webview.favicon().and_then(|image| favicon_from(&image)) {
            self.delegate
                .push(EngineEvent::FaviconChanged { tab, favicon });
        }
        webview.show();
        self.tabs.insert(tab, webview);
        true
    }

    /// Drop the engine tab. Servo closes it when the last handle goes.
    fn close_tab(&mut self, tab: TabId) {
        if let Some(webview) = self.tabs.remove(&tab) {
            self.delegate.unregister(&webview);
        }
    }

    fn has_tab(&self, tab: TabId) -> bool {
        self.tabs.contains_key(&tab)
    }

    fn navigate(&self, tab: TabId, url: Url) {
        self.with(tab, |w| w.load(url));
    }

    fn back(&self, tab: TabId) {
        self.with(tab, |w| {
            if w.can_go_back() {
                w.go_back(1);
            }
        });
    }

    fn forward(&self, tab: TabId) {
        self.with(tab, |w| {
            if w.can_go_forward() {
                w.go_forward(1);
            }
        });
    }

    fn reload(&self, tab: TabId) {
        self.with(tab, |w| w.reload());
    }

    /// Servo 0.7.0 has no API to stop a load; this only logs.
    fn stop(&self, tab: TabId) {
        log::warn!("stop requested for {tab:?}, but Servo 0.7.0 cannot stop a load");
    }

    fn show(&self, tab: TabId) {
        self.with(tab, |w| w.show());
    }

    fn hide(&self, tab: TabId) {
        self.with(tab, |w| w.hide());
    }

    fn set_focused(&self, tab: TabId, focused: bool) {
        self.with(tab, |w| w.set_focused(focused));
    }

    /// Resize the tab's render target to the page area, if it changed.
    /// All tabs in a host share one target, so this resizes them all.
    fn resize_content(&self, tab: TabId, size: PhysicalSize<u32>) {
        self.with(tab, |w| {
            let current = w.size();
            if current.width as u32 != size.width || current.height as u32 != size.height {
                // Servo resizes its offscreen target in whichever GL context
                // is current, and the caller may not have made this window's
                // context current yet (another window may own it), so do it
                // here.
                if let Err(e) = self.content.make_current() {
                    log::error!("making the content context current: {e:?}");
                }
                w.resize(size);
            }
        });
    }

    fn set_hidpi(&self, scale: f32) {
        self.hidpi.set(scale);
        for webview in self.tabs.values() {
            webview.set_hidpi_scale_factor(Scale::new(scale));
        }
    }

    fn input(&self, tab: TabId, input: EngineInput) {
        self.with(tab, |w| {
            w.notify_input_event(to_servo(input));
        });
    }

    /// Paint the tab into the offscreen target and hand the shell the blit
    /// that copies it under the toolbar; nothing for a software host.
    fn paint_page(&self, tab: TabId, sink: &mut dyn FnMut(PagePaint<'_>)) {
        self.paint(tab);
        if let Some(blit) = self.blit_callback() {
            sink(PagePaint::Blit(blit));
        }
    }
}
