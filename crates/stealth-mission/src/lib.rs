pub mod drones;
mod generator;
pub mod insertion;
pub mod rng;
pub mod tables;
mod text;
pub mod weather;
pub mod world;

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use stealth_formats::ALL_THEATERS;

pub use drones::DroneWave;
pub use rng::MscRand;
pub use tables::{MissionRow, PlaneInfo, StartTables};
pub use text::{Briefing, Orders, OrdersLine};
pub use weather::{CloudKind, Layer, Sky, Weather};
pub use world::{FlightUnit, MissionWorld, TheaterData, WorldObject};

pub const THEATER_NAMES: [&str; 10] = [
    "Libya",
    "Persian Gulf",
    "North Cape",
    "Central Europe",
    "Vietnam",
    "Middle East",
    "Desert Storm",
    "Cuba",
    "Korea",
    "Ukraine",
];
pub const ROE_NAMES: [&str; 3] = ["Cold War", "Limited War", "Conventional War"];
pub const OPPONENT_NAMES: [&str; 4] = ["Green", "Regular", "Veteran", "Elite"];

/// Mission kind. Bit 0 = strike, bit 1 = training.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MissionKind {
    Air = 0,
    #[default]
    Strike = 1,
    AirTraining = 2,
    StrikeTraining = 3,
    /// The remake's drone defense (theaters with drone launch sites): generated as an air
    /// mission, then turned into an intercept of a Geran wave.
    DroneDefense = 4,
}

impl MissionKind {
    /// The classic mission kinds.
    pub const ALL: [MissionKind; 4] =
        [Self::Air, Self::Strike, Self::AirTraining, Self::StrikeTraining];
    /// Generator kind (bit 0 strike, bit 1 training).
    pub fn index(self) -> u8 {
        match self {
            Self::DroneDefense => 0,
            k => k as u8,
        }
    }
    pub fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }
    /// Number on the command line (`--kind`).
    pub fn id(self) -> u8 {
        self as u8
    }
    pub fn from_id(i: u8) -> Option<Self> {
        Self::ALL.into_iter().chain([Self::DroneDefense]).find(|k| k.id() == i)
    }
    pub fn is_strike(self) -> bool {
        self.index() & 1 != 0
    }
    pub fn is_training(self) -> bool {
        self.index() & 2 != 0
    }
}

/// F-117A realism option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Plane {
    /// 4 bays, carriers allowed.
    #[default]
    Fictional = 0,
    /// 2 bays, no carriers, no AIM-9 / AIM-120.
    Lockheed = 1,
}

/// Player choices that feed the generator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionSettings {
    /// Index into [`ALL_THEATERS`] / [`THEATER_NAMES`].
    pub theater: usize,
    /// 0 Cold War, 1 Limited War, 2 Conventional War.
    pub roe: u8,
    pub kind: MissionKind,
    /// Opponent quality 0 Green .. 3 Elite (`diff`).
    pub opponents: u8,
    pub plane: Plane,
    /// Missions flown; only used for the briefing date.
    pub sorties: u16,
    /// Pilot name (with rank) for the briefing.
    pub pilot: String,
    /// Sky to fly in; `None` rolls it from the seed. The default is a clear sky (classic).
    #[serde(default = "clear_sky")]
    pub weather: Option<Sky>,
    /// Coverage (percent) of the weather's main cloud layer; `None` rolls it.
    #[serde(default)]
    pub cloud_cover: Option<u8>,
    /// Hour (0-23) the mission starts at; `None`: 06:00-08:00 by day or 18:00-20:00 by night,
    /// rolled from the seed.
    #[serde(default)]
    pub start_hour: Option<u8>,
    /// Moon phase in eighths of the cycle (0 new, 4 full); `None`: the briefing date's. Only
    /// the renderer's sky uses it.
    #[serde(default)]
    pub moon: Option<u8>,
    /// Flown in a helicopter: short missions, targets a short flight from a friendly base.
    #[serde(default)]
    pub helicopter: bool,
}

fn clear_sky() -> Option<Sky> {
    Some(Sky::Clear)
}

