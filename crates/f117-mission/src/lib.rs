mod generator;
pub mod rng;
pub mod tables;
mod text;
pub mod world;

use anyhow::{Result, ensure};
use f117_formats::{GameDir, THEATERS};
use serde::{Deserialize, Serialize};

pub use rng::MscRand;
pub use tables::{MissionRow, PlaneInfo, StartTables};
pub use text::{Briefing, Orders, OrdersLine};
pub use world::{FlightUnit, MissionWorld, NAMES_BYTES, TheaterData, WorldObject};

pub const THEATER_NAMES: [&str; 9] =
    ["Libya", "Persian Gulf", "North Cape", "Central Europe", "Vietnam", "Middle East", "Desert Storm", "Cuba", "Korea"];
pub const ROE_NAMES: [&str; 3] = ["Cold War", "Limited War", "Conventional War"];
pub const OPPONENT_NAMES: [&str; 4] = ["Green", "Regular", "Veteran", "Elite"];

/// Mission kind (game+0x3C, §1). Bit 0 = strike, bit 1 = training.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MissionKind {
    Air = 0,
    #[default]
    Strike = 1,
    AirTraining = 2,
    StrikeTraining = 3,
}

impl MissionKind {
    pub const ALL: [MissionKind; 4] = [Self::Air, Self::Strike, Self::AirTraining, Self::StrikeTraining];
    pub fn index(self) -> u8 {
        self as u8
    }
    pub fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }
    pub fn is_strike(self) -> bool {
        self.index() & 1 != 0
    }
    pub fn is_training(self) -> bool {
        self.index() & 2 != 0
    }
}

/// F-117A realism option (game+0x42, §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Plane {
    /// 4 bays, carriers allowed.
    #[default]
    MicroProse = 0,
    /// 2 bays, no carriers, no AIM-9 / AIM-120.
    Lockheed = 1,
}

/// Player choices that feed the generator (§1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionSettings {
    /// Index into [`THEATERS`] / [`THEATER_NAMES`].
    pub theater: usize,
    /// 0 Cold War, 1 Limited War, 2 Conventional War.
    pub roe: u8,
    pub kind: MissionKind,
    /// Opponent quality 0 Green .. 3 Elite (`diff`).
    pub opponents: u8,
    pub plane: Plane,
    /// Missions flown (game+0x36); only used for the briefing date.
    pub sorties: u16,
    /// Pilot name (with rank) for the briefing.
    pub pilot: String,
}

impl Default for MissionSettings {
    fn default() -> Self {
        Self { theater: 3, roe: 2, kind: MissionKind::Strike, opponents: 1, plane: Plane::MicroProse, sorties: 0, pilot: "Pilot".into() }
    }
}

/// How the RNG is seeded (§2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Seed {
    /// `holdrand` at `missionGenerate` entry; no weapon is marked unavailable
    /// (like `mission_gen.py --seed` without `--avail`).
    Holdrand(u32),
    /// `srand(s)`, then the weapon availability roll (1A18:0E23), then generate, like a fresh
    /// START run (`mission_gen.py --srand`).
    Srand(u16),
}

/// Exact generator inputs, for reproducing an emulator state bit for bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenInput {
    /// `holdrand` (DS:AE8C) at `missionGenerate` entry.
    pub holdrand: u32,
    /// DS:D878[20]: 1 = weapon out of stock.
    pub weapon_unavailable: [u8; 20],
    /// DS:CBDC: stale "special aircraft start" global left by a previous mission (0 at start).
    pub stale_cbdc: i16,
}

/// START.EXE tables plus all nine theaters, loaded once from the game directory
/// (serialisable, so f117-convert can ship it as an asset).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameData {
    pub tables: StartTables,
    pub theaters: Vec<TheaterData>,
}

