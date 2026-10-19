//! Saved files in the user's config directory, and the menu choices remembered between runs.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use stealth_mission::{MissionKind, Sky};

use crate::menu::MenuState;
use crate::settings::{DEFAULT_VR_COCKPIT_SCALE, Settings, VR_COCKPIT_SCALE_RANGE};

/// `<config dir>/stealth-mission-89/<name>`.
pub fn config_path(name: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("stealth-mission-89").join(name))
}

/// Reads a saved file; None if it is missing or unreadable (logged).
pub fn load_json<T: DeserializeOwned>(name: &str) -> Option<T> {
    let path = config_path(name)?;
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).map_err(|e| warn!("{}: {e}; ignored", path.display())).ok()
}

pub fn save_json<T: Serialize>(name: &str, value: &T) -> anyhow::Result<PathBuf> {
    let path = config_path(name).ok_or_else(|| anyhow::anyhow!("no user config directory"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(value)?)?;
    Ok(path)
}

/// Menu choices and switches (`prefs.json`), used when the game starts into the menu.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Prefs {
    pub theater: usize,
    pub roe: u8,
    pub kind: MissionKind,
    pub opponents: u8,
    pub lockheed: bool,
    pub weather: Option<Sky>,
    pub cloud_cover: Option<u8>,
    pub no_crashes: bool,
    pub modern_shading: bool,
    pub shadows: bool,
    pub local_lights: bool,
    pub classic_cockpit: bool,
    pub classic_flight: bool,
    pub vr_cockpit_scale: f32,
}

impl Default for Prefs {
    fn default() -> Self {
        let menu = MenuState::default();
        Self {
            theater: menu.theater,
            roe: menu.roe,
            kind: menu.kind,
            opponents: menu.opponents,
            lockheed: menu.lockheed,
            weather: menu.weather,
            cloud_cover: menu.cloud_cover,
            no_crashes: false,
            modern_shading: false,
            shadows: false,
            local_lights: false,
            classic_cockpit: false,
            classic_flight: false,
            vr_cockpit_scale: DEFAULT_VR_COCKPIT_SCALE,
        }
    }
}

const FILE: &str = "prefs.json";

impl Prefs {
    pub fn load() -> Self {
        load_json(FILE).unwrap_or_default()
    }

    pub fn from_state(menu: &MenuState, s: &Settings) -> Self {
        Self {
            theater: menu.theater,
            roe: menu.roe,
            kind: menu.kind,
            opponents: menu.opponents,
            lockheed: menu.lockheed,
            weather: menu.weather,
            cloud_cover: menu.cloud_cover,
            no_crashes: s.no_crashes,
            modern_shading: s.modern_shading,
            shadows: s.shadows,
            local_lights: s.local_lights,
            classic_cockpit: s.classic_cockpit,
            classic_flight: s.classic_flight,
            vr_cockpit_scale: s.vr_cockpit_scale,
        }
    }

    /// Switches turned on by command-line flags stay on.
    pub fn apply(&self, menu: &mut MenuState, s: &mut Settings) {
        menu.theater = self.theater.min(crate::menu::THEATERS.len() - 1);
        menu.roe = self.roe.min(2);
        menu.kind = self.kind;
        menu.opponents = self.opponents.min(3);
        menu.lockheed = self.lockheed;
        menu.weather = self.weather;
        menu.cloud_cover = self.cloud_cover.map(|c| c.min(100));
        s.no_crashes |= self.no_crashes;
        s.modern_shading |= self.modern_shading;
        s.shadows |= self.shadows;
        s.local_lights |= self.local_lights;
        s.classic_cockpit |= self.classic_cockpit;
        s.classic_flight |= self.classic_flight;
        if !std::env::args().any(|a| a == "--vr-cockpit-scale") {
            s.vr_cockpit_scale = self.vr_cockpit_scale.clamp(*VR_COCKPIT_SCALE_RANGE.start(), *VR_COCKPIT_SCALE_RANGE.end());
        }
    }

    /// Writes the file if anything changed since the last save.
    pub fn save_if_changed(menu: &MenuState, s: &Settings, last: &mut Option<Prefs>) {
        let now = Self::from_state(menu, s);
        if last.as_ref() == Some(&now) {
            return;
        }
        if let Err(e) = save_json(FILE, &now) {
            warn!("saving {FILE}: {e}");
        }
        *last = Some(now);
    }
}
