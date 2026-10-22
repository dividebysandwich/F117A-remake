//! Missions: the START generator (crate stealth-mission, byte-exact) running on the exported
//! mission data (assets/mission/*.json).

use std::sync::Arc;

use bevy::prelude::*;
use stealth_mission::{GameData, Mission, MissionKind, MissionSettings, Plane, Seed, Sky};

use crate::settings::{MissionSetup, StartPose};

/// Generator input data, loaded on first use.
#[derive(Resource, Clone)]
pub struct MissionData(pub Arc<GameData>);

impl MissionData {
    pub fn load() -> anyhow::Result<Self> {
        Ok(Self(Arc::new(GameData::load_json(&crate::asset_path::asset("mission"))?)))
    }
}

/// The mission being flown.
#[derive(Resource, Clone)]
pub struct ActiveMission(pub Mission);

pub use stealth_campaign::{RouteKind, RoutePoint};

/// The route flown (mission planner); the mission's 4 waypoints unless the player changed it.
#[derive(Resource, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlannedRoute {
    /// The mission it belongs to (theater, kind, seed of the RNG on entry).
    pub mission: (String, u8, u32),
    pub points: Vec<RoutePoint>,
}

impl PlannedRoute {
    /// The mission's own route: midpoint, primary, secondary, landing base.
    pub fn of(m: &Mission) -> Self {
        let kinds = [RouteKind::Nav, RouteKind::Primary, RouteKind::Secondary, RouteKind::Land];
        Self {
            mission: Self::key(m),
            points: m
                .waypoints
                .iter()
                .zip(kinds)
                .map(|(p, kind)| RoutePoint { x: p.x, y: p.y, kind })
                .collect(),
        }
    }

    pub fn key(m: &Mission) -> (String, u8, u32) {
        (m.theater_code.clone(), m.settings.kind.id(), m.rng.holdrand_entry)
    }

    pub fn fits(&self, m: &Mission) -> bool {
        self.mission == Self::key(m)
    }

    /// Route index of the mission's waypoint k (0 midpoint/first point, 1 primary, 2 secondary,
    /// 3 landing base).
    pub fn anchors(&self) -> Vec<usize> {
        let find = |kind| self.points.iter().position(|p| p.kind == kind).unwrap_or(0);
        vec![0, find(RouteKind::Primary), find(RouteKind::Secondary), find(RouteKind::Land)]
    }

    /// Route length in map units.
    pub fn length(&self) -> f64 {
        self.points
            .windows(2)
            .map(|w| {
                (((w[1].x - w[0].x) as f64).powi(2) + ((w[1].y - w[0].y) as f64).powi(2)).sqrt()
            })
            .sum()
    }
}

/// File the menu hands a planned route to the VR process in (`--route`).
pub const ROUTE_FILE: &str = "route.json";

/// Menu choices -> generator settings.
pub fn settings(
    theater: usize,
    roe: u8,
    kind: MissionKind,
    opponents: u8,
    lockheed: bool,
    weather: Option<Sky>,
    cloud_cover: Option<u8>,
) -> MissionSettings {
    MissionSettings {
        theater,
        roe,
        kind,
        opponents,
        plane: if lockheed { Plane::Lockheed } else { Plane::MicroProse },
        weather,
        cloud_cover,
        ..default()
    }
}

pub fn generate(
    data: &MissionData,
    settings: &MissionSettings,
    seed: u16,
) -> anyhow::Result<Mission> {
    stealth_mission::generate(&data.0, settings, Seed::Srand(seed))
}

/// Strips START's text control bytes (colour switches >= 0x80, CR).
pub fn clean_text(s: &str) -> String {
    s.chars().filter(|&c| (c as u32) < 0x80 && c != '\r').collect()
}

/// Per-theater runway direction, exported in assets/sim/tables.ron.
pub fn start_pose(m: &Mission, start_dir: &[i16]) -> StartPose {
    let t = start_dir.get(m.settings.theater).copied().unwrap_or(1) as f64;
    let (bx, by) = (m.takeoff.position.x as f64, m.takeoff.position.y as f64);
    // ViewX = x<<5 + 2, ViewY = (0x8000 - y) << 5; land base: ViewY -= t*0x708;
    // carrier: ViewX -= t<<7, heading + 0x400. Heading 0 (t = +1) or 0x8000.
    let mut east = bx * 32.0 + 2.0;
    let mut north = (32768.0 - by) * 32.0;
    let mut heading = if t > 0.0 { 0.0 } else { 0x8000 as f64 };
    if m.takeoff.carrier {
        east -= t * 128.0;
        heading += 0x400 as f64;
    } else {
        north -= t * 0x708 as f64;
    }
    StartPose {
        east_wu: east,
        north_wu: north,
        heading: (heading / 65536.0 * std::f64::consts::TAU) as f32,
        carrier: m.takeoff.carrier,
    }
}

/// Fills the flight setup from a generated mission.
pub fn setup_from_mission(m: &Mission, start_dir: &[i16]) -> MissionSetup {
    MissionSetup {
        theater: m.theater_code.to_ascii_lowercase(),
        night: m.night,
        // Palette fade seed k = (row0 + row1) & 15; `--tod-seed` overrides it (screenshots).
        tod_seed: std::env::args()
            .skip_while(|a| a != "--tod-seed")
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(((m.primary.row + m.secondary.row) & 15) as u8),
        start_base: Some(m.takeoff.name.clone()),
        start: Some(start_pose(m, start_dir)),
        freecam: false,
        weather: Some(m.weather.sky),
        cloud_cover: m.settings.cloud_cover,
    }
}

