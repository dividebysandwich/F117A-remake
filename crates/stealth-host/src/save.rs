//! Campaign saves: `campaigns/<name>.json` in the game's data directory, or a file of its own
//! (a dedicated server's).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use stealth_campaign::CampaignState;

pub fn dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("stealth-mission-89").join("campaigns"))
}

fn file_name(name: &str) -> String {
    let safe: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    format!("{safe}.json")
}

/// The file of the campaign saved under `name`.
pub fn path_of(name: &str) -> Option<PathBuf> {
    dir().map(|d| d.join(file_name(name)))
}

pub fn save(name: &str, state: &CampaignState) -> Result<()> {
    let path = path_of(name).context("no data directory")?;
    save_to(&path, state)
}

/// Writes a campaign to `path` (aside first: a crash mid-write keeps the last save).
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
    match path_of(name) {
        Some(p) => load_from(&p),
        None => Ok(None),
    }
}

/// Removes the campaign saved under `name`; a save that is already gone is no error.
pub fn delete(name: &str) -> Result<()> {
    let path = path_of(name).context("no data directory")?;
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| path.display().to_string())
        }
        _ => Ok(()),
    }
}

/// The campaign in `path`, None if there is no such file.
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
    let Some(dir) = dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(std::time::SystemTime, String, String, f64)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension()? != "json" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            let modified = e.metadata().ok()?.modified().ok()?;
            let state: CampaignState =
                serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            Some((modified, name, state.setup.theater_code.clone(), state.clock / 3600.0))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, n, t, h)| (n, t, h)).collect()
}
