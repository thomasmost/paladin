//! Starts Servo without a window, loads a page that opens a popup, and
//! checks the page paints and the popup is adopted. One test function on
//! purpose: Servo's options are process-global, so a test binary may build
//! exactly one engine.

use std::sync::Arc;
use std::time::{Duration, Instant};

use osmena_core::{EngineEvent, LoadState, PendingTab, TabId};
use osmena_engine::{Engine as _, TabHost as _};
use paladin_engine::{Engine, PhysicalSize, TabHost, Waker};

struct NoopWaker;

impl Waker for NoopWaker {
    fn wake(&self) {}
}

#[test]
fn loads_a_data_url_headlessly() {
    let dir = tempfile::tempdir().expect("temp dir");
    let engine =
        Engine::new(&dir.path().join("servo"), Arc::new(NoopWaker), false).expect("engine starts");
    let mut host =
        TabHost::new_software(&engine, PhysicalSize::new(400, 300)).expect("software host");

    let tab = TabId(1);
    let popup = TabId(2);
    // The page opens a popup once it has loaded, so the opener is complete
    // before `window.open` runs.
    let url = url::Url::parse(
        "data:text/html,<title>Paladin headless</title><p>hello</p>\
         <script>addEventListener('load', function () { \
         window.open('data:text/html,<title>Paladin popup</title>'); });</script>",
    )
    .expect("data URL parses");
    host.create_tab(tab, url);

    let deadline = Instant::now() + Duration::from_secs(120);
    let mut saw_title = false;
    let mut saw_complete = false;
    let mut saw_frame = false;
    let mut popup_pending: Option<PendingTab> = None;
    let mut saw_popup_url = false;
    while !(saw_title && saw_complete && saw_frame && saw_popup_url) {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the page: title={saw_title} complete={saw_complete} \
             frame={saw_frame} popup_requested={} popup_url={saw_popup_url}",
            popup_pending.is_some(),
        );
        engine.spin();
        for event in engine.drain_events() {
            match event {
                // Servo reports an empty title when navigation starts and
                // the real one once <title> is parsed; wait for the latter.
                EngineEvent::TitleChanged { tab: t, title }
                    if t == tab && title == "Paladin headless" =>
                {
                    saw_title = true;
                }
                EngineEvent::LoadStateChanged {
                    tab: t,
                    state: LoadState::Complete,
                } if t == tab => saw_complete = true,
                EngineEvent::FrameReady(t) if t == tab => {
                    host.paint(t);
                    saw_frame = true;
                }
                EngineEvent::NewTabRequested { opener, pending } if opener == tab => {
                    assert!(popup_pending.is_none(), "page requested two popups");
                    popup_pending = Some(pending);
                    assert!(host.adopt_tab(pending, popup), "adopting the popup failed");
                }
                EngineEvent::UrlChanged { tab: t, url } if t == popup && url.scheme() == "data" => {
                    saw_popup_url = true;
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(host.page_title(tab).as_deref(), Some("Paladin headless"));
    assert!(host.load_complete(tab));
    assert!(host.has_tab(popup));
    // The pending entry was consumed by the first adoption.
    let pending = popup_pending.expect("popup was requested");
    assert!(!host.adopt_tab(pending, TabId(3)));

    host.close_tab(popup);
    host.close_tab(tab);
    assert!(host.is_empty());
    drop(host);
    // Dropping the engine shuts Servo down and waits for it to finish.
    drop(engine);
}
