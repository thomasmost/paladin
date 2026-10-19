//! What makes this build Paladin: the one constant the shared shell takes
//! its product strings and icon from.

use osmena_shell::osmena_core::Brand;

pub static BRAND: Brand = Brand {
    name: "Paladin",
    binary: "paladin",
    data_dir: "Paladin",
    homepage: "https://servo.org/",
    icon_png: include_bytes!("../../../assets/icons/paladin-256.png"),
};
