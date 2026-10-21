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
    pub realistic_damage: bool,
    pub modern_shading: bool,
    pub shadows: bool,
    pub local_lights: bool,
    pub classic_cockpit: bool,
    pub classic_hud: bool,
    pub classic_avionics: bool,
    pub classic_flight: bool,
    pub vr_cockpit_scale: f32,
    pub aircraft: crate::aircraft::AircraftKind,
    /// 0, 1: `vr_cockpit_scale` is relative to life size or to one size for every aircraft,
    /// 2: relative to the aircraft's own (`AircraftSpec::vr_scale`).
    #[serde(default)]
    pub vr_scale_version: u32,
}

const VR_SCALE_VERSION: u32 = 2;

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
            realistic_damage: false,
            // New players: modern shaders; classic shadows and lighting; modern cockpit, HUD,
            // avionics and flight model.
            modern_shading: true,
            shadows: false,
            local_lights: false,
            classic_cockpit: false,
            classic_hud: false,
            classic_avionics: false,
            classic_flight: false,
            vr_cockpit_scale: DEFAULT_VR_COCKPIT_SCALE,
            aircraft: Default::default(),
            vr_scale_version: VR_SCALE_VERSION,
        }
    }
}

const FILE: &str = "prefs.json";

impl Prefs {
    pub fn load() -> Self {
        let mut p: Self = load_json(FILE).unwrap_or_default();
        p.migrate();
        p
    }

    /// A VR cockpit size saved before the sizes were the aircraft's own goes back to 100%.
    fn migrate(&mut self) {
        if self.vr_scale_version < VR_SCALE_VERSION {
            self.vr_cockpit_scale = DEFAULT_VR_COCKPIT_SCALE;
            self.vr_scale_version = VR_SCALE_VERSION;
        }
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
            realistic_damage: s.realistic_damage,
            modern_shading: s.modern_shading,
            shadows: s.shadows,
            local_lights: s.local_lights,
            classic_cockpit: s.classic_cockpit,
            classic_hud: s.classic_hud,
            classic_avionics: s.classic_avionics,
            classic_flight: s.classic_flight,
            vr_cockpit_scale: s.vr_cockpit_scale,
            aircraft: s.aircraft,
            vr_scale_version: VR_SCALE_VERSION,
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
        s.realistic_damage |= self.realistic_damage;
        s.modern_shading |= self.modern_shading;
        s.shadows |= self.shadows;
        s.local_lights |= self.local_lights;
        s.classic_cockpit |= self.classic_cockpit;
        s.classic_hud |= self.classic_hud;
        s.classic_avionics |= self.classic_avionics;
        s.classic_flight |= self.classic_flight;
        if !std::env::args().any(|a| a == "--aircraft") {
            s.aircraft = self.aircraft;
        }
        if !std::env::args().any(|a| a == "--vr-cockpit-scale") {
            s.vr_cockpit_scale = self
                .vr_cockpit_scale
                .clamp(*VR_COCKPIT_SCALE_RANGE.start(), *VR_COCKPIT_SCALE_RANGE.end());
        }
    }

    /// Saves the graphics, cockpit and HUD switches and the VR cockpit size (in-flight menu),
    /// keeping the rest of the file.
    pub fn save_switches(s: &Settings) {
        let mut p = Self::load();
        p.modern_shading = s.modern_shading;
        p.shadows = s.shadows;
        p.local_lights = s.local_lights;
        p.classic_cockpit = s.classic_cockpit;
        p.classic_hud = s.classic_hud;
        p.classic_avionics = s.classic_avionics;
        p.vr_cockpit_scale = s.vr_cockpit_scale;
        if let Err(e) = save_json(FILE, &p) {
            warn!("saving the preferences: {e}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_vr_cockpit_size_is_reset() {
        for old in
            [r#"{"vr_cockpit_scale": 0.7}"#, r#"{"vr_cockpit_scale": 1.25, "vr_scale_version": 1}"#]
        {
            let mut p: Prefs = serde_json::from_str(old).unwrap();
            p.migrate();
            assert!((p.vr_cockpit_scale - 1.0).abs() < 1e-6);
        }
        let mut p: Prefs =
            serde_json::from_str(r#"{"vr_cockpit_scale": 0.9, "vr_scale_version": 2}"#).unwrap();
        p.migrate();
        assert!((p.vr_cockpit_scale - 0.9).abs() < 1e-6);
        let mut q: Prefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        q.migrate();
        assert!((q.vr_cockpit_scale - 0.9).abs() < 1e-6, "kept through a save");
        assert_eq!(Prefs::default().vr_scale_version, VR_SCALE_VERSION);
    }
}
