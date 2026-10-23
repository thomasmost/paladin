//! What makes this build Paladin: the one constant the shared shell takes
//! its product strings and icon from.

use osmena_shell::osmena_core::{Brand, ChromeColor};

pub static BRAND: Brand = Brand {
    name: "Paladin",
    binary: "paladin",
    data_dir: "Paladin",
    homepage: "https://servo.org/",
    chrome_color: ChromeColor::Slate,
    icon_png: include_bytes!("../../../assets/icons/paladin-256.png"),
};
