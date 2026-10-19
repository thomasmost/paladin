//! Servo delegate callbacks become [`EngineEvent`]s keyed by core tab ids.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};

use osmena_core::{EngineEvent, Favicon, LoadState, PendingTab, TabId};
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
pub(crate) fn favicon_from(image: &Image) -> Option<Favicon> {
    favicon_from_pixels(image.format, image.width, image.height, image.data())
}

/// The conversion behind [`favicon_from`], over raw pixels so tests can
/// cover every format without building a Servo `Image`.
fn favicon_from_pixels(
    format: PixelFormat,
    width: u32,
    height: u32,
    data: &[u8],
) -> Option<Favicon> {
    let pixels = (width as usize) * (height as usize);
    let rgba: Vec<u8> = match format {
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
        rgba: rgba.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::{PixelFormat, favicon_from_pixels};

    fn rgba_of(format: PixelFormat, width: u32, height: u32, data: &[u8]) -> Option<Vec<u8>> {
        favicon_from_pixels(format, width, height, data).map(|f| f.rgba.to_vec())
    }

    #[test]
    fn rgba_passes_through_unchanged() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            rgba_of(PixelFormat::RGBA8, 2, 1, &data),
            Some(data.to_vec())
        );
    }

    #[test]
    fn bgra_swaps_red_and_blue() {
        // Two pixels: B,G,R,A then B,G,R,A.
        let data = [3, 2, 1, 4, 7, 6, 5, 8];
        assert_eq!(
            rgba_of(PixelFormat::BGRA8, 2, 1, &data),
            Some(vec![1, 2, 3, 4, 5, 6, 7, 8])
        );
    }

    #[test]
    fn rgb_gains_opaque_alpha() {
        let data = [1, 2, 3, 4, 5, 6];
        assert_eq!(
            rgba_of(PixelFormat::RGB8, 1, 2, &data),
            Some(vec![1, 2, 3, 255, 4, 5, 6, 255])
        );
    }

    #[test]
    fn k8_replicates_luminance_with_opaque_alpha() {
        let data = [10, 200];
        assert_eq!(
            rgba_of(PixelFormat::K8, 2, 1, &data),
            Some(vec![10, 10, 10, 255, 200, 200, 200, 255])
        );
    }

    #[test]
    fn ka8_replicates_luminance_and_keeps_alpha() {
        let data = [10, 128, 200, 64];
        assert_eq!(
            rgba_of(PixelFormat::KA8, 2, 1, &data),
            Some(vec![10, 10, 10, 128, 200, 200, 200, 64])
        );
    }

    #[test]
    fn every_format_rejects_a_short_buffer() {
        // A 2x2 image needs 4 pixels: 16 RGBA bytes, 12 RGB, 8 KA, 4 K.
        let formats = [
            (PixelFormat::RGBA8, 4),
            (PixelFormat::BGRA8, 4),
            (PixelFormat::RGB8, 3),
            (PixelFormat::KA8, 2),
            (PixelFormat::K8, 1),
        ];
        for (format, bytes_per_pixel) in formats {
            let short = vec![0u8; 4 * bytes_per_pixel - 1];
            assert!(
                rgba_of(format, 2, 2, &short).is_none(),
                "{format:?} accepted a short buffer"
            );
        }
    }

    #[test]
    fn rgba_rejects_a_buffer_longer_than_the_image() {
        let long = vec![0u8; 4 * 2 * 2 + 1];
        assert!(rgba_of(PixelFormat::RGBA8, 2, 2, &long).is_none());
    }

    #[test]
    fn empty_image_yields_empty_favicon() {
        assert_eq!(rgba_of(PixelFormat::RGBA8, 0, 0, &[]), Some(Vec::new()));
    }
}
