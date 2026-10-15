//! GL surfaces and the per-window tab host.
//!
//! Rendering follows Servo's reference shell: a `WindowRenderingContext`
//! owns the window's GL context; an `OffscreenRenderingContext` derived
//! from it is what every tab in the window paints into; the app's egui
//! shares the window context and blits the offscreen framebuffer below the
//! toolbar through [`TabHost::blit_callback`].

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use dpi::PhysicalSize;
use euclid::{Point2D, Scale, Size2D};
use paladin_core::{EngineEvent, LoadState, PendingTab, TabId};
use raw_window_handle::{DisplayHandle, WindowHandle};
use servo::{
    LoadStatus, OffscreenRenderingContext, RenderingContext, Servo, SoftwareRenderingContext,
    WebView, WebViewBuilder, WindowRenderingContext,
};
use url::Url;

use crate::delegate::DelegateState;
use crate::engine::{Engine, EngineError};
use crate::input::{EngineInput, to_servo};

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

    pub fn make_current(&self) -> Result<(), EngineError> {
        self.context
            .make_current()
            .map_err(|e| EngineError(format!("making the GL context current: {e:?}")))
    }

    /// Call on every window resize, in physical pixels, never zero.
    pub fn resize(&self, size: PhysicalSize<u32>) {
        self.context.resize(size);
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        self.context.size()
    }

    /// The glow handle egui renders through.
    pub fn gl(&self) -> Arc<glow::Context> {
        self.context.glow_gl_api()
    }

    /// Make current and bind the window's framebuffer before egui paints.
    pub fn begin_frame(&self) {
        if let Err(e) = self.make_current() {
            log::error!("{e}");
        }
        self.context.prepare_for_rendering();
    }

    /// Swap buffers.
    pub fn present(&self) {
        self.context.present();
    }
}

/// A target rectangle for the blit, in GL window coordinates (origin at
/// the bottom left, physical pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlitRect {
    pub x: i32,
    pub y_from_bottom: i32,
    pub width: i32,
    pub height: i32,
}

/// Copies the active tab's offscreen framebuffer into the window.
pub type BlitFn = Box<dyn Fn(&glow::Context, BlitRect) + Send + Sync>;

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
    surface: Option<GlSurface>,
    hidpi: Cell<f32>,
}

impl TabHost {
    /// A host rendering into `surface`. Tabs start at the surface's size
    /// and shrink to the content area via [`TabHost::resize_content`].
    pub fn new(engine: &Engine, surface: GlSurface, hidpi: f32) -> Result<TabHost, EngineError> {
        surface.make_current()?;
        let offscreen = Rc::new(surface.context.offscreen_context(surface.size()));
        let content: Rc<dyn RenderingContext> = offscreen.clone();
        Ok(TabHost {
            tabs: HashMap::new(),
            delegate: engine.delegate().clone(),
            servo: engine.servo().clone(),
            content,
            offscreen: Some(offscreen),
            surface: Some(surface),
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
            surface: None,
            hidpi: Cell::new(1.0),
        })
    }

    /// The window surface; `None` for a software host.
    pub fn surface(&self) -> Option<&GlSurface> {
        self.surface.as_ref()
    }

    /// Create a visible tab loading `url`.
    pub fn create_tab(&mut self, tab: TabId, url: Url) {
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
    pub fn adopt_tab(&mut self, pending: PendingTab, tab: TabId) -> bool {
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
        webview.show();
        self.tabs.insert(tab, webview);
        true
    }

    /// Drop the engine tab. Servo closes it when the last handle goes.
    pub fn close_tab(&mut self, tab: TabId) {
        if let Some(webview) = self.tabs.remove(&tab) {
            self.delegate.unregister(&webview);
        }
    }

    pub fn has_tab(&self, tab: TabId) -> bool {
        self.tabs.contains_key(&tab)
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

    pub fn navigate(&self, tab: TabId, url: Url) {
        self.with(tab, |w| w.load(url));
    }

    pub fn back(&self, tab: TabId) {
        self.with(tab, |w| {
            if w.can_go_back() {
                w.go_back(1);
            }
        });
    }

    pub fn forward(&self, tab: TabId) {
        self.with(tab, |w| {
            if w.can_go_forward() {
                w.go_forward(1);
            }
        });
    }

    pub fn reload(&self, tab: TabId) {
        self.with(tab, |w| w.reload());
    }

    /// Servo 0.7.0 has no API to stop a load; this only logs.
    pub fn stop(&self, tab: TabId) {
        log::warn!("stop requested for {tab:?}, but Servo 0.7.0 cannot stop a load");
    }

    pub fn show(&self, tab: TabId) {
        self.with(tab, |w| w.show());
    }

    pub fn hide(&self, tab: TabId) {
        self.with(tab, |w| w.hide());
    }

    pub fn set_focused(&self, tab: TabId, focused: bool) {
        self.with(tab, |w| w.set_focused(focused));
    }

    /// Resize the tab's render target to the page area, if it changed.
    /// All tabs in a host share one target, so this resizes them all.
    pub fn resize_content(&self, tab: TabId, size: PhysicalSize<u32>) {
        self.with(tab, |w| {
            let current = w.size();
            if current.width as u32 != size.width || current.height as u32 != size.height {
                w.resize(size);
            }
        });
    }

    pub fn set_hidpi(&self, scale: f32) {
        self.hidpi.set(scale);
        for webview in self.tabs.values() {
            webview.set_hidpi_scale_factor(Scale::new(scale));
        }
    }

    pub fn input(&self, tab: TabId, input: EngineInput) {
        self.with(tab, |w| {
            w.notify_input_event(to_servo(input));
        });
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
    /// before the first paint or for a software host.
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
