//! Servo construction, the event-loop waker bridge, and the event queue.

use std::fmt;
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Once};

use paladin_core::{EngineEvent, PendingTab};
use servo::{EventLoopWaker, Opts, Preferences, Servo, ServoBuilder};

use crate::delegate::DelegateState;

/// Wakes the application's event loop from any thread. Servo calls this
/// when it has work for the main thread; the app then calls
/// [`Engine::spin`].
pub trait Waker: Send + Sync + 'static {
    fn wake(&self);
}

#[derive(Debug)]
pub struct EngineError(pub String);

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EngineError {}

#[derive(Clone)]
struct ServoWaker(Arc<dyn Waker>);

impl EventLoopWaker for ServoWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        self.0.wake();
    }
}

/// One Servo instance. Servo's options and preferences are process-global,
/// so build exactly one `Engine` per process.
pub struct Engine {
    servo: Servo,
    delegate: Rc<DelegateState>,
}

impl Engine {
    /// Start Servo with its cookies, local storage, and HTTP cache under
    /// `servo_config_dir` (the profile's `servo/` directory).
    pub fn new(servo_config_dir: &Path, waker: Arc<dyn Waker>) -> Result<Engine, EngineError> {
        install_crypto_provider();
        install_panic_hook();
        std::fs::create_dir_all(servo_config_dir).map_err(|e| {
            EngineError(format!(
                "creating engine directory {}: {e}",
                servo_config_dir.display()
            ))
        })?;
        let opts = Opts {
            config_dir: Some(servo_config_dir.to_path_buf()),
            // Servo's default is `true`, meant for test runners: a panic on
            // any script thread then exits the whole process with code 1 and
            // no crash report. With `false` the constellation reports the
            // panic to the delegate (`notify_crashed`, an
            // `EngineEvent::TabCrashed`), closes that tab's pipeline, and
            // replaces it with a crash page; the other tabs keep running.
            hard_fail: false,
            ..Opts::default()
        };
        // Servo reports start-up failures by panicking. Turn that into an
        // error so the app can name the profile and exit cleanly.
        let servo = catch_unwind(AssertUnwindSafe(|| {
            ServoBuilder::default()
                .opts(opts)
                .preferences(Preferences {
                    // Web Animations: implemented in Paladin's Servo fork.
                    dom_web_animations_enabled: true,
                    ..Preferences::default()
                })
                .event_loop_waker(Box::new(ServoWaker(waker)))
                .build()
        }))
        .map_err(|payload| {
            EngineError(format!(
                "Servo failed to start: {}",
                panic_message(&*payload)
            ))
        })?;
        // Servo's logger: an `env_logger` (filtered by `RUST_LOG`) plus a
        // forwarder to the constellation, which turns an error logged while
        // a thread is panicking into that thread's panic report (see
        // `install_panic_hook`). It must be the process's only logger, so
        // nothing else may initialise one.
        servo.setup_logging();
        let delegate = Rc::new(DelegateState::default());
        servo.set_delegate(delegate.clone());
        Ok(Engine { servo, delegate })
    }

    /// Run one iteration of Servo's embedder loop: delivers delegate
    /// callbacks (which become [`EngineEvent`]s) and updates painting state.
    /// Call after every window event and every [`Waker::wake`].
    pub fn spin(&self) {
        self.servo.spin_event_loop();
    }

    /// Take every event produced since the last call.
    pub fn drain_events(&self) -> Vec<EngineEvent> {
        self.delegate.drain()
    }

    /// Drop a tab that page content opened but that no window adopted.
    pub fn discard_pending(&self, pending: PendingTab) {
        if self.delegate.take_pending(pending).is_none() {
            log::debug!("no pending tab {pending:?} to discard");
        }
    }

    pub(crate) fn servo(&self) -> &Servo {
        &self.servo
    }

    pub(crate) fn delegate(&self) -> &Rc<DelegateState> {
        &self.delegate
    }
}

impl Drop for Engine {
    /// Pending tabs hold `Servo` handles; release them so Servo can shut
    /// down when the `servo` field drops next.
    fn drop(&mut self) {
        self.delegate.clear_pending();
    }
}

/// Install the process panic hook Servo's crash reporting relies on, once.
///
/// The hook keeps the default one (the "thread X panicked at" message and,
/// with `RUST_BACKTRACE`, the backtrace) and then logs the panic message at
/// error level. Servo's logger (see `Servo::setup_logging`) records an
/// error logged on a panicking thread as a panic entry, and the
/// constellation then closes the pipelines of that script thread, reports
/// the crash to the delegate (`notify_crashed`, an
/// `EngineEvent::TabCrashed`), and loads a crash page in their place, so
/// a panic in one tab's script thread does not take the others down.
/// Panics on other threads unwind as before once the hook returns.
fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
            default_hook(info);
            if !servo::should_panic_hook_suppress_termination() {
                log::error!("{}", panic_message(info.payload()));
            }
        }));
    });
}

/// Servo's networking reads the process default rustls provider. Installing
/// twice is an error, which is harmless here.
fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
