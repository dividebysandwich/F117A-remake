//! Where the game's files are, resolved like Bevy's asset server: `$BEVY_ASSET_ROOT`, else
//! `$CARGO_MANIFEST_DIR` at run time (set by `cargo run` / `cargo test`), else the executable's
//! directory.

use std::path::PathBuf;
use std::sync::OnceLock;

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
