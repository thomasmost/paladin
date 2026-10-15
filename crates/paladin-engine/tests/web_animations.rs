//! Loads the Web Animations test pages in `tests/pages/` through Servo
//! without a window. Each page reports through `document.title`: `PASS`,
//! or `FAIL: <reason>`. One test function on purpose: Servo's options are
//! process-global, so a test binary may build exactly one engine.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use paladin_core::{EngineEvent, TabId};
use paladin_engine::{Engine, PhysicalSize, TabHost, Waker};

struct NoopWaker;

impl Waker for NoopWaker {
    fn wake(&self) {}
}

/// Every page in `tests/pages/`, run in this order.
const PAGES: &[&str] = &[
    "finish-event.html",
    "finished-promise.html",
    "cancel-rejects.html",
    "pause-holds-current-time.html",
    "playback-rate-scales.html",
];

/// How long one page may take to report; the first page also pays for
/// engine start-up.
const PAGE_TIMEOUT: Duration = Duration::from_secs(120);

#[test]
fn web_animations_pages_pass() {
    let dir = tempfile::tempdir().expect("temp dir");
    let engine =
        Engine::new(&dir.path().join("servo"), Arc::new(NoopWaker)).expect("engine starts");
    let mut host =
        TabHost::new_software(&engine, PhysicalSize::new(400, 300)).expect("software host");
    let pages = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pages");

    let mut failures = Vec::new();
    for (index, page) in PAGES.iter().enumerate() {
        let tab = TabId(index as u64 + 1);
        let url = url::Url::from_file_path(pages.join(page)).expect("absolute page path");
        host.create_tab(tab, url);

        let deadline = Instant::now() + PAGE_TIMEOUT;
        let verdict = loop {
            assert!(
                Instant::now() < deadline,
                "{page}: no verdict within {PAGE_TIMEOUT:?}"
            );
            engine.spin();
            let mut verdict = None;
            for event in engine.drain_events() {
                match event {
                    EngineEvent::TitleChanged { tab: t, title }
                        if t == tab && (title == "PASS" || title.starts_with("FAIL")) =>
                    {
                        verdict = Some(title);
                    }
                    EngineEvent::FrameReady(t) if t == tab => host.paint(t),
                    _ => {}
                }
            }
            if let Some(verdict) = verdict {
                break verdict;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        if verdict != "PASS" {
            failures.push(format!("{page}: {verdict}"));
        }
        host.close_tab(tab);
    }

    assert!(host.is_empty());
    drop(host);
    // Dropping the engine shuts Servo down and waits for it to finish.
    drop(engine);
    assert!(
        failures.is_empty(),
        "failing pages:\n{}",
        failures.join("\n")
    );
}
