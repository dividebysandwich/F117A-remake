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
    /// Extra scenery in the green theaters (towns, roads, power lines, trees); `--no-scenery`.
    pub scenery: bool,
}

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
}

/// Aircraft start position and heading (original world units, radians clockwise from north).
#[derive(Debug, Clone, Copy)]
pub struct StartPose {
    pub east_wu: f64,
    pub north_wu: f64,
    pub heading: f32,
    pub carrier: bool,
}

fn args() -> Vec<String> {
    std::env::args().collect()
}

fn get(args: &[String], f: &str) -> Option<String> {
    args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned()
}

impl Settings {
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
            scenery: !has("--no-scenery"),
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
        }
    }
}