impl GameData {
    pub fn load(game: &GameDir) -> Result<Self> {
        let tables = StartTables::load(game)?;
        let theaters = (0..THEATERS.len()).map(|t| TheaterData::load(game, t, tables.grid_size)).collect::<Result<_>>()?;
        Ok(Self { tables, theaters })
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

/// Primary or secondary target (`targets[k]`, §5 / §5.4 / §5.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    /// 1 photo recon, 2 destroy, 3 airdrop at beacon, 4 land at secret airstrip,
    /// 5 An-72 known route, 6 An-72 leaving, 7 Tu-95 heading home, 8 aircraft / fighter patrol.
    pub mission_type: i16,
    pub type_name: String,
    /// Mission table row (DS:11DA).
    pub row: i16,
    /// Code word: low byte = row flags, high byte = special model id (§5.4).
    pub code: i16,
    /// code & 0x02.
    pub has_deadline: bool,
    /// code & 0x10: waypoint snapped to the 1024 grid.
    pub area_target: bool,
    /// code & 0x20: turns to rubble at the deadline ("cargo unloaded").
    pub rubble_at_deadline: bool,
    /// Extra 3D model VGAME loads for this target (code >> 8), e.g. 0x23 = SS missile.
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
    /// Weapon suitability class (buf7 & 0xF).
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

/// The mission's special aircraft (flight unit 0, types 5..=8, §5.5).
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
    /// DS:0D8E ONC reference used by the briefing (types 5..7).
    pub reference_onc: Option<String>,
    pub flight: FlightUnit,
}

/// Mission clock (§5.9). 1 tick = 2 s; the clock starts at 06:00 by day, 18:00 by night.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionTime {
    /// Ticks after the 06:00 / 18:00 base at mission start (DS:9870 in VGAME).
    pub tod_ticks: i32,
    /// 6 or 18.
    pub base_hour: u8,
    /// START strings (rounded down to 5 min): departure DS:0D88, deadline DS:0D7C,
    /// deadline + 15 min DS:0D82.
    pub depart: String,
    pub deadline: String,
    pub deadline_plus_15: String,
    /// mtime (DS:D876): deadline in ticks after mission start, as handed to VGAME.
    pub deadline_ticks: i16,
    /// Whether any target uses the deadline (code & 2) or a special aircraft schedule.
    pub has_deadline: bool,
}

/// One weapon bay (COMM+0x38 / +0x40).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bay {
    pub weapon: i16,
    pub name: String,
    pub count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loadout {
    /// All 4 bay words as START writes them.
    pub bays: Vec<Bay>,
    /// Usable bays: 4 MicroProse, 2 Lockheed.
    pub usable_bays: usize,
}

/// RNG bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngInfo {
    pub holdrand_entry: u32,
    pub holdrand_exit: u32,
    pub rand_calls: u32,
}

/// A fully generated mission: the decoded hand-off block plus everything START shows or
/// writes to COMM.
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
    /// Route length estimate in "km" (dist >> 6 sums, §5.3).
    pub route_length: i16,
    /// 0 midpoint, 1 primary (snapped if area target), 2 secondary, 3 landing base.
    pub waypoints: [MapPoint; 4],
    /// Player start (baseXP/baseYP as START computes them, §5.6).
    pub start_position: WorldPoint,
    pub special_aircraft: Option<SpecialAircraft>,
    /// DS:CABC: flight unit index of the special aircraft, -1 none.
    pub escort: i16,
    /// The edited theater world handed to VGAME.
    pub world: MissionWorld,
    pub night: bool,
    pub time: MissionTime,
    pub fuel_lbs: i16,
    pub loadout: Loadout,
    /// Some grid cell still has "hostile forces concentration" (DS:D33A).
    pub hostile_concentrations: bool,
    /// COMM+0x30 = kind > 1 (training).
    pub invulnerable: bool,
    /// START exit code: 0x0D carrier takeoff, 0x0C land base.
    pub exit_code: u8,
    pub briefing: Briefing,
    pub orders: Orders,
}

/// Roll the weapon availability flags after `srand` (1A18:0E23, §2).
pub fn weapon_availability(rng: &mut MscRand, tables: &StartTables, plane: Plane) -> [u8; 20] {
    generator::weapon_availability(rng, tables, plane == Plane::Lockheed)
}

/// Generate a mission.
pub fn generate(data: &GameData, settings: &MissionSettings, seed: Seed) -> Result<Mission> {
    let input = match seed {
        Seed::Holdrand(h) => GenInput { holdrand: h, weapon_unavailable: [0; 20], stale_cbdc: 0 },
        Seed::Srand(s) => {
            let mut r = MscRand::srand(s);
            let avail = weapon_availability(&mut r, &data.tables, settings.plane);
            GenInput { holdrand: r.state, weapon_unavailable: avail, stale_cbdc: 0 }
        }
    };
    generate_exact(data, settings, &input)
}

/// Load the game data and generate one mission.
pub fn generate_from_dir(game: &GameDir, settings: &MissionSettings, seed: Seed) -> Result<Mission> {
    generate(&GameData::load(game)?, settings, seed)
}

