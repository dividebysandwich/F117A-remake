//! Missions: the original START generator (crate f117-mission, byte-exact with the original)
//! running on data exported by f117-convert (assets/mission/*.json).

use std::sync::Arc;

use bevy::prelude::*;
use f117_mission::{GameData, Mission, MissionKind, MissionSettings, Plane, Seed, Sky};

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

/// Menu choices -> generator settings.
pub fn settings(theater: usize, roe: u8, kind: MissionKind, opponents: u8, lockheed: bool, weather: Option<Sky>, cloud_cover: Option<u8>) -> MissionSettings {
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

pub fn generate(data: &MissionData, settings: &MissionSettings, seed: u16) -> anyhow::Result<Mission> {
    f117_mission::generate(&data.0, settings, Seed::Srand(seed))
}

/// Strips START's text control bytes (colour switches >= 0x80, CR).
pub fn clean_text(s: &str) -> String {
    s.chars().filter(|&c| (c as u32) < 0x80 && c != '\r').collect()
}

/// Per-theater runway direction (VGAME DS:3D7C), exported in assets/sim/tables.ron.
pub fn start_pose(m: &Mission, start_dir: &[i16]) -> StartPose {
    let t = start_dir.get(m.settings.theater).copied().unwrap_or(1) as f64;
    let (bx, by) = (m.takeoff.position.x as f64, m.takeoff.position.y as f64);
    // VGAME 3A92: ViewX = x<<5 + 2, ViewY = (0x8000 - y) << 5; land base: ViewY -= t*0x708;
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
        // Palette fade seed k = (row0 + row1) & 15 (mission-generation.md).
        tod_seed: ((m.primary.row + m.secondary.row) & 15) as u8,
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
    let theater = get("--theater-index").and_then(|s| s.parse().ok()).unwrap_or(3);
    // The menu's other choices (also used when the menu relaunches the game in VR).
    let roe = get("--roe").and_then(|s| s.parse().ok()).unwrap_or(1);
    let kind = get("--kind")
        .and_then(|s| s.parse::<u8>().ok())
        .and_then(|n| MissionKind::ALL.into_iter().find(|k| *k as u8 == n))
        .unwrap_or(MissionKind::Strike);
    let opponents = get("--opponents").and_then(|s| s.parse().ok()).unwrap_or(1);
    let lockheed = args.iter().any(|a| a == "--lockheed");
    let weather = crate::settings::weather_arg(&args);
    let cloud_cover = crate::settings::cover_arg(&args);
    let data = MissionData::load().expect("mission data");
    let m = generate(&data, &settings(theater, roe, kind, opponents, lockheed, weather, cloud_cover), seed).expect("mission");
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
    let mut setup = setup_from_mission(&m, &tables.start_dir);
    setup.freecam = args.iter().any(|a| a == "--freecam");
    app.insert_resource(setup)
        .insert_resource(ActiveMission(m))
        .insert_resource(data)
        .insert_state(crate::settings::AppState::Flight);
}

/// Theater index of a theater code ("ce" -> 3).
pub fn theater_index(code: &str) -> Option<usize> {
    f117_formats::THEATERS.iter().position(|t| t.eq_ignore_ascii_case(code))
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
        match generate(&data, &settings(theater, 1, MissionKind::Strike, 1, false, setup.weather, setup.cloud_cover), seed) {
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