impl Default for MissionSettings {
    fn default() -> Self {
        Self {
            theater: 3,
            roe: 2,
            kind: MissionKind::Strike,
            opponents: 1,
            plane: Plane::Fictional,
            sorties: 0,
            pilot: "Pilot".into(),
            weather: clear_sky(),
            cloud_cover: None,
            start_hour: None,
            moon: None,
            helicopter: false,
        }
    }
}

/// How the RNG is seeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Seed {
    /// `holdrand` at generator entry; no weapon is marked unavailable.
    Holdrand(u32),
    /// `srand(s)`, then the weapon availability roll, then generate, like a fresh game start.
    Srand(u16),
}

/// Exact generator inputs, for reproducing a generator state bit for bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenInput {
    /// `holdrand` at generator entry.
    pub holdrand: u32,
    /// Per weapon: 1 = out of stock.
    pub weapon_unavailable: [u8; 20],
    /// Stale "special aircraft start" value left by a previous mission (0 at start).
    pub stale_special_start: i16,
}

/// Mission generator tables plus the theaters, loaded once from the game directory (the nine
/// classic theaters) and shipped as JSON in `assets/mission` (`tables.json`,
/// `theaters/<code>.json`, with the remake's own theaters after the classic ones).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameData {
    pub tables: StartTables,
    pub theaters: Vec<TheaterData>,
    /// Briefing wording (`briefings.json`).
    pub briefings: text::BriefingTexts,
}

impl GameData {
    /// Read the JSON export written by [`GameData::write_json`].
    pub fn load_json(dir: &Path) -> Result<Self> {
        Self::load_with(|name| {
            std::fs::read(dir.join(name)).with_context(|| format!("{}", dir.join(name).display()))
        })
    }

    /// [`GameData::load_json`] with the files' contents from `read` (a name such as
    /// `tables.json`).
    pub fn load_with(read: impl Fn(&str) -> Result<Vec<u8>>) -> Result<Self> {
        let read = |name: String| read(&name);
        let tables = serde_json::from_slice(&read("tables.json".into())?).context("tables.json")?;
        let theaters = ALL_THEATERS
            .iter()
            .map(|code| {
                let name = format!("theaters/{}.json", code.to_ascii_lowercase());
                TheaterData::from_json(&read(name.clone())?).context(name)
            })
            .collect::<Result<_>>()?;
        let briefings =
            serde_json::from_slice(&read("briefings.json".into())?).context("briefings.json")?;
        Ok(Self { tables, theaters, briefings })
    }

    /// Write `tables.json` and `theaters/<code>.json` under `dir`.
    pub fn write_json(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir.join("theaters"))?;
        std::fs::write(dir.join("tables.json"), serde_json::to_string_pretty(&self.tables)?)?;
        std::fs::write(dir.join("briefings.json"), serde_json::to_string_pretty(&self.briefings)?)?;
        for (t, code) in self.theaters.iter().zip(ALL_THEATERS) {
            std::fs::write(
                dir.join(format!("theaters/{}.json", code.to_ascii_lowercase())),
                serde_json::to_string(t)?,
            )?;
        }
        Ok(())
    }
}

/// A map position (map units: x east, y south; world = map << 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapPoint {
    pub x: i32,
    pub y: i32,
}

/// A world position in 32-bit world units (x east, y north).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldPoint {
    pub x: i32,
    pub y: i32,
}

