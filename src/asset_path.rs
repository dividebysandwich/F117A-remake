//! Where the game's files are, resolved like Bevy's asset server: `$BEVY_ASSET_ROOT`, else
//! `$CARGO_MANIFEST_DIR` at run time (set by `cargo run` / `cargo test`), else the executable's
//! directory. Files read with `std::fs` must go through here, never through the current
//! directory or a compile-time path, so shipped builds find `assets/` next to the executable.

use std::path::PathBuf;
use std::sync::OnceLock;

/// The directory containing `assets/` (and, in a checkout, `re-work/`).
pub fn root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(bevy::asset::io::file::FileAssetReader::get_base_path)
}

/// Path of a file under `assets/`, e.g. `asset("images/sprites.idx")`.
pub fn asset(rel: &str) -> PathBuf {
    root().join("assets").join(rel)
}
