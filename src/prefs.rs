//! Saved files in the user's config directory, and the menu choices remembered between runs.

use bevy::prelude::*;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use stealth_mission::{MissionKind, Sky};

use crate::menu::MenuState;
use crate::settings::{DEFAULT_VR_COCKPIT_SCALE, Settings, VR_COCKPIT_SCALE_RANGE};

/// `<config dir>/stealth-mission-89/<name>` (`stealth_host::store`); None in the browser.
pub fn config_path(name: &str) -> Option<std::path::PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    return stealth_host::store::path(&format!("config/{name}"));
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        None
    }
}

/// Reads a saved file; None if it is missing or unreadable (logged).
pub fn load_json<T: DeserializeOwned>(name: &str) -> Option<T> {
    let text =
        stealth_host::store::read(&format!("config/{name}")).map_err(|e| warn!("{e:#}")).ok()??;
    serde_json::from_str(&text).map_err(|e| warn!("{name}: {e}; ignored")).ok()
}

pub fn save_json<T: Serialize>(name: &str, value: &T) -> anyhow::Result<()> {
    stealth_host::store::write(&format!("config/{name}"), &serde_json::to_string_pretty(value)?)
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
    pub friendly_fire: bool,
    pub weather: Option<Sky>,
    pub cloud_cover: Option<u8>,
    pub start_hour: Option<u8>,
    pub moon: Option<u8>,
    pub no_crashes: bool,
    pub auto_trim: bool,
    pub realistic_damage: bool,
    pub realistic_guidance: bool,
    pub tacview: bool,
    pub tacview_real_time: bool,
    pub air_defence: bool,
    pub head_tracking: bool,
    pub modern_shading: bool,
    pub shadows: bool,
    pub local_lights: bool,
    pub continuous_time: bool,
    #[serde(default)]
    pub sun_moon: bool,
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
    /// Multiplayer (`MenuState`).
    pub player_name: String,
    pub server_addr: String,
    pub server_name: String,
    pub host_port: u16,
    pub max_players: u8,
    /// None: on (the default before it could be chosen).
    pub list_public: Option<bool>,
    pub tooltips: bool,
    pub master_volume: f32,
    pub engine_volume: f32,
    pub effects_volume: f32,
    pub warning_volume: f32,
    pub engine_low_pass: bool,
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
            friendly_fire: menu.friendly_fire,
            weather: menu.weather,
            cloud_cover: menu.cloud_cover,
            start_hour: menu.start_hour,
            moon: menu.moon,
            no_crashes: false,
            auto_trim: true,
            realistic_damage: false,
            realistic_guidance: false,
            tacview: false,
            tacview_real_time: false,
            air_defence: true,
            head_tracking: false,
            // New players: modern shaders and time of day; classic shadows and lighting;
            // modern cockpit, HUD, avionics and flight model.
            modern_shading: true,
            shadows: false,
            local_lights: false,
            continuous_time: true,
            sun_moon: true,
            classic_cockpit: false,
            classic_hud: false,
            classic_avionics: false,
            classic_flight: false,
            vr_cockpit_scale: DEFAULT_VR_COCKPIT_SCALE,
            aircraft: Default::default(),
            vr_scale_version: VR_SCALE_VERSION,
            player_name: menu.player_name,
            server_addr: menu.server_addr,
            server_name: menu.server_name,
            host_port: menu.host_port,
            max_players: menu.max_players,
            list_public: Some(menu.list_public),
            tooltips: true,
            master_volume: 1.0,
            engine_volume: 1.0,
            effects_volume: 1.0,
            warning_volume: 1.0,
            engine_low_pass: true,
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
            friendly_fire: menu.friendly_fire,
            weather: menu.weather,
            cloud_cover: menu.cloud_cover,
            start_hour: menu.start_hour,
            moon: menu.moon,
            no_crashes: s.no_crashes,
            auto_trim: s.auto_trim,
            realistic_damage: s.realistic_damage,
            realistic_guidance: s.realistic_guidance,
            tacview: s.tacview,
            tacview_real_time: s.tacview_real_time,
            air_defence: s.air_defence,
            head_tracking: s.head_tracking,
            modern_shading: s.modern_shading,
            shadows: s.shadows,
            local_lights: s.local_lights,
            continuous_time: s.continuous_time,
            sun_moon: s.sun_moon,
            classic_cockpit: s.classic_cockpit,
            classic_hud: s.classic_hud,
            classic_avionics: s.classic_avionics,
            classic_flight: s.classic_flight,
            vr_cockpit_scale: s.vr_cockpit_scale,
            aircraft: s.aircraft,
            vr_scale_version: VR_SCALE_VERSION,
            player_name: menu.player_name.clone(),
            server_addr: menu.server_addr.clone(),
            server_name: menu.server_name.clone(),
            host_port: menu.host_port,
            max_players: menu.max_players,
            list_public: Some(menu.list_public),
            tooltips: s.tooltips,
            master_volume: s.master_volume,
            engine_volume: s.engine_volume,
            effects_volume: s.effects_volume,
            warning_volume: s.warning_volume,
            engine_low_pass: s.engine_low_pass,
        }
    }

    /// Switches turned on by command-line flags stay on.
    pub fn apply(&self, menu: &mut MenuState, s: &mut Settings) {
        menu.theater = self.theater.min(crate::menu::THEATERS.len() - 1);
        menu.roe = self.roe.min(2);
        menu.kind = self.kind;
        menu.opponents = self.opponents.min(3);
        menu.lockheed = self.lockheed;
        menu.friendly_fire = self.friendly_fire;
        menu.weather = self.weather;
        menu.cloud_cover = self.cloud_cover.map(|c| c.min(100));
        menu.start_hour = self.start_hour.map(|h| h.min(23));
        menu.moon = self.moon.map(|m| m % 8);
        menu.player_name = self.player_name.clone();
        menu.server_addr = self.server_addr.clone();
        menu.server_name = self.server_name.clone();
        menu.list_public = self.list_public.unwrap_or(true);
        if self.host_port != 0 {
            menu.host_port = self.host_port;
        }
        if self.max_players != 0 {
            menu.max_players = self.max_players.min(stealth_net::protocol::MAX_PLAYERS as u8);
        }
        s.no_crashes |= self.no_crashes;
        // On by default: a command-line flag turning it off stays off.
        s.auto_trim &= self.auto_trim;
        s.realistic_damage |= self.realistic_damage;
        s.realistic_guidance |= self.realistic_guidance;
        s.tacview |= self.tacview;
        s.tacview_real_time |= self.tacview_real_time;
        s.air_defence |= self.air_defence;
        s.head_tracking |= self.head_tracking;
        s.modern_shading |= self.modern_shading;
        s.shadows |= self.shadows;
        s.local_lights |= self.local_lights;
        s.continuous_time |= self.continuous_time;
        s.sun_moon |= self.sun_moon;
        s.classic_cockpit |= self.classic_cockpit;
        s.classic_hud |= self.classic_hud;
        s.classic_avionics |= self.classic_avionics;
        s.classic_flight |= self.classic_flight;
        s.tooltips = self.tooltips;
        let volume = |v: f32| v.clamp(0.0, 1.0);
        s.master_volume = volume(self.master_volume);
        s.engine_volume = volume(self.engine_volume);
        s.effects_volume = volume(self.effects_volume);
        s.warning_volume = volume(self.warning_volume);
        s.engine_low_pass = self.engine_low_pass;
        if !std::env::args().any(|a| a == "--aircraft") {
            s.aircraft = self.aircraft;
        }
        if !std::env::args().any(|a| a == "--vr-cockpit-scale") {
            s.vr_cockpit_scale = self
                .vr_cockpit_scale
                .clamp(*VR_COCKPIT_SCALE_RANGE.start(), *VR_COCKPIT_SCALE_RANGE.end());
        }
    }

    /// Takes the OPTIONS menus' settings (the per-user ones: game, graphics, sound and head
    /// tracking) from `s`.
    fn copy_options(&mut self, s: &Settings) {
        self.modern_shading = s.modern_shading;
        self.shadows = s.shadows;
        self.local_lights = s.local_lights;
        self.continuous_time = s.continuous_time;
        self.sun_moon = s.sun_moon;
        self.classic_cockpit = s.classic_cockpit;
        self.classic_hud = s.classic_hud;
        self.classic_avionics = s.classic_avionics;
        self.classic_flight = s.classic_flight;
        self.auto_trim = s.auto_trim;
        self.tooltips = s.tooltips;
        self.vr_cockpit_scale = s.vr_cockpit_scale;
        self.tacview = s.tacview;
        self.tacview_real_time = s.tacview_real_time;
        self.head_tracking = s.head_tracking;
        self.master_volume = s.master_volume;
        self.engine_volume = s.engine_volume;
        self.effects_volume = s.effects_volume;
        self.warning_volume = s.warning_volume;
        self.engine_low_pass = s.engine_low_pass;
    }

    /// The OPTIONS menus' settings of `s`, to compare.
    pub fn options(s: &Settings) -> Self {
        let mut p = Self::default();
        p.copy_options(s);
        p
    }

    /// Saves the OPTIONS menus' settings (in-flight menu), keeping the rest of the file.
    pub fn save_options(s: &Settings) {
        let mut p = Self::load();
        p.copy_options(s);
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

    /// The OPTIONS settings go through the preferences; an old file gets full volumes and the
    /// engine low-pass.
    #[test]
    fn options_round_trip() {
        let mut s = Settings::from_args();
        s.engine_volume = 0.4;
        s.engine_low_pass = false;
        s.classic_hud = true;
        let p: Prefs = serde_json::from_str(&serde_json::to_string(&Prefs::options(&s)).unwrap())
            .unwrap();
        let mut back = Settings::from_args();
        p.apply(&mut MenuState::default(), &mut back);
        assert_eq!(Prefs::options(&back), Prefs::options(&s));
        let old: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!((old.master_volume, old.engine_low_pass), (1.0, true));
    }

    #[test]
    fn tooltips_default_to_on() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert!(p.tooltips);
        let p: Prefs = serde_json::from_str(r#"{"tooltips": false}"#).unwrap();
        assert!(!p.tooltips);
    }
}