/// Primary or secondary target (`targets[k]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    /// 1 photo recon, 2 destroy, 3 airdrop at beacon, 4 land at secret airstrip,
    /// 5 An-72 known route, 6 An-72 leaving, 7 Tu-95 heading home, 8 aircraft / fighter patrol,
    /// 9 helicopter insertion ([`insertion`]).
    pub mission_type: i16,
    pub type_name: String,
    /// Mission table row.
    pub row: i16,
    /// Code word: low byte = row flags, high byte = special model id.
    pub code: i16,
    /// code & 0x02.
    pub has_deadline: bool,
    /// code & 0x10: waypoint snapped to the 1024 grid.
    pub area_target: bool,
    /// code & 0x20: turns to rubble at the deadline ("cargo unloaded").
    pub rubble_at_deadline: bool,
    /// Extra 3D model the flight loads for this target (code >> 8), e.g. 0x23 = SS missile.
    pub special_model: Option<u8>,
    /// Object index in [`MissionWorld::objects`] (0..2 are generator scratch slots).
    pub object_index: i16,
    /// Base used for this target's distance (targets[0].base is the takeoff base).
    pub base_index: i16,
    pub place_name: String,
    /// Shape / type-name index (objectIdx & 0x7F).
    pub shape: u8,
    pub shape_name: String,
    /// objectIdx & 0x100: a tile object placed into a scratch slot by the generator.
    pub placed_by_generator: bool,
    pub position: MapPoint,
    /// Target category (objectTypeTable of the shape).
    pub category: i8,
    /// Weapon suitability class (shape class & 0xF).
    pub weapon_class: u8,
    /// ONC grid reference, e.g. "CC45".
    pub onc: String,
    pub onc_bytes: [u8; 4],
    /// targets[k].dist (primary: deadline >> 4; secondary: 0).
    pub dist: i16,
    /// Briefing intelligence paragraph.
    pub intelligence: String,
    /// Briefing objective sentence.
    pub objective: String,
}

/// Takeoff or landing base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Base {
    pub object_index: i16,
    pub name: String,
    pub position: MapPoint,
    pub carrier: bool,
    pub neutral: bool,
    /// Generator distance `bd[k]` (map units, incl. jitter).
    pub distance: i16,
}

/// The mission's special aircraft (flight unit 0, types 5..=8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecialAircraft {
    pub flight_unit: usize,
    pub plane_type: i16,
    pub plane_name: String,
    /// Object it was positioned at, and the object it flies to.
    pub start_object: i16,
    pub start_name: String,
    pub destination_object: i16,
    pub destination_name: String,
    /// ONC reference used by the briefing (types 5..7).
    pub reference_onc: Option<String>,
    pub flight: FlightUnit,
}

/// Mission clock. 1 tick = 2 s; the clock starts at 06:00 by day, 18:00 by night.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionTime {
    /// Ticks after the 06:00 / 18:00 base at mission start.
    pub tod_ticks: i32,
    /// 6 or 18, or the chosen start hour.
    pub base_hour: u8,
    /// Briefing time strings (rounded down to 5 min): departure, deadline, deadline + 15 min.
    pub depart: String,
    pub deadline: String,
    pub deadline_plus_15: String,
    /// mtime: deadline in ticks after mission start, as handed to the flight.
    pub deadline_ticks: i16,
    /// Whether any target uses the deadline (code & 2) or a special aircraft schedule.
    pub has_deadline: bool,
}

/// One weapon bay: weapon type and round count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bay {
    pub weapon: i16,
    pub name: String,
    pub count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loadout {
    /// All 4 bay words as the generator writes them.
    pub bays: Vec<Bay>,
    /// Usable bays: 4 fictional, 2 Lockheed.
    pub usable_bays: usize,
}

/// RNG bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngInfo {
    pub holdrand_entry: u32,
    pub holdrand_exit: u32,
    pub rand_calls: u32,
}

/// A fully generated mission: the decoded hand-off block plus everything the briefing shows
/// and the flight receives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mission {
    pub settings: MissionSettings,
    pub theater_code: String,
    pub theater_name: String,
    pub rng: RngInfo,
    pub weapon_unavailable: Vec<u8>,
    pub primary: Target,
    pub secondary: Target,
    pub takeoff: Base,
    pub landing: Base,
    /// Route length estimate in "km" (dist >> 6 sums).
    pub route_length: i16,
    /// 0 midpoint, 1 primary (snapped if area target), 2 secondary, 3 landing base.
    pub waypoints: [MapPoint; 4],
    /// Player start (computed from the takeoff base).
    pub start_position: WorldPoint,
    pub special_aircraft: Option<SpecialAircraft>,
    /// Flight unit index of the special aircraft, -1 none.
    pub escort: i16,
    /// The edited theater world handed to the flight.
    pub world: MissionWorld,
    pub night: bool,
    /// Rolled from its own RNG stream.
    #[serde(default)]
    pub weather: Weather,
    pub time: MissionTime,
    pub fuel_lbs: i16,
    pub loadout: Loadout,
    /// Some grid cell still has "hostile forces concentration".
    pub hostile_concentrations: bool,
    /// kind > 1 (training).
    pub invulnerable: bool,
    /// Generator exit code: 0x0D carrier takeoff, 0x0C land base.
    pub exit_code: u8,
    pub briefing: Briefing,
    pub orders: Orders,
    /// Attack drones in the air.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drones: Option<DroneWave>,
}

