//! What makes this build Paladin: the one constant the shared shell takes
//! its product strings, icon, and update source from.

use osmena_shell::osmena_core::update::{UpdateSource, build_number};
use osmena_shell::osmena_core::{Brand, ChromeColor};

pub static BRAND: Brand = Brand {
    name: "Paladin",
    binary: "paladin",
    data_dir: "Paladin",
    homepage: "https://servo.org/",
    chrome_color: ChromeColor::Slate,
    icon_png: include_bytes!("../../../assets/icons/paladin-256.png"),
    version: env!("CARGO_PKG_VERSION"),
    // CI sets OSMENA_BUILD_NUMBER to the workflow run number when it
    // packages a build (.github/workflows/ci.yml); a local build has
    // none and never updates itself.
    build: build_number(option_env!("OSMENA_BUILD_NUMBER")),
    // Every green push to main is a GitHub release; "latest" redirects
    // to the newest one, and its latest.toml names the build's assets.
    updates: Some(UpdateSource {
        manifest_url: "https://github.com/thomasmost/paladin/releases/latest/download/latest.toml",
    }),
};
