//! Servo delegate callbacks become [`EngineEvent`]s keyed by core tab ids.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};

use paladin_core::{EngineEvent, Favicon, LoadState, PendingTab, TabId};
use servo::{
    ConsoleLogLevel, CreateNewWebViewRequest, Image, LoadStatus, PixelFormat, ServoDelegate,
    ServoError, WebView, WebViewDelegate, WebViewId,
};
use url::Url;

/// Shared by every `WebView` and by `Servo` itself. All callbacks arrive on
/// the engine thread from inside `spin_event_loop`, so plain `RefCell`s
/// suffice.
#[derive(Default)]
pub(crate) struct DelegateState {
    events: RefCell<VecDeque<EngineEvent>>,
    tabs: RefCell<HashMap<WebViewId, TabId>>,
    /// Tabs that page content opened (`window.open`), held alive until the
    /// model adopts them.
    pending: RefCell<HashMap<PendingTab, WebView>>,
    next_pending: Cell<u64>,
}

impl DelegateState {
    pub(crate) fn register(&self, webview: &WebView, tab: TabId) {
        self.tabs.borrow_mut().insert(webview.id(), tab);
    }

    pub(crate) fn unregister(&self, webview: &WebView) {
        self.tabs.borrow_mut().remove(&webview.id());
    }

    pub(crate) fn take_pending(&self, pending: PendingTab) -> Option<WebView> {
        self.pending.borrow_mut().remove(&pending)
    }

    /// Drop every tab page content opened that no window adopted.
    pub(crate) fn clear_pending(&self) {
        self.pending.borrow_mut().clear();
    }

    pub(crate) fn drain(&self) -> Vec<EngineEvent> {
        self.events.borrow_mut().drain(..).collect()
    }

    pub(crate) fn push(&self, event: EngineEvent) {
        self.events.borrow_mut().push_back(event);
    }

    fn tab(&self, webview: &WebView) -> Option<TabId> {
        let tab = self.tabs.borrow().get(&webview.id()).copied();
        if tab.is_none() {
            log::debug!("event for unregistered webview {:?}", webview.id());
        }
        tab
    }
}

impl WebViewDelegate for DelegateState {
    fn notify_url_changed(&self, webview: WebView, url: Url) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::UrlChanged { tab, url });
        }
    }

    fn notify_page_title_changed(&self, webview: WebView, title: Option<String>) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::TitleChanged {
                tab,
                title: title.unwrap_or_default(),
            });
        }
    }

    fn notify_status_text_changed(&self, webview: WebView, status: Option<String>) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::StatusChanged {
                tab,
                text: status.unwrap_or_default(),
            });
        }
    }

    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        if let Some(tab) = self.tab(&webview) {
            let state = match status {
                LoadStatus::Started | LoadStatus::HeadParsed => LoadState::Loading,
                LoadStatus::Complete => LoadState::Complete,
            };
            self.push(EngineEvent::LoadStateChanged { tab, state });
        }
    }

    fn notify_favicon_changed(&self, webview: WebView) {
        let Some(tab) = self.tab(&webview) else {
            return;
        };
        let favicon = webview.favicon().and_then(|image| favicon_from(&image));
        if let Some(favicon) = favicon {
            self.push(EngineEvent::FaviconChanged { tab, favicon });
        }
    }

    fn notify_new_frame_ready(&self, webview: WebView) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::FrameReady(tab));
        }
    }

    fn notify_history_changed(&self, webview: WebView, entries: Vec<Url>, current: usize) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::HistoryChanged {
                tab,
                can_go_back: current > 0,
                can_go_forward: current + 1 < entries.len(),
            });
        }
    }

    fn notify_closed(&self, webview: WebView) {
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::TabClosed(tab));
        }
    }

    fn notify_crashed(&self, webview: WebView, reason: String, backtrace: Option<String>) {
        if let Some(backtrace) = backtrace {
            log::error!("tab crashed: {reason}\n{backtrace}");
        } else {
            log::error!("tab crashed: {reason}");
        }
        if let Some(tab) = self.tab(&webview) {
            self.push(EngineEvent::TabCrashed { tab, reason });
        }
    }

    fn request_create_new(&self, parent: WebView, request: CreateNewWebViewRequest) {
        let Some(opener) = self.tab(&parent) else {
            return;
        };
        // The new WebView must be built now and kept alive, or Servo drops
        // it. It shares the parent's rendering context, so it belongs to
        // the same window; the model decides where it goes via AdoptTab.
        let webview = request
            .builder(parent.rendering_context())
            .hidpi_scale_factor(parent.hidpi_scale_factor())
            .delegate(parent.delegate())
            .build();
        let pending = PendingTab(self.next_pending.get());
        self.next_pending.set(pending.0 + 1);
        self.pending.borrow_mut().insert(pending, webview);
        self.push(EngineEvent::NewTabRequested { opener, pending });
    }

    fn show_console_message(&self, _webview: WebView, level: ConsoleLogLevel, message: String) {
        log::log!(level.into(), "{message}");
    }
}

impl ServoDelegate for DelegateState {
    fn notify_error(&self, error: ServoError) {
        self.push(EngineEvent::EngineError(format!("{error:?}")));
    }

    fn show_console_message(&self, level: ConsoleLogLevel, message: String) {
        log::log!(level.into(), "{message}");
    }
}

/// Convert Servo's favicon image to straight RGBA8.
fn favicon_from(image: &Image) -> Option<Favicon> {
    let (width, height) = (image.width, image.height);
    let pixels = (width as usize) * (height as usize);
    let data = image.data();
    let rgba: Vec<u8> = match image.format {
        PixelFormat::RGBA8 => data.to_vec(),
        PixelFormat::BGRA8 => data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect(),
        PixelFormat::RGB8 => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        PixelFormat::K8 => data.iter().flat_map(|&k| [k, k, k, 255]).collect(),
        PixelFormat::KA8 => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
    };
    if rgba.len() != pixels * 4 {
        log::warn!("favicon pixel data has unexpected length");
        return None;
    }
    Some(Favicon {
        width,
        height,
        rgba,
    })
}
