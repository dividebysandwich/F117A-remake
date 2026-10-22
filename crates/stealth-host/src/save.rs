//! Campaign saves: `campaigns/<name>.json` in the game's data directory (`store`), or a file of
//! its own (a dedicated server's).

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use stealth_campaign::CampaignState;

use crate::store;

/// The store directory of the saves (`stealth_host::store`).
const DIR: &str = "data/campaigns";

fn file_name(name: &str) -> String {
    let safe: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    format!("{safe}.json")
}

fn key(name: &str) -> String {
    format!("{DIR}/{}", file_name(name))
}

/// The file of the campaign saved under `name`.
#[cfg(not(target_arch = "wasm32"))]
pub fn path_of(name: &str) -> Option<PathBuf> {
    store::path(&key(name))
}

pub fn save(name: &str, state: &CampaignState) -> Result<()> {
    store::write(&key(name), &serde_json::to_string(state)?)
}

/// Writes a campaign to `path` (aside first: a crash mid-write keeps the last save).
#[cfg(not(target_arch = "wasm32"))]
pub fn save_to(path: &Path, state: &CampaignState) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let json = serde_json::to_string(state)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load(name: &str) -> Result<Option<CampaignState>> {
    let key = key(name);
    let Some(text) = store::read(&key)? else { return Ok(None) };
    Ok(Some(serde_json::from_str(&text).context(key)?))
}

/// Removes the campaign saved under `name`; a save that is already gone is no error.
pub fn delete(name: &str) -> Result<()> {
    store::remove(&key(name))
}

/// The campaign in `path`, None if there is no such file.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_from(path: &Path) -> Result<Option<CampaignState>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| path.display().to_string()),
    };
    Ok(Some(serde_json::from_str(&text).with_context(|| path.display().to_string())?))
}

/// Saved campaigns, the most recently played first: (name, theater code, campaign hours).
pub fn list_saves() -> Vec<(String, String, f64)> {
    let mut found: Vec<(f64, String, String, f64)> = store::list(DIR)
        .into_iter()
        .filter_map(|(file, modified)| {
            let name = file.strip_suffix(".json")?.to_string();
            let state: CampaignState =
                serde_json::from_str(&store::read(&format!("{DIR}/{file}")).ok()??).ok()?;
            Some((modified, name, state.setup.theater_code.clone(), state.clock / 3600.0))
        })
        .collect();
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    found.into_iter().map(|(_, n, t, h)| (n, t, h)).collect()
}
