//! Workspace helper tasks, run as `cargo xtask <task>` (alias in
//! `.cargo/config.toml`).
//!
//! `icons`: osmena's icon task for this workspace: rasterize
//! `assets/logo.svg` (which frames `assets/logo.png`) into `assets/icons/`
//! as the `paladin` icon set. Output is deterministic for a given
//! `Cargo.lock`, so the generated files are committed and CI checks they
//! are up to date.
//!
//! `angle-dlls [profile]`: Windows only. Copy `libEGL.dll` and
//! `libGLESv2.dll`, which the `servo` crate's `no-wgl` feature builds into
//! `target/<profile>/build/mozangle-*/out/`, next to the executables in
//! `target/<profile>/` and `target/<profile>/deps/`. The binary links
//! `libEGL.dll` at load time, and surfman loads it by name, so without this
//! step `paladin.exe` and the engine's test binary do not start.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    let task = std::env::args().nth(1);
    let result = match task.as_deref() {
        Some("icons") => osmena_xtask::icons(&project_root(), "paladin"),
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
