//! Servo construction, the event-loop waker bridge, and the event queue.

use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Once};

use osmena_core::{EngineEvent, EngineInfo, PendingTab};
use osmena_engine::raw_window_handle::{DisplayHandle, WindowHandle};
use osmena_engine::{EngineError, PhysicalSize, Waker};
use servo::{EventLoopWaker, Opts, Preferences, Servo, ServoBuilder};

use crate::delegate::DelegateState;
use crate::host::{GlSurface, TabHost};

/// What the shell needs to know about this engine. The version is the
/// `servo` crate pinned in this crate's `Cargo.toml`.
pub static INFO: EngineInfo = EngineInfo {
    name: "Servo",
    version: "0.7.0",
    data_dir: "servo",
    // Servo 0.7.0 has no API to stop a load.
    supports_stop: false,
    // The value Servo's own shell uses per wheel line.
    wheel_line_px: 76.0,
    // Servo runs no browser extensions.
    extensions: None,
};

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
    ///
    /// With `multiprocess`, Servo runs each tab's script thread (DOM, JS,
    /// layout) in a content process that it starts by running the current
    /// executable again with `--content-process <token>`, so the binary must
    /// route that argument to [`run_content_process`] before doing anything
    /// else. A content process that dies, from a native crash or a panic,
    /// costs that tab its page (the constellation reports it as crashed, see
    /// `notify_crashed`) and nothing else. Test binaries pass `false`: they
    /// do not handle the argument.
    pub fn new(
        servo_config_dir: &Path,
        waker: Arc<dyn Waker>,
        multiprocess: bool,
    ) -> Result<Engine, EngineError> {
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
            multiprocess,
            // Servo's content process sandbox exists for Linux and macOS
            // only; the content processes run unsandboxed everywhere.
            sandbox: false,
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

    pub(crate) fn servo(&self) -> &Servo {
        &self.servo
    }

    pub(crate) fn delegate(&self) -> &Rc<DelegateState> {
        &self.delegate
    }
}

impl osmena_engine::Engine for Engine {
    /// Servo renders through its own GL context on the window, which the
    /// chrome shares.
    type Surface = GlSurface;
    type Host = TabHost;

    /// Run one iteration of Servo's embedder loop: delivers delegate
    /// callbacks (which become [`EngineEvent`]s) and updates painting state.
    fn spin(&self) {
        self.servo.spin_event_loop();
    }

    fn drain_events(&self) -> Vec<EngineEvent> {
        self.delegate.drain()
    }

    /// Drop a tab that page content opened but that no window adopted.
    fn discard_pending(&self, pending: PendingTab) {
        if self.delegate.take_pending(pending).is_none() {
            log::debug!("no pending tab {pending:?} to discard");
        }
    }

    fn open_surface(
        &self,
        display: DisplayHandle<'_>,
        window: WindowHandle<'_>,
        size: PhysicalSize<u32>,
    ) -> Result<GlSurface, EngineError> {
        GlSurface::new(display, window, size)
    }

    fn open_host(
        &self,
        surface: &GlSurface,
        _window: WindowHandle<'_>,
        scale: f32,
    ) -> Result<TabHost, EngineError> {
        TabHost::new(self, surface, scale)
    }
}

impl Drop for Engine {
    /// Pending tabs hold `Servo` handles; release them so Servo can shut
    /// down when the `servo` field drops next.
    fn drop(&mut self) {
        self.delegate.clear_pending();
    }
}

/// Run this process as one of Servo's content processes and return when it
/// is done. `token` is the value of the `--content-process` argument Servo
/// gave the process when it started it (see [`Engine::new`]). The process
/// needs no profile, window, or engine: it connects back to the parent over
/// IPC and takes its options and preferences from there. The panic hook is
/// installed here too, so a panic on this process's script thread reaches
/// the parent's constellation as a crash report rather than a dead tab.
pub fn run_content_process(token: String) {
    install_panic_hook();
    exit_with_parent();
    servo::run_content_process(token);
}

/// End this content process when the browser process is gone, so a browser
/// that was killed or crashed natively leaves no orphans. On Windows the
/// browser puts itself and its children in a job object that the kernel
/// tears down with it (`paladin::process`), so nothing is needed here. On
/// Unix an orphan is re-parented, which a thread polling the parent pid
/// notices within a second.
#[cfg(unix)]
fn exit_with_parent() {
    // SAFETY: `getppid` has no preconditions and cannot fail.
    let parent = unsafe { libc::getppid() };
    let _ = std::thread::Builder::new()
        .name("ParentWatchdog".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                // SAFETY: as above.
                if unsafe { libc::getppid() } != parent {
                    std::process::exit(0);
                }
            }
        });
}

#[cfg(not(unix))]
fn exit_with_parent() {}

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
