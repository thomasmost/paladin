//! Paladin: a desktop browser on the Servo engine with the osmena shell.
//! This crate is the brand, Servo's content-process preamble, the command
//! line, and the packaging metadata; the browser itself is `osmena_shell`
//! over `paladin_engine`.

// Release builds on Windows are GUI-subsystem apps, so the Start menu shortcut
// opens no console window. Debug builds keep the console for logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod brand;

use std::process::ExitCode;

use clap::Parser;
use osmena_shell::{CliArgs, Launch};

#[derive(Parser, Debug)]
#[command(name = "paladin", version, about = "Paladin desktop browser")]
struct Cli {
    /// Run every tab in this process instead of one content process per
    /// tab's script thread. A crash in a page then takes the browser down
    /// with it; for debugging the engine.
    #[arg(long)]
    single_process: bool,
    /// Passed by the browser process to the content processes it starts;
    /// not for users.
    #[arg(long, hide = true, value_name = "TOKEN")]
    content_process: Option<String>,
    #[command(flatten)]
    base: CliArgs,
}

fn main() -> ExitCode {
    // The engine installs the process logger when it starts (Servo's, which
    // also feeds its crash reporting; see `paladin_engine::Engine::new`), so
    // nothing is logged before then. It reads `RUST_LOG`; default it to
    // `info` like an `env_logger` setup would.
    if std::env::var_os("RUST_LOG").is_none() {
        // SAFETY: the first thing `main` does, before any other thread exists.
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }
    let cli = Cli::parse();
    if let Some(token) = cli.content_process {
        // A content process: no profile, window, or engine of its own. See
        // `paladin_engine::Engine::new`.
        paladin_engine::run_content_process(token);
        return ExitCode::SUCCESS;
    }
    let multiprocess = !cli.single_process;
    osmena_shell::run(Launch {
        brand: &brand::BRAND,
        engine: &paladin_engine::INFO,
        args: cli.base,
        start_engine: Box::new(move |dir, waker| {
            paladin_engine::Engine::new(dir, waker, multiprocess)
        }),
    })
}
