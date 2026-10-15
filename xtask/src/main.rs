//! Workspace helper tasks, run as `cargo xtask <task>` (alias in
//! `.cargo/config.toml`).
//!
//! `icons`: rasterize `assets/logo.svg` into `assets/icons/`:
//! `paladin-<size>.png` for each size in `PNG_SIZES`, `paladin.png`
//! (1024 px), `paladin.icns` (macOS), and `paladin.ico` (Windows). Output is
//! deterministic for a given `Cargo.lock`, so the generated files are
//! committed and CI checks they are up to date.
//!
//! `angle-dlls [profile]`: Windows only. Copy `libEGL.dll` and
//! `libGLESv2.dll`, which the `servo` crate's `no-wgl` feature builds into
//! `target/<profile>/build/mozangle-*/out/`, next to the executables in
//! `target/<profile>/` and `target/<profile>/deps/`. The binary links
//! `libEGL.dll` at load time, and surfman loads it by name, so without this
//! step `paladin.exe` and the engine's test binary do not start.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use resvg::tiny_skia;
use resvg::usvg;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PNG_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];
const ICO_SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];
/// (pixel size, icns slot): the set Apple's `iconutil` produces for a
/// standard `.iconset`. The `_2x` slots are the Retina versions.
const ICNS_TYPES: [(u32, icns::IconType); 10] = [
    (16, icns::IconType::RGBA32_16x16),
    (32, icns::IconType::RGBA32_16x16_2x),
    (32, icns::IconType::RGBA32_32x32),
    (64, icns::IconType::RGBA32_32x32_2x),
    (128, icns::IconType::RGBA32_128x128),
    (256, icns::IconType::RGBA32_128x128_2x),
    (256, icns::IconType::RGBA32_256x256),
    (512, icns::IconType::RGBA32_256x256_2x),
    (512, icns::IconType::RGBA32_512x512),
    (1024, icns::IconType::RGBA32_512x512_2x),
];

fn main() {
    let task = std::env::args().nth(1);
    let result = match task.as_deref() {
        Some("icons") => icons(&project_root()),
        Some("angle-dlls") => {
            let profile = std::env::args()
                .nth(2)
                .unwrap_or_else(|| "debug".to_string());
            angle_dlls(&project_root(), &profile)
        }
        _ => {
            eprintln!("usage: cargo xtask icons | cargo xtask angle-dlls [debug|release]");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

fn icons(root: &Path) -> Result<()> {
    let svg_path = root.join("assets").join("logo.svg");
    let svg_data =
        std::fs::read(&svg_path).map_err(|e| format!("reading {}: {e}", svg_path.display()))?;
    let tree = usvg::Tree::from_data(&svg_data, &usvg::Options::default())?;

    let out = root.join("assets").join("icons");
    std::fs::create_dir_all(&out)?;

    let mut pngs: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for size in PNG_SIZES.iter().chain(ICO_SIZES.iter()).copied() {
        if let std::collections::btree_map::Entry::Vacant(entry) = pngs.entry(size) {
            entry.insert(render_png(&tree, size)?);
        }
    }
    for size in PNG_SIZES {
        std::fs::write(out.join(format!("paladin-{size}.png")), &pngs[&size])?;
    }
    std::fs::write(out.join("paladin.png"), &pngs[&1024])?;

    let mut family = icns::IconFamily::new();
    for (size, icon_type) in ICNS_TYPES {
        let image = icns::Image::read_png(Cursor::new(&pngs[&size]))?;
        family.add_icon_with_type(&image, icon_type)?;
    }
    let mut icns_bytes = Vec::new();
    family.write(&mut icns_bytes)?;
    std::fs::write(out.join("paladin.icns"), icns_bytes)?;

    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in ICO_SIZES {
        let image = ico::IconImage::read_png(Cursor::new(&pngs[&size]))?;
        dir.add_entry(ico::IconDirEntry::encode_as_png(&image)?);
    }
    let mut ico_bytes = Vec::new();
    dir.write(&mut ico_bytes)?;
    std::fs::write(out.join("paladin.ico"), ico_bytes)?;

    println!("wrote icons to {}", out.display());
    Ok(())
}

fn angle_dlls(root: &Path, profile: &str) -> Result<()> {
    const DLLS: [&str; 2] = ["libEGL.dll", "libGLESv2.dll"];
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let profile_dir = target_dir.join(profile);
    let build_dir = profile_dir.join("build");

    // There may be several mozangle-<hash> directories from older builds;
    // take the most recently modified copy of each DLL.
    let mut newest: BTreeMap<&str, (std::time::SystemTime, PathBuf)> = BTreeMap::new();
    for entry in std::fs::read_dir(&build_dir)
        .map_err(|e| format!("reading {}: {e}", build_dir.display()))?
    {
        let dir = entry?.path();
        let is_mozangle = dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("mozangle-"));
        if !is_mozangle {
            continue;
        }
        for name in DLLS {
            let dll = dir.join("out").join(name);
            let Ok(modified) = dll.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            let replace = newest.get(name).is_none_or(|(when, _)| modified > *when);
            if replace {
                newest.insert(name, (modified, dll));
            }
        }
    }
    if newest.len() != DLLS.len() {
        return Err(format!(
            "ANGLE DLLs not found under {}; build the workspace first (Windows only)",
            build_dir.display()
        )
        .into());
    }
    for (name, (_, dll)) in newest {
        for dest_dir in [profile_dir.clone(), profile_dir.join("deps")] {
            std::fs::create_dir_all(&dest_dir)?;
            let dest = dest_dir.join(name);
            std::fs::copy(&dll, &dest)
                .map_err(|e| format!("copying {} to {}: {e}", dll.display(), dest.display()))?;
            println!("copied {name} to {}", dest_dir.display());
        }
    }
    Ok(())
}

fn render_png(tree: &usvg::Tree, size: u32) -> Result<Vec<u8>> {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("icon size must be non-zero")?;
    let scale_x = size as f32 / tree.size().width();
    let scale_y = size as f32 / tree.size().height();
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.encode_png()?)
}
