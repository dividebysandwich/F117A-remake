//! Where the game's files are, resolved like Bevy's asset server: `$BEVY_ASSET_ROOT`, else
//! `$CARGO_MANIFEST_DIR` at run time (set by `cargo run` / `cargo test`), else the executable's
//! directory. In the browser build the data files are compiled in (`build.rs`).

use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result};

/// The directory containing `assets/`.
pub fn root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        if let Some(r) = std::env::var_os("BEVY_ASSET_ROOT") {
            return PathBuf::from(r);
        }
        if let Some(r) = std::env::var_os("CARGO_MANIFEST_DIR") {
            let dir = PathBuf::from(r);
            // A crate of the workspace: the assets are the workspace's.
            return dir
                .ancestors()
                .find(|d| d.join("assets").is_dir())
                .map_or(dir.clone(), PathBuf::from);
        }
        std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_default()
    })
}

/// Path of a file under `assets/`, e.g. `asset("sim")`.
pub fn asset(rel: &str) -> PathBuf {
    root().join("assets").join(rel)
}

#[cfg(target_arch = "wasm32")]
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));
}

/// The contents of a file under `assets/`, e.g. `read("sim/tables.ron")`.
#[cfg(not(target_arch = "wasm32"))]
pub fn read(rel: &str) -> Result<Vec<u8>> {
    let path = asset(rel);
    std::fs::read(&path).with_context(|| path.display().to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn read(rel: &str) -> Result<Vec<u8>> {
    embedded::EMBEDDED
        .iter()
        .find(|(name, _)| *name == rel)
        .map(|(_, data)| data.to_vec())
        .with_context(|| format!("assets/{rel}: not in the browser build"))
}

pub fn read_to_string(rel: &str) -> Result<String> {
    String::from_utf8(read(rel)?).with_context(|| format!("assets/{rel}"))
}

/// Whether there is a file `rel` under `assets/`.
#[cfg(not(target_arch = "wasm32"))]
pub fn exists(rel: &str) -> bool {
    asset(rel).exists()
}

#[cfg(target_arch = "wasm32")]
pub fn exists(rel: &str) -> bool {
    embedded::FILES.contains(&rel)
}

/// The names of the files in directory `rel` of `assets/` (not those in its subdirectories).
#[cfg(not(target_arch = "wasm32"))]
pub fn list(rel: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(asset(rel)) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

#[cfg(target_arch = "wasm32")]
pub fn list(rel: &str) -> Vec<String> {
    let prefix = format!("{}/", rel.trim_end_matches('/'));
    embedded::FILES
        .iter()
        .filter_map(|f| f.strip_prefix(&prefix))
        .filter(|f| !f.contains('/'))
        .map(String::from)
        .collect()
}