/// Generate from exact emulator inputs.
pub fn generate_exact(data: &GameData, s: &MissionSettings, input: &GenInput) -> Result<Mission> {
    ensure!(s.theater < THEATERS.len(), "theater {} out of range", s.theater);
    ensure!(s.roe <= 2, "roe {} out of range", s.roe);
    ensure!(s.opponents <= 3, "opponents {} out of range", s.opponents);
    let x = &data.tables;
    let th = &data.theaters[s.theater];
    let lockheed = s.plane == Plane::Lockheed;
    let g = generator::Gen::new(x, th, input.holdrand, s.roe, s.kind.index(), s.opponents, lockheed, input.weapon_unavailable, input.stale_cbdc)
        .run();
    let mut avail = input.weapon_unavailable;
    avail[0] = lockheed as u8;
    avail[1] = lockheed as u8;
    let ctx = text::TextCtx { x, g: &g, theater: s.theater, kind: s.kind.index() as usize, roe: s.roe as usize, lockheed };
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
            weapon_class: w.buf7.get(sh).copied().unwrap_or(0) & 0xF,
            onc: t.coord.iter().map(|&c| c as char).collect(),
            onc_bytes: t.coord,
            dist: t.dist as i16,
            intelligence: x.mission_text[t.num as usize].clone(),
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
    Ok(Mission {
        settings: s.clone(),
        theater_code: THEATERS[s.theater].into(),
        theater_name: THEATER_NAMES[s.theater].into(),
        rng: RngInfo { holdrand_entry: input.holdrand, holdrand_exit: g.rng.state, rand_calls: g.rng.calls },
        weapon_unavailable: avail.to_vec(),
        time: MissionTime {
            tod_ticks: g.tod,
            base_hour: if g.night { 18 } else { 6 },
            depart: g.depart.clone(),
            deadline: g.deadline.clone(),
            deadline_plus_15: g.deadline2.clone(),
            deadline_ticks: g.mtime as i16,
            has_deadline: primary.has_deadline || secondary.has_deadline || (5..=7).contains(&primary.mission_type),
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
        fuel_lbs: g.fuel as i16,
        loadout: Loadout {
            bays: (0..4)
                .map(|k| Bay { weapon: g.weapons[k] as i16, name: x.weapon_name(g.weapons[k] as i16).into(), count: g.weapon_qty[k] as u8 })
                .collect(),
            usable_bays,
        },
        hostile_concentrations: g.hostile_conc,
        invulnerable: s.kind.index() > 1,
        briefing: ctx.briefing(s.sorties, &s.pilot),
        orders: ctx.orders(),
        world: g.world.clone(),
    })
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

/// The START -> VGAME hand-off block (`exportWorld` 1A18:5625, copied to COMM+0x7A; §7).
pub fn handoff_block(m: &Mission) -> Vec<u8> {
    let w = &m.world;
    let mut out = Vec::with_capacity(0x2000);
    let w16 = |out: &mut Vec<u8>, v: i32| out.extend_from_slice(&(v as u16).to_le_bytes());
    w16(&mut out, w.header as i32);
    w16(&mut out, w.objects.len() as i32);
    w16(&mut out, w.ground_unit_count as i32);
    w16(&mut out, w.world_object_count as i32);
    for o in &w.objects {
        for v in [o.name, o.x, o.y, o.unit_type, o.flags, o.occupant_type, o.patrol_count, o.object_idx] {
            w16(&mut out, v as i32);
        }
    }
    w16(&mut out, w.flights.len() as i32);
    for f in &w.flights {
        for v in [f.waypoint, f.x, f.y, f.alt] {
            w16(&mut out, v as i32);
        }
        out.extend_from_slice(&f.xp.to_le_bytes());
        out.extend_from_slice(&f.yp.to_le_bytes());
        for v in [f.heading, f.pitch, f.roll, f.plane_type, f.flags, f.speed, f.fuel, f.reserved[0], f.reserved[1], f.reserved[2]] {
            w16(&mut out, v as i32);
        }
    }
    out.extend_from_slice(&w.buf7);
    out.extend_from_slice(&w.buf8);
    let mut names = w.names_raw.clone();
    names.resize(NAMES_BYTES, 0);
    out.extend_from_slice(&names);
    out.extend_from_slice(&w.grid);
    w16(&mut out, m.time.deadline_ticks as i32);
    w16(&mut out, m.escort as i32);
    for p in &m.waypoints {
        w16(&mut out, p.x);
        w16(&mut out, p.y);
    }
    for t in [&m.primary, &m.secondary] {
        for v in [t.mission_type, t.object_index, t.base_index, t.code, t.row] {
            w16(&mut out, v as i32);
        }
        out.extend_from_slice(&t.onc_bytes);
        out.extend_from_slice(&[0, 0]);
        w16(&mut out, t.dist as i32);
    }
    out
}

/// Byte offset of the name block inside [`handoff_block`] (bytes past the WLD file's end are
/// stale memory in the original and are not compared).
pub fn handoff_names_offset(m: &Mission) -> usize {
    8 + 16 * m.world.objects.len() + 2 + 36 * m.world.flights.len() + 256
}