/// Roll the weapon availability flags after `srand`.
pub fn weapon_availability(rng: &mut MscRand, tables: &StartTables, plane: Plane) -> [u8; 20] {
    generator::weapon_availability(rng, tables, plane == Plane::Lockheed)
}

/// Generate a mission.
pub fn generate(data: &GameData, settings: &MissionSettings, seed: Seed) -> Result<Mission> {
    let input = match seed {
        Seed::Holdrand(h) => {
            GenInput { holdrand: h, weapon_unavailable: [0; 20], stale_special_start: 0 }
        }
        Seed::Srand(s) => {
            let mut r = MscRand::srand(s);
            let avail = weapon_availability(&mut r, &data.tables, settings.plane);
            GenInput { holdrand: r.state, weapon_unavailable: avail, stale_special_start: 0 }
        }
    };
    generate_exact(data, settings, &input)
}

/// Generate from exact generator inputs.
pub fn generate_exact(data: &GameData, s: &MissionSettings, input: &GenInput) -> Result<Mission> {
    ensure!(s.theater < data.theaters.len(), "theater {} out of range", s.theater);
    ensure!(s.roe <= 2, "roe {} out of range", s.roe);
    ensure!(s.opponents <= 3, "opponents {} out of range", s.opponents);
    let x = &data.tables;
    let th = &data.theaters[s.theater];
    let lockheed = s.plane == Plane::Lockheed;
    let g = generator::Gen::new(
        x,
        th,
        input.holdrand,
        s.roe,
        s.kind.index(),
        s.opponents,
        lockheed,
        input.weapon_unavailable,
        input.stale_special_start,
    )
    .start_hour(s.start_hour)
    .helicopter(s.helicopter)
    .run();
    let mut avail = input.weapon_unavailable;
    avail[0] = lockheed as u8;
    avail[1] = lockheed as u8;
    let ctx = text::TextCtx {
        x,
        texts: &data.briefings,
        seed: input.holdrand,
        g: &g,
        theater: th.index,
        date: th.date,
        kind: s.kind.index() as usize,
        roe: s.roe as usize,
        lockheed,
    };
    let w = &g.world;
    let obj = |i: i32| w.objects[i.rem_euclid(w.objects.len() as i32) as usize];

    let target = |k: usize| -> Target {
        let t = &g.targets[k];
        let o = obj(t.idx);
        let sh = o.shape();
        let code = t.code as i16;
        let hi = (t.code >> 8) & 0xFF;
        Target {
            mission_type: t.ty as i16,
            type_name: type_name(t.ty).into(),
            row: t.num as i16,
            code,
            has_deadline: code & 2 != 0,
            area_target: code & 0x10 != 0,
            rubble_at_deadline: code & 0x20 != 0,
            special_model: (hi != 0).then_some(hi as u8),
            object_index: t.idx as i16,
            base_index: t.base as i16,
            place_name: if o.name != 0 { w.name(o.name as i32).into() } else { String::new() },
            shape: sh as u8,
            shape_name: w.name(sh as i32).into(),
            placed_by_generator: o.object_idx & 0x100 != 0,
            position: MapPoint { x: o.x as i32, y: o.y as i32 },
            category: w.ott(sh) as i8,
            weapon_class: w.shape_class.get(sh).copied().unwrap_or(0) & 0xF,
            onc: t.coord.iter().map(|&c| c as char).collect(),
            onc_bytes: t.coord,
            dist: t.dist as i16,
            intelligence: ctx.summary(k),
            objective: ctx.objective(k),
        }
    };
    let base = |k: usize| -> Base {
        let i = g.targets[k].base;
        let o = obj(i);
        Base {
            object_index: i as i16,
            name: ctx.base_name(i),
            position: MapPoint { x: o.x as i32, y: o.y as i32 },
            carrier: o.flags & world::flags::CARRIER != 0,
            neutral: o.flags & world::flags::NEUTRAL != 0,
            distance: g.bd[k] as i16,
        }
    };
    let special_aircraft = g.special.map(|r| {
        let f = w.flights[0];
        SpecialAircraft {
            flight_unit: 0,
            plane_type: f.plane_type,
            plane_name: x.plane(f.plane_type).name,
            start_object: r.start as i16,
            start_name: ctx.base_name(r.start),
            destination_object: r.dest as i16,
            destination_name: ctx.base_name(r.dest),
            reference_onc: g.intercept_ref.map(|b| b.iter().map(|&c| c as char).collect()),
            flight: f,
        }
    });
    let takeoff = base(0);
    let primary = target(0);
    let secondary = target(1);
    let usable_bays = if lockheed { 2 } else { 4 };
    let weather = Weather::roll(input.holdrand, s.theater, s.weather, s.cloud_cover);
    let mut briefing = ctx.briefing(s.sorties, &s.pilot);
    briefing.weather_line = weather.describe();
    let mut m = Mission {
        settings: s.clone(),
        theater_code: ALL_THEATERS[s.theater].into(),
        theater_name: THEATER_NAMES[s.theater].into(),
        rng: RngInfo {
            holdrand_entry: input.holdrand,
            holdrand_exit: g.rng.state,
            rand_calls: g.rng.calls,
        },
        weapon_unavailable: avail.to_vec(),
        time: MissionTime {
            tod_ticks: g.tod,
            base_hour: g.base_hour as u8,
            depart: g.depart.clone(),
            deadline: g.deadline.clone(),
            deadline_plus_15: g.deadline2.clone(),
            deadline_ticks: g.mtime as i16,
            has_deadline: primary.has_deadline
                || secondary.has_deadline
                || (5..=7).contains(&primary.mission_type),
        },
        primary,
        secondary,
        exit_code: if takeoff.carrier { 0x0D } else { 0x0C },
        takeoff,
        landing: base(1),
        route_length: g.total as i16,
        waypoints: g.waypoints.map(|(x, y)| MapPoint { x, y }),
        start_position: WorldPoint { x: g.base_xp, y: g.base_yp },
        special_aircraft,
        escort: g.escort as i16,
        night: g.night,
        weather,
        fuel_lbs: g.fuel as i16,
        loadout: Loadout {
            bays: (0..4)
                .map(|k| Bay {
                    weapon: g.weapons[k] as i16,
                    name: x.weapon_name(g.weapons[k] as i16).into(),
                    count: g.weapon_qty[k] as u8,
                })
                .collect(),
            usable_bays,
        },
        hostile_concentrations: g.hostile_conc,
        invulnerable: s.kind.index() > 1,
        briefing,
        orders: ctx.orders(),
        world: g.world.clone(),
        drones: None,
    };
    if s.kind == MissionKind::DroneDefense {
        drones::defend(&mut m, data, th)?;
    } else if s.kind == MissionKind::Strike && !th.launch_sites.is_empty() {
        drones::crossing(&mut m, th);
    }
    Ok(m)
}

fn type_name(t: i32) -> &'static str {
    match t {
        1 => "photo recon",
        2 => "destroy",
        3 => "airdrop supplies at beacon",
        4 => "land at secret airstrip",
        5 => "intercept transport (known route)",
        6 => "intercept transport (leaving, destination unknown)",
        7 => "intercept Tu-95 (last seen, heading to base)",
        8 => "destroy aircraft/patrol",
        _ => "",
    }
}
