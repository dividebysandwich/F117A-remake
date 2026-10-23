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

/// The world is drawn: in flight, or behind the menu (menu_scene.rs).
pub fn world_shown(
    state: Res<State<AppState>>,
    scene: Option<Res<crate::menu_scene::MenuScene>>,
) -> bool {
    *state.get() == AppState::Flight || scene.is_some()
}

pub fn configure(app: &mut App) {
    for s in [AppState::Flight, AppState::Viewer] {
        app.configure_sets(
            OnEnter(s),
            (SetupSet::Mission, SetupSet::Palette, SetupSet::World, SetupSet::Spawn).chain(),
        );
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
    /// `--continuous-time`: the sun and moon move with the mission clock, the palette follows
    /// the sun, moonlight sets the enemy's night sight (sky.rs); off, the classic drift.
    pub continuous_time: bool,
    /// `--sun-moon`: without continuous time of day, the sky still shows the sun by day and the
    /// moon and stars by night (sky.rs).
    pub sun_moon: bool,
    /// `--classic-cockpit`: the classic instrument panel layout and 77x60 displays instead
    /// of the modern panel with high-resolution displays and bezel buttons.
    pub classic_cockpit: bool,
    /// `--classic-avionics`: the classic MFD pages (MAP/TAC left, camera/WAY/WPN/ORD right)
    /// instead of the modern formats (HSD, HARM, targeting pod, MAP, SMS on either MFD).
    pub classic_avionics: bool,
    /// `--classic-hud`: the classic low-resolution HUD instead of the F-16 style one.
    pub classic_hud: bool,
    /// `--classic-flight`: the classic flight model instead of the modern aerodynamic one
    /// (the default for `--script` runs unless `--modern-flight` is given).
    pub classic_flight: bool,
    /// `--no-crashes`: crash protection (auto pull-up, no ground crashes, collisions bounce).
    pub no_crashes: bool,
    /// `--realistic-damage`: realistic weapon damage instead of the classic
    /// (`stealth_sim::SimOptions::realistic_damage`).
    pub realistic_damage: bool,
    /// `--realistic-guidance`: realistic missile guidance instead of the classic
    /// (`stealth_sim::SimOptions::realistic_guidance`).
    pub realistic_guidance: bool,
    /// `--tacview`: the flight is saved as a Tacview ACMI file when it ends.
    pub tacview: bool,
    /// `--tacview-real-time`: the flight is sent to Tacview while it is flown
    /// (tacview_real_time.rs).
    pub tacview_real_time: bool,
    /// `--tacview-address <ip[:port]|port>`: where the Tacview session is hosted.
    pub tacview_address: std::net::SocketAddr,
    /// `--tacview-password <text>`: the password Tacview must give to join.
    pub tacview_password: Option<String>,
    /// `--air-defence`: low-level air defences (`stealth_sim::SimOptions::air_defence`).
    pub air_defence: bool,
    /// `--head-tracking`: TrackIR-style head tracking through OpenTrack (desktop, `headtrack`).
    pub head_tracking: bool,
    /// `--vr-cockpit-scale <percent>`: size of the cockpit (and the world) in the headset,
    /// 100% = the flown aircraft's `AircraftSpec::vr_scale`.
    pub vr_cockpit_scale: f32,
    /// `--aircraft f117|f16`: the flown aircraft.
    pub aircraft: crate::aircraft::AircraftKind,
}

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

/// Aircraft start position and heading (world units, radians clockwise from north).
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

    /// The MFDs are the high-resolution ones (every cockpit but the classic one, which has them
    /// too with the modern avionics).
    pub fn hires_displays(&self) -> bool {
        self.cockpit().0 != crate::cockpit3d::layout::PanelStyle::Classic || !self.classic_avionics
    }

    /// Modern HUD scale (canvas pixels per unit of the tangent plane) of the aircraft, on the
    /// screen or in VR.
    pub fn hud_scale(&self) -> f32 {
        let spec = self.aircraft.spec();
        if self.vr { spec.vr_hud_scale } else { spec.hud_scale }
    }

    /// The eye point and cockpit in the aircraft frame: the design eye point, in VR raised to
    /// the aircraft's `vr_eye_height` above the ground as seen through the rig scale.
    pub fn cockpit_eye(&self) -> Vec3 {
        let spec = self.aircraft.spec();
        let mut eye = spec.eye;
        if self.vr
            && let Some(h) = spec.vr_eye_height
        {
            eye.y = eye.y.max(h / self.vr_rig_scale());
        }
        eye
    }

    /// Scale of the VR rig (head and hand offsets) for the chosen cockpit size.
    pub fn vr_rig_scale(&self) -> f32 {
        self.vr_cockpit_scale * self.aircraft.spec().vr_scale
    }

    pub fn from_args() -> Self {
        let a = args();
        let has = |f: &str| a.iter().any(|x| x == f);
        let initial_state = if has("--viewer") {
            AppState::Viewer
        } else if has("--connect") || has("--host") {
            // Multiplayer test runs go through the menu (`--theater` and `--script` with them).
            AppState::Menu
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
            continuous_time: has("--continuous-time"),
            sun_moon: has("--sun-moon"),
            classic_cockpit: has("--classic-cockpit"),
            classic_hud: has("--classic-hud"),
            classic_avionics: has("--classic-avionics"),
            classic_flight: has("--classic-flight") || (has("--script") && !has("--modern-flight")),
            no_crashes: has("--no-crashes"),
            realistic_damage: has("--realistic-damage"),
            realistic_guidance: has("--realistic-guidance"),
            tacview: has("--tacview"),
            tacview_real_time: has("--tacview-real-time"),
            tacview_address: get(&a, "--tacview-address")
                .and_then(|s| crate::tacview_real_time::parse_address(&s))
                .unwrap_or(crate::tacview_real_time::DEFAULT_ADDRESS),
            tacview_password: get(&a, "--tacview-password").filter(|p| !p.is_empty()),
            air_defence: has("--air-defence"),
            head_tracking: has("--head-tracking"),
            vr_cockpit_scale: get(&a, "--vr-cockpit-scale")
                .and_then(|s| s.parse::<f32>().ok())
                .map_or(DEFAULT_VR_COCKPIT_SCALE, |p| {
                    (p / 100.0)
                        .clamp(*VR_COCKPIT_SCALE_RANGE.start(), *VR_COCKPIT_SCALE_RANGE.end())
                }),
            aircraft: get(&a, "--aircraft")
                .and_then(|s| crate::aircraft::AircraftKind::parse(&s))
                .unwrap_or_default(),
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
