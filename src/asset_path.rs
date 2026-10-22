//! Where the game's files are, resolved like Bevy's asset server: `$BEVY_ASSET_ROOT`, else
//! `$CARGO_MANIFEST_DIR` at run time (set by `cargo run` / `cargo test`), else the executable's
//! directory. Files read directly (not through the asset server) must go through here, never
//! through the current directory or a compile-time path, so shipped builds find `assets/` next
//! to the executable, and the browser build its compiled-in copies (`stealth_host::assets`).

use std::path::PathBuf;
use std::sync::OnceLock;

pub use stealth_host::assets::{exists, list, read, read_to_string};

/// The directory containing `assets/`.
#[cfg(not(target_arch = "wasm32"))]
pub fn root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(bevy::asset::io::file::FileAssetReader::get_base_path)
}

/// The browser build has no directory: the asset server fetches from the page's address.
#[cfg(target_arch = "wasm32")]
pub fn root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(PathBuf::new)
}

/// Path of a file under `assets/`, e.g. `asset("images/sprites.png")`.
pub fn asset(rel: &str) -> PathBuf {
    root().join("assets").join(rel)
}

/// A RON file under `assets/`; panics naming the file when it is missing or malformed (the
/// game cannot run without it).
pub fn load_ron<T: serde::de::DeserializeOwned>(rel: &str) -> T {
    let text = read_to_string(rel).unwrap_or_else(|e| panic!("{e:#}"));
    ron::from_str(&text).unwrap_or_else(|e| panic!("assets/{rel}: {e}"))
}
