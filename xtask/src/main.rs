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

/// The size rendered from the SVG; every other size is downsampled from it.
const LARGEST_SIZE: u32 = 1024;
const PNG_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, LARGEST_SIZE];
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
    let assets = root.join("assets");
    let svg_path = assets.join("logo.svg");
    let svg_data =
        std::fs::read(&svg_path).map_err(|e| format!("reading {}: {e}", svg_path.display()))?;
    // The SVG references the PNG artwork by a relative path, resolved
    // against the assets directory rather than the current directory.
    let options = usvg::Options {
        resources_dir: Some(assets.clone()),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(&svg_data, &options)?;
    // usvg drops an <image> whose file is missing or unreadable with only a
    // log warning, which would leave an empty tile in every icon.
    if !contains_image(tree.root()) {
        return Err(format!(
            "{} has no loaded <image>: check that assets/logo.png exists and is a PNG",
            svg_path.display()
        )
        .into());
    }

    let out = root.join("assets").join("icons");
    std::fs::create_dir_all(&out)?;

    // Render once at the largest size and area-average the rest down from
    // it. The artwork is a raster, and resvg's bicubic sampling cannot
    // average the dozens of source pixels behind each pixel of a 16 px icon;
    // area averaging over the 1024 px render can. Sizes that do not divide
    // 1024 (24, 48) weight the partly covered source pixels.
    let largest = render(&tree, LARGEST_SIZE)?;
    let mut pngs: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for size in PNG_SIZES.iter().chain(ICO_SIZES.iter()).copied() {
        if let std::collections::btree_map::Entry::Vacant(entry) = pngs.entry(size) {
            entry.insert(downsample(&largest, size)?.encode_png()?);
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

fn render(tree: &usvg::Tree, size: u32) -> Result<tiny_skia::Pixmap> {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("icon size must be non-zero")?;
    let scale_x = size as f32 / tree.size().width();
    let scale_y = size as f32 / tree.size().height();
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

/// Shrinks a square pixmap to `size` by area averaging: each output pixel
/// is the mean of the source pixels under it, weighted by how much of each
/// source pixel its footprint covers. Sizes that do not divide the source
/// (24, 48) work the same way. The pixels are premultiplied RGBA, so a
/// plain weighted mean is correct.
fn downsample(source: &tiny_skia::Pixmap, size: u32) -> Result<tiny_skia::Pixmap> {
    if source.width() != source.height() {
        return Err(format!(
            "downsample needs a square source, got {}x{}",
            source.width(),
            source.height()
        )
        .into());
    }
    let src = source.width() as usize;
    let n = size as usize;
    if src == n {
        return Ok(source.clone());
    }
    if src < n {
        return Err(format!("cannot downsample {src} px to {n} px").into());
    }
    let ratio = src as f64 / n as f64;
    let data = source.data();
    let mut out = tiny_skia::Pixmap::new(size, size).ok_or("icon size must be non-zero")?;
    let out_data = out.data_mut();
    // Overlap of the one-pixel-wide source cell `s` with the span [a, b).
    let overlap = |s: usize, a: f64, b: f64| (b.min(s as f64 + 1.0) - a.max(s as f64)).max(0.0);
    for y in 0..n {
        let (y0, y1) = (y as f64 * ratio, (y + 1) as f64 * ratio);
        for x in 0..n {
            let (x0, x1) = (x as f64 * ratio, (x + 1) as f64 * ratio);
            let mut sum = [0f64; 4];
            let mut weight = 0f64;
            for sy in y0.floor() as usize..(y1.ceil() as usize).min(src) {
                let wy = overlap(sy, y0, y1);
                for sx in x0.floor() as usize..(x1.ceil() as usize).min(src) {
                    let w = wy * overlap(sx, x0, x1);
                    let i = (sy * src + sx) * 4;
                    for (acc, &value) in sum.iter_mut().zip(&data[i..i + 4]) {
                        *acc += w * f64::from(value);
                    }
                    weight += w;
                }
            }
            let o = (y * n + x) * 4;
            for (dst, total) in out_data[o..o + 4].iter_mut().zip(sum) {
                *dst = (total / weight).round() as u8;
            }
        }
    }
    Ok(out)
}

/// Whether the group or any group below it holds a loaded `<image>`.
fn contains_image(group: &usvg::Group) -> bool {
    group.children().iter().any(|node| match node {
        usvg::Node::Image(_) => true,
        usvg::Node::Group(child) => contains_image(child),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(size: u32, rgba: [u8; 4]) -> tiny_skia::Pixmap {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
        for px in pixmap.data_mut().chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
        pixmap
    }

    #[test]
    fn uniform_source_stays_uniform_at_a_size_that_does_not_divide_it() {
        let source = filled(1024, [200, 100, 50, 255]);
        let out = downsample(&source, 48).unwrap();
        assert_eq!((out.width(), out.height()), (48, 48));
        assert!(
            out.data()
                .chunks_exact(4)
                .all(|px| px == [200, 100, 50, 255])
        );
    }

    #[test]
    fn checkerboard_averages_to_the_mean() {
        // 4x4 checkerboard of opaque black and white, premultiplied.
        let mut source = tiny_skia::Pixmap::new(4, 4).unwrap();
        for (i, px) in source.data_mut().chunks_exact_mut(4).enumerate() {
            let (x, y) = (i % 4, i / 4);
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            px.copy_from_slice(&[v, v, v, 255]);
        }
        for size in [2, 1] {
            let out = downsample(&source, size).unwrap();
            // Each output pixel covers an equal number of black and white
            // pixels, so the mean is 127.5, rounded to 128.
            assert!(
                out.data()
                    .chunks_exact(4)
                    .all(|px| px == [128, 128, 128, 255]),
                "size {size}: {:?}",
                out.data()
            );
        }
    }

    #[test]
    fn two_by_two_to_one_weights_premultiplied_alpha() {
        // Three transparent pixels and one opaque red: a quarter of the
        // coverage, so premultiplied red 64 (255 / 4 rounded) and alpha 64.
        let mut source = tiny_skia::Pixmap::new(2, 2).unwrap();
        source.data_mut()[0..4].copy_from_slice(&[255, 0, 0, 255]);
        let out = downsample(&source, 1).unwrap();
        assert_eq!(out.data(), [64, 0, 0, 64]);
    }

    #[test]
    fn same_size_is_a_copy_and_upsampling_is_an_error() {
        let source = filled(8, [1, 2, 3, 4]);
        assert_eq!(downsample(&source, 8).unwrap().data(), source.data());
        assert!(downsample(&source, 16).is_err());
        let wide = tiny_skia::Pixmap::new(8, 4).unwrap();
        assert!(downsample(&wide, 2).is_err());
    }

    #[test]
    fn contains_image_sees_a_loaded_image_and_not_a_missing_one() {
        let dir = std::env::temp_dir().join(format!("paladin-xtask-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("art.png"),
            filled(2, [0, 0, 0, 255]).encode_png().unwrap(),
        )
        .unwrap();
        let options = usvg::Options {
            resources_dir: Some(dir.clone()),
            ..usvg::Options::default()
        };
        let svg = |href: &str| {
            format!(
                "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 4 4'>\
                 <g><image href='{href}' width='4' height='4'/></g></svg>"
            )
        };
        let loaded = usvg::Tree::from_str(&svg("art.png"), &options).unwrap();
        assert!(contains_image(loaded.root()));
        let missing = usvg::Tree::from_str(&svg("missing.png"), &options).unwrap();
        assert!(!contains_image(missing.root()));
        let no_image = usvg::Tree::from_str(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 4 4'><rect width='4' height='4'/></svg>",
            &options,
        )
        .unwrap();
        assert!(!contains_image(no_image.root()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
