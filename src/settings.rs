//! Command-line settings, app states and the mission setup handed from the menu to the flight.

use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Theater selector and mission generator.
    #[default]
    Menu,
    /// `--viewer`: model browser.
    Viewer,
    Flight,
    /// Mission results after a flight.
    Debrief,
}

/// Ordering of the setup done when entering the flight (or viewer).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SetupSet {
    /// Makes sure a mission exists and fills `MissionSetup` from it.
    Mission,
    Palette,
    World,
    Spawn,
}

pub fn configure(app: &mut App) {
    for s in [AppState::Flight, AppState::Viewer] {
        app.configure_sets(OnEnter(s), (SetupSet::Mission, SetupSet::Palette, SetupSet::World, SetupSet::Spawn).chain());
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Settings {
    /// Modern shading: MSAA, subtle directional shading, smooth haze.
    pub modern_shading: bool,
    pub initial_state: AppState,
    /// `--vr`: OpenXR headset (chosen at launch; falls back to the window without a runtime).
    pub vr: bool,
    /// Sun shadows (cockpit and world).
    pub shadows: bool,
    /// Local lights: cockpit flood/display lights, explosions, plumes, runway lights.
    pub local_lights: bool,
    /// `--classic-cockpit`: the original instrument panel layout and 77x60 displays instead
    /// of the modern panel with high-resolution displays and bezel buttons.
    pub classic_cockpit: bool,
    /// `--classic-hud`: the original's low-resolution HUD instead of the F-16 style one.
    pub classic_hud: bool,
    /// `--classic-flight`: the original's flight model instead of the modern aerodynamic one
    /// (the default for `--script` runs unless `--modern-flight` is given).
    pub classic_flight: bool,
    /// `--no-crashes`: crash protection (auto pull-up, no ground crashes, collisions bounce).
    pub no_crashes: bool,
    /// `--vr-cockpit-scale <percent>`: size of the cockpit (and the world) in the headset,
    /// 100% = `VR_COCKPIT_SCALE_BASE` of life size.
    pub vr_cockpit_scale: f32,
    /// `--aircraft f117|f16`: the flown aircraft.
    pub aircraft: crate::aircraft::AircraftKind,
}

/// Rig scale of the 100% VR cockpit size: 70% of life size looked most realistic in the headset.
pub const VR_COCKPIT_SCALE_BASE: f32 = 0.7;
/// Default VR cockpit size.
pub const DEFAULT_VR_COCKPIT_SCALE: f32 = 1.0;
/// Range of the VR cockpit size.
pub const VR_COCKPIT_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.7..=1.5;

/// What the next flight looks like (filled by the menu or command-line flags).
#[derive(Resource, Debug, Clone)]
pub struct MissionSetup {
    /// Theater id (lower case, e.g. "ce").
    pub theater: String,
    pub night: bool,
    /// Time-of-day seed n (palette starts 2n steps along).
    pub tod_seed: u8,
    /// WLD name of the takeoff base (None = first airbase).
    pub start_base: Option<String>,
    /// Exact start position from a generated mission (else the base is used).
    pub start: Option<StartPose>,
    /// Debug free camera instead of the aircraft.
    pub freecam: bool,
    /// Sky of the flight's mission (None: rolled from the mission seed).
    pub weather: Option<stealth_mission::Sky>,
    /// Coverage of the weather's main cloud layer (None: rolled).
    pub cloud_cover: Option<u8>,
}

/// Aircraft start position and heading (original world units, radians clockwise from north).
#[derive(Debug, Clone, Copy)]
pub struct StartPose {
    pub east_wu: f64,
    pub north_wu: f64,
    pub heading: f32,
    pub carrier: bool,
}

/// `--weather random|clear|scattered|broken|overcast|storm` (default random).
pub fn weather_arg(args: &[String]) -> Option<stealth_mission::Sky> {
    get(args, "--weather").and_then(|s| stealth_mission::Sky::parse(&s))
}

/// `--cloud-cover <percent>`: coverage of the main cloud layer.
pub fn cover_arg(args: &[String]) -> Option<u8> {
    get(args, "--cloud-cover").and_then(|s| s.parse::<u8>().ok()).map(|c| c.min(100))
}

fn args() -> Vec<String> {
    std::env::args().collect()
}

fn get(args: &[String], f: &str) -> Option<String> {
    args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned()
}

impl Settings {
    /// The cockpit flown: the classic cockpit only where the aircraft has one.
    pub fn cockpit(&self) -> (crate::cockpit3d::layout::PanelStyle, &'static str) {
        self.aircraft.spec().cockpit(self.classic_cockpit)
    }

    /// Scale of the VR rig (head and hand offsets) for the chosen cockpit size.
    pub fn vr_rig_scale(&self) -> f32 {
        self.vr_cockpit_scale * VR_COCKPIT_SCALE_BASE
    }

    pub fn from_args() -> Self {
        let a = args();
        let has = |f: &str| a.iter().any(|x| x == f);
        let initial_state = if has("--viewer") {
            AppState::Viewer
        } else if has("--fly") || has("--freecam") || has("--theater") || has("--script") {
            AppState::Flight
        } else {
            AppState::Menu
        };
        let vr = has("--vr");
        Self {
            modern_shading: has("--modern"),
            initial_state,
            vr,
            shadows: has("--shadows"),
            local_lights: has("--local-lights"),
            classic_cockpit: has("--classic-cockpit"),
            classic_hud: has("--classic-hud"),
            classic_flight: has("--classic-flight") || (has("--script") && !has("--modern-flight")),
            no_crashes: has("--no-crashes"),
            vr_cockpit_scale: get(&a, "--vr-cockpit-scale")
                .and_then(|s| s.parse::<f32>().ok())
                .map_or(DEFAULT_VR_COCKPIT_SCALE, |p| (p / 100.0).clamp(*VR_COCKPIT_SCALE_RANGE.start(), *VR_COCKPIT_SCALE_RANGE.end())),
            aircraft: get(&a, "--aircraft").and_then(|s| crate::aircraft::AircraftKind::parse(&s)).unwrap_or_default(),
        }
    }
}

impl MissionSetup {
    pub fn from_args() -> Self {
        let a = args();
        Self {
            theater: get(&a, "--theater").unwrap_or_else(|| "ce".into()).to_ascii_lowercase(),
            night: a.iter().any(|x| x == "--night"),
            tod_seed: get(&a, "--tod-seed").and_then(|s| s.parse().ok()).unwrap_or(6),
            start_base: get(&a, "--base"),
            start: None,
            freecam: a.iter().any(|x| x == "--freecam"),
            weather: weather_arg(&a),
            cloud_cover: cover_arg(&a),
        }
    }
}