/// `--mission <seed> [--theater-index N]`: generate a mission at startup and fly it directly.
pub fn from_args(app: &mut App) {
    let args: Vec<String> = std::env::args().collect();
    let get = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let Some(seed) = get("--mission").and_then(|s| s.parse::<u16>().ok()) else { return };
    let theater = get("--theater-index")
        .and_then(|s| s.parse().ok())
        .or_else(|| get("--theater").and_then(|c| theater_index(&c)))
        .unwrap_or(3);
    // The menu's other choices (also used when the menu relaunches the game in VR).
    let roe = get("--roe").and_then(|s| s.parse().ok()).unwrap_or(1);
    let kind = get("--kind")
        .and_then(|s| s.parse::<u8>().ok())
        .and_then(MissionKind::from_id)
        .unwrap_or(MissionKind::Strike);
    let opponents = get("--opponents").and_then(|s| s.parse().ok()).unwrap_or(1);
    // Realistic weapons are the F-117A's limits.
    let f16 = get("--aircraft").and_then(|s| crate::aircraft::AircraftKind::parse(&s))
        == Some(crate::aircraft::AircraftKind::F16);
    let lockheed = args.iter().any(|a| a == "--lockheed") && !f16;
    let weather = crate::settings::weather_arg(&args);
    let cloud_cover = crate::settings::cover_arg(&args);
    let data = MissionData::load().expect("mission data");
    let mut settings = settings(theater, roe, kind, opponents, lockheed, weather, cloud_cover);
    // `--start-hour <0-23>`: takeoff time.
    settings.start_hour = get("--start-hour").and_then(|s| s.parse::<u8>().ok()).map(|h| h.min(23));
    let mut m = generate(&data, &settings, seed).expect("mission");
    // `--start-base <name>`: take off from another airfield (also an enemy one; for tests).
    if let Some(name) = get("--start-base") {
        let w = &m.world;
        match (0..w.objects.len()).find(|&i| {
            w.objects[i].flags & stealth_mission::world::flags::AIRFIELD != 0
                && w.name(w.objects[i].name as i32).eq_ignore_ascii_case(&name)
        }) {
            Some(i) => {
                let o = w.objects[i];
                // The simulation starts at the primary target's base.
                m.primary.base_index = i as i16;
                m.takeoff = stealth_mission::Base {
                    object_index: i as i16,
                    name: w.name(o.name as i32).into(),
                    position: stealth_mission::MapPoint {
                        x: o.x as u16 as i32,
                        y: o.y as u16 as i32,
                    },
                    carrier: o.flags & stealth_mission::world::flags::CARRIER != 0,
                    neutral: false,
                    distance: 0,
                };
            }
            None => warn!("--start-base {name}: no such airfield"),
        }
    }
    let tables = crate::flight::SimTables::load();
    if args.iter().any(|a| a == "--print-mission") {
        println!(
            "{} -> {}: {} / {} night={} {}",
            m.takeoff.name,
            m.landing.name,
            m.orders.primary,
            m.orders.secondary,
            m.night,
            m.weather.describe()
        );
    }
    // `--career <pilot>`: a tour sortie (the pilot's tour damage applies).
    if let Some(key) = get("--career") {
        app.insert_resource(crate::career::CareerFlight(key));
    }
    // `--route <file>`: the route planned in the menu (when it is for this mission).
    if let Some(route) = get("--route")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<PlannedRoute>(&t).ok())
        .filter(|r| r.fits(&m))
    {
        app.insert_resource(route);
    }
    // `--loadout <file>`: the stores chosen in the menu.
    if let Some(lo) = get("--loadout")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<crate::loadout::PlannedLoadout>(&t).ok())
    {
        app.insert_resource(lo);
    }
    let mut setup = setup_from_mission(&m, &tables.start_dir);
    setup.freecam = args.iter().any(|a| a == "--freecam");
    app.insert_resource(setup)
        .insert_resource(ActiveMission(m))
        .insert_resource(data)
        .insert_state(crate::settings::AppState::Flight);
}

/// Theater index of a theater code ("ce" -> 3).
pub fn theater_index(code: &str) -> Option<usize> {
    stealth_formats::ALL_THEATERS.iter().position(|t| t.eq_ignore_ascii_case(code))
}

/// OnEnter(Flight): every flight flies a mission. Without one (`--fly`, `--theater`, `--script`)
/// a strike mission is generated for the chosen theater with a random seed; the flight setup
/// (theater, night, time of day, start pose) then follows the mission.
pub fn ensure_mission(
    mut commands: Commands,
    existing: Option<Res<ActiveMission>>,
    data: Option<Res<MissionData>>,
    tables: Res<crate::flight::SimTables>,
    mut setup: ResMut<MissionSetup>,
) {
    if existing.is_some() {
        return;
    }
    let data = match data {
        Some(d) => d.clone(),
        None => match MissionData::load() {
            Ok(d) => {
                commands.insert_resource(d.clone());
                d
            }
            Err(e) => {
                warn!("no mission generator data, flying without a mission: {e}");
                return;
            }
        },
    };
    let theater = theater_index(&setup.theater).unwrap_or(3);
    for _ in 0..16 {
        let seed: u16 = rand::random();
        match generate(
            &data,
            &settings(theater, 1, MissionKind::Strike, 1, false, setup.weather, setup.cloud_cover),
            seed,
        ) {
            Ok(m) => {
                info!("generated mission {seed} for {}: {}", m.theater_code, m.orders.primary);
                let freecam = setup.freecam;
                *setup = setup_from_mission(&m, &tables.start_dir);
                setup.freecam = freecam;
                commands.insert_resource(ActiveMission(m));
                return;
            }
            Err(e) => warn!("mission generation failed for seed {seed}: {e:#}"),
        }
    }
}
