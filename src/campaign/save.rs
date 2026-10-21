//! Campaign saves: `campaigns/<name>.json` in the game's data directory.

use bevy::prelude::*;
use stealth_campaign::CampaignState;

fn dir() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("stealth-mission-89").join("campaigns"))
}

fn file_name(name: &str) -> String {
    let safe: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    format!("{safe}.json")
}

pub fn save(name: &str, state: &CampaignState) {
    let Some(dir) = dir() else { return };
    let result = std::fs::create_dir_all(&dir).and_then(|_| {
        let json = serde_json::to_string(state).map_err(std::io::Error::other)?;
        // Written aside first: a crash mid-write keeps the last save.
        let tmp = dir.join(format!("{}.tmp", file_name(name)));
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, dir.join(file_name(name)))
    });
    if let Err(e) = result {
        warn!("saving campaign {name}: {e}");
    }
}

pub fn load(name: &str) -> Option<CampaignState> {
    let text = std::fs::read_to_string(dir()?.join(file_name(name))).ok()?;
    serde_json::from_str(&text).map_err(|e| warn!("campaign {name}: {e}")).ok()
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
