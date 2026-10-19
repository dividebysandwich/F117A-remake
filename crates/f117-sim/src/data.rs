use std::path::Path;

use anyhow::{Context, Result};
use f117_formats::{THEATERS, terrain};
use serde::{Deserialize, Serialize};

use crate::math::Trig;

pub const N_THREATS: usize = 23;
pub const N_AIRCRAFT: usize = 29;
pub const N_MISSILES: usize = 41;
pub const N_WEAPONS: usize = 20;

/// Radar type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreatType {
    pub name: String,
    /// Maximum detection range in km (R = map >> 6).
    pub lethality: i16,
    pub tier: i16,
    /// bit 0 Doppler, bit 1 omni-directional airborne (AWACS).
    pub flags: i16,
}

/// Aircraft type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AircraftType {
    pub name: String,
    pub nato: String,
    pub max_speed: i16,
    pub range: i16,
    pub agility: i16,
    /// Radar type index; 0 = none, -1 = friendly.
    pub radar: i16,
    /// STFLT.3DX model index.
    pub model: i16,
    pub unknown: i16,
    /// Initial kill counter.
    pub kills: i16,
}

/// Missile / SAM / player weapon projectile type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissileType {
    pub name: String,
    /// km.
    pub lock_range: i16,
    /// speed = max_speed >> 6.
    pub max_speed: i16,
    /// Seeker class: <= 0 infrared, > 0 radar; 4..=6 guided air-to-ground, 7 air-to-air.
    pub class: i16,
    pub turn: i16,
    /// STFLT.3DX model index.
    pub model: i16,
}

/// Player store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponType {
    pub short_name: String,
    pub long_name: String,
    /// Index into the missile table; -1 camera, -2 extra fuel, 0 guns.
    pub spec: i16,
    pub picture: i16,
}

/// Per-theater tile-object data used by the nearest-tile-object search and the wreck shape test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TheaterTiles {
    pub grid: terrain::Grid,
    /// `objects[level-1][tile type]` = (x, y, shape) for LOD levels 1 and 2.
    pub objects: [Vec<Vec<(i16, i16, u8)>>; 2],
    /// Theater .3DX attribute byte per shape (bit 0x20: the model has its own wreck state).
    pub model_attr: Vec<u8>,
}

/// All static data the simulation needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimData {
    pub threats: Vec<ThreatType>,
    pub aircraft: Vec<AircraftType>,
    pub missiles: Vec<MissileType>,
    pub weapons: Vec<WeaponType>,
    /// Weapon-vs-category effectiveness `compat[w][cat]`, 20 rows.
    pub compat: Vec<[i8; 16]>,
    /// Dogfight manoeuvre table, 3 x 8 x 8 int16.
    pub maneuver: Vec<i16>,
    pub trig: Trig,
    /// Countermeasure stores at start ([_, flare, chaff, decoy]).
    pub stores_init: [i16; 4],
    /// Gun rounds at start.
    pub gun_ammo_init: i16,
    /// Runway direction per theater: +1 start south heading north, -1 the reverse.
    pub start_dir: [i16; 9],
    /// One per original theater, stored in `theaters/<code>.json` beside `sim.json`.
    #[serde(skip)]
    pub theaters: Vec<TheaterTiles>,
}

impl SimData {

    /// Read `sim.json` and `theaters/<code>.json` from `dir` (`assets/sim`).
    pub fn load_json(dir: &Path) -> Result<Self> {
        let read = |name: &str| std::fs::read(dir.join(name)).with_context(|| format!("{}", dir.join(name).display()));
        let mut d: Self = serde_json::from_slice(&read("sim.json")?).context("sim.json")?;
        d.theaters = THEATERS
            .iter()
            .map(|code| {
                let name = format!("theaters/{}.json", code.to_ascii_lowercase());
                serde_json::from_slice(&read(&name)?).context(name)
            })
            .collect::<Result<_>>()?;
        Ok(d)
    }

    /// Write `sim.json` and `theaters/<code>.json` under `dir`.
    pub fn write_json(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir.join("theaters"))?;
        std::fs::write(dir.join("sim.json"), serde_json::to_string_pretty(self)?)?;
        for (t, code) in self.theaters.iter().zip(THEATERS) {
            std::fs::write(dir.join(format!("theaters/{}.json", code.to_ascii_lowercase())), serde_json::to_string(t)?)?;
        }
        Ok(())
    }

    pub fn threat(&self, i: i32) -> Option<&ThreatType> {
        usize::try_from(i).ok().and_then(|i| self.threats.get(i))
    }

    pub fn aircraft_type(&self, i: i32) -> &AircraftType {
        &self.aircraft[(i.max(0) as usize).min(self.aircraft.len() - 1)]
    }

    pub fn missile(&self, i: i32) -> &MissileType {
        &self.missiles[(i.max(0) as usize).min(self.missiles.len() - 1)]
    }

    pub fn weapon(&self, i: i32) -> &WeaponType {
        &self.weapons[(i.max(0) as usize).min(self.weapons.len() - 1)]
    }

    /// Weapon-vs-category effectiveness given the category directly.
    pub fn compat_cat(&self, w: i32, cat: i32) -> i32 {
        self.compat.get(w.max(0) as usize).map_or(0, |r| r[(cat & 0xF) as usize] as i32)
    }
}

impl TheaterTiles {
    /// LOD1/LOD2 tile type at (col, row) (row counted north-up), -1 outside the grid.
    pub fn cell(&self, level: usize, col: i32, row: i32) -> i32 {
        let dim = terrain::GRID_DIM[level];
        if col < 0 || row < 0 || col >= dim || row >= dim {
            return -1;
        }
        self.grid.tile(level, col, row) as i32
    }

    pub fn objects_of(&self, level: usize, cell: i32) -> &[(i16, i16, u8)] {
        if cell < 0 {
            return &[];
        }
        self.objects.get(level - 1).and_then(|l| l.get(cell as usize)).map_or(&[], Vec::as_slice)
    }
}
