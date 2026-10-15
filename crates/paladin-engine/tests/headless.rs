//! Starts Servo without a window and loads a page. One test function on
//! purpose: Servo's options are process-global, so a test binary may build
//! exactly one engine.

use std::sync::Arc;
use std::time::{Duration, Instant};

use paladin_core::{EngineEvent, LoadState, TabId};
use paladin_engine::{Engine, PhysicalSize, TabHost, Waker};

struct NoopWaker;

impl Waker for NoopWaker {
    fn wake(&self) {}
}

#[test]
fn loads_a_data_url_headlessly() {
    let dir = tempfile::tempdir().expect("temp dir");
    let engine =
        Engine::new(&dir.path().join("servo"), Arc::new(NoopWaker)).expect("engine starts");
    let mut host =
        TabHost::new_software(&engine, PhysicalSize::new(400, 300)).expect("software host");

    let tab = TabId(1);
    let url = url::Url::parse("data:text/html,<title>Paladin headless</title><p>hello</p>")
        .expect("data URL parses");
    host.create_tab(tab, url);

    let deadline = Instant::now() + Duration::from_secs(120);
    let mut saw_title = false;
    let mut saw_complete = false;
    while !(saw_title && saw_complete) {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the page: title={saw_title} complete={saw_complete}"
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
                EngineEvent::FrameReady(t) if t == tab => host.paint(t),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(host.page_title(tab).as_deref(), Some("Paladin headless"));
    assert!(host.load_complete(tab));

    host.close_tab(tab);
    assert!(host.is_empty());
    drop(host);
    // Dropping the engine shuts Servo down and waits for it to finish.
    drop(engine);
}
