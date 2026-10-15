//! Paladin: a desktop browser on the Servo engine with an egui chrome.

mod app;
mod chrome;
mod icon;
mod input;
mod keyutils;
mod window;

use std::process::ExitCode;

use clap::Parser;
use paladin_core::command::open_window;
use paladin_core::profile::DEFAULT_PROFILE_NAME;
use paladin_core::{Browser, Command, Profile, Settings, dispatch};
use url::Url;
use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::{App, AppEvent};

#[derive(Parser, Debug)]
#[command(name = "paladin", version, about = "Paladin desktop browser")]
struct Cli {
    /// Profile name; state lives under the per-user Paladin directory.
    #[arg(long, default_value = DEFAULT_PROFILE_NAME)]
    profile: String,
    /// URLs to open, one tab each. With none, the homepage opens.
    urls: Vec<String>,
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();

    let profile = match Profile::resolve(&cli.profile) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("paladin: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = profile.ensure_dirs() {
        eprintln!(
            "paladin: cannot create profile directory {}: {e}",
            profile.dir.display()
        );
        return ExitCode::FAILURE;
    }
    let settings = match Settings::load(&profile.settings_path()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("paladin: {e} (in {})", profile.settings_path().display());
            return ExitCode::FAILURE;
        }
    };
    log::info!("profile {:?} at {}", profile.name, profile.dir.display());

    let mut urls = Vec::new();
    for arg in &cli.urls {
        match chrome::location_input_to_url(arg) {
            Some(url) => urls.push(url),
            None => {
                eprintln!("paladin: not a URL: {arg:?}");
                return ExitCode::FAILURE;
            }
        }
    }

    let mut browser = Browser::new(profile, settings);
    let first: Url = urls.first().cloned().unwrap_or_else(|| browser.homepage());
    let mut actions = open_window(&mut browser, first);
    let window = browser.windows[0].id;
    for url in urls.into_iter().skip(1) {
        actions.extend(dispatch(
            &mut browser,
            &Command::NewTab {
                window,
                url: Some(url),
            },
        ));
    }

    let mut builder = EventLoop::<AppEvent>::with_user_event();
    // winit's default macOS menu owns Cmd+Q: AppKit runs `terminate:`
    // before the window sees the key. Without the menu, the app's own
    // shutdown path runs instead (the keymap binds Cmd+Q in Phase 5).
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_default_menu(false);
    }
    let event_loop = match builder.build() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("paladin: cannot create event loop: {e}");
            return ExitCode::FAILURE;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(browser, actions, &event_loop);
    match event_loop.run_app(&mut app) {
        Ok(()) if app.started_cleanly() => ExitCode::SUCCESS,
        Ok(()) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("paladin: event loop error: {e}");
            ExitCode::FAILURE
        }
    }
}
