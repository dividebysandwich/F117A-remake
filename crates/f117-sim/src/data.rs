use anyhow::{Context, Result, ensure};
use f117_formats::{GameDir, THEATERS, terrain};
use serde::{Deserialize, Serialize};

use crate::math::Trig;

/// DGROUP offset inside the unpacked VGAME load module.
pub const DS: usize = (0x383B - 0x1A18) * 16;

pub const THREAT_TABLE: usize = 0x2E80;
pub const AIRCRAFT_TABLE: usize = 0x2FC2;
pub const MISSILE_TABLE: usize = 0x3362;
pub const WEAPON_TABLE: usize = 0x3662;
pub const COMPAT_TABLE: usize = 0x386A;
pub const MANEUVER_TABLE: usize = 0x3A8C;
pub const SINE_TABLE: usize = 0x2084;
pub const STORES_INIT: usize = 0x3636;
pub const GUN_AMMO_INIT: usize = 0x3650;
pub const START_DIR: usize = 0x3D7C;

pub const N_THREATS: usize = 23;
pub const N_AIRCRAFT: usize = 29;
pub const N_MISSILES: usize = 41;
pub const N_WEAPONS: usize = 20;

/// Radar type (DS:2E80, 14-byte records).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreatType {
    pub name: String,
    /// Maximum detection range in km (R = map >> 6).
    pub lethality: i16,
    pub tier: i16,
    /// bit 0 Doppler, bit 1 omni-directional airborne (AWACS).
    pub flags: i16,
}

/// Aircraft type (DS:2FC2, 32-byte records).
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
    /// Initial kill counter (+0x1E).
    pub kills: i16,
}

/// Missile / SAM / player weapon projectile type (DS:3362, 18-byte records).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissileType {
    pub name: String,
    /// km.
    pub lock_range: i16,
    /// speed = max_speed >> 6.
    pub max_speed: i16,
    /// Seeker class (combat.md §4.1).
    pub class: i16,
    pub turn: i16,
    /// STFLT.3DX model index.
    pub model: i16,
}

/// Player store (DS:3662, 26-byte records).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponType {
    pub short_name: String,
    pub long_name: String,
    /// Index into the missile table; -1 camera, -2 extra fuel, 0 guns.
    pub spec: i16,
    pub picture: i16,
}

/// Per-theater tile-object data used by `findNearestTile` (V:0D08) and the wreck shape test.
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
    /// Weapon-vs-category effectiveness `compat[w][cat]` (DS:386A), 20 rows.
    pub compat: Vec<[i8; 16]>,
    /// Dogfight manoeuvre table (DS:3A8C), 3 x 8 x 8 int16.
    pub maneuver: Vec<i16>,
    pub trig: Trig,
    /// Countermeasure stores at start (DS:3636: [_, flare, chaff, decoy]).
    pub stores_init: [i16; 4],
    /// Gun rounds at start (DS:3650).
    pub gun_ammo_init: i16,
    /// Runway direction per theater (DS:3D7C): +1 start south heading north, -1 the reverse.
    pub start_dir: [i16; 9],
    pub theaters: Vec<TheaterTiles>,
}

fn cstr(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>().trim().to_string()
}

impl SimData {
    /// Read everything from an original game directory.
    pub fn load(game: &GameDir) -> Result<Self> {
        let vgame = game.vgame()?;
        let mut d = Self::from_vgame_image(&vgame.image)?;
        for (i, th) in THEATERS.iter().enumerate() {
            let lod4 = vgame.image.get(terrain::LOD4_GRIDS_OFFSET + i * 64..).context("VGAME: no LOD4 grids")?;
            let grid = terrain::Grid::parse(&game.read(&format!("{th}.3DG"))?, lod4)?;
            let ts = terrain::TileSets::parse(&game.read(&format!("{th}.3DT"))?)?;
            let lv = |l: usize| -> Vec<Vec<(i16, i16, u8)>> {
                ts.tiles.get(l).map(|v| v.iter().map(|t| t.iter().map(|o| (o.x, o.y, o.shape)).collect()).collect()).unwrap_or_default()
            };
            let x3d = f117_formats::x3d::parse(&game.read(&format!("{th}.3DX"))?).with_context(|| format!("{th}.3DX"))?;
            d.theaters.push(TheaterTiles { grid, objects: [lv(1), lv(2)], model_attr: x3d.models.iter().map(|m| m.attr).collect() });
        }
        Ok(d)
    }

    /// The VGAME tables only (no theater tiles).
    pub fn from_vgame_image(img: &[u8]) -> Result<Self> {
        ensure!(img.len() > DS + 0x3D90, "VGAME image too small");
        let b = |o: usize| &img[DS + o..];
        let w = |o: usize| i16::from_le_bytes([img[DS + o], img[DS + o + 1]]);
        let threats = (0..N_THREATS)
            .map(|i| {
                let o = THREAT_TABLE + 14 * i;
                ThreatType { name: cstr(&b(o)[..8]), lethality: w(o + 8), tier: w(o + 10), flags: w(o + 12) }
            })
            .collect();
        let aircraft = (0..N_AIRCRAFT)
            .map(|i| {
                let o = AIRCRAFT_TABLE + 32 * i;
                AircraftType {
                    name: cstr(&b(o)[..7]),
                    nato: cstr(&b(o + 7)[..11]),
                    max_speed: w(o + 0x12),
                    range: w(o + 0x14),
                    agility: w(o + 0x16),
                    radar: w(o + 0x18),
                    model: w(o + 0x1A),
                    unknown: w(o + 0x1C),
                    kills: w(o + 0x1E),
                }
            })
            .collect();
        let missiles = (0..N_MISSILES)
            .map(|i| {
                let o = MISSILE_TABLE + 18 * i;
                MissileType {
                    name: cstr(&b(o)[..8]),
                    lock_range: w(o + 8),
                    max_speed: w(o + 10),
                    class: w(o + 12),
                    turn: w(o + 14),
                    model: w(o + 16),
                }
            })
            .collect();
        let weapons = (0..N_WEAPONS)
            .map(|i| {
                let o = WEAPON_TABLE + 26 * i;
                WeaponType { short_name: cstr(&b(o)[..10]), long_name: cstr(&b(o + 10)[..12]), spec: w(o + 22), picture: w(o + 24) }
            })
            .collect();
        let compat = (0..N_WEAPONS)
            .map(|r| {
                let mut row = [0i8; 16];
                for (c, v) in row.iter_mut().enumerate() {
                    *v = img[DS + COMPAT_TABLE + r * 16 + c] as i8;
                }
                row
            })
            .collect();
        let maneuver = (0..192).map(|i| w(MANEUVER_TABLE + 2 * i)).collect();
        let trig = Trig { lut: (0..257).map(|i| w(SINE_TABLE + 2 * i)).collect() };
        let stores_init = [w(STORES_INIT), w(STORES_INIT + 2), w(STORES_INIT + 4), w(STORES_INIT + 6)];
        let start_dir = std::array::from_fn(|i| w(START_DIR + 2 * i));
        Ok(Self {
            threats,
            aircraft,
            missiles,
            weapons,
            compat,
            maneuver,
            trig,
            stores_init,
            gun_ammo_init: w(GUN_AMMO_INIT),
            start_dir,
            theaters: Vec::new(),
        })
    }

    /// Decode `assets/sim/sim_data.bin` (postcard).
    pub fn from_postcard(bytes: &[u8]) -> Result<Self> {
        Ok(postcard::from_bytes(bytes)?)
    }

    /// Encode as postcard (the `assets/sim/sim_data.bin` format).
    pub fn to_postcard(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(self)?)
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

    /// `compat(w, cat)` (V:8BAB) given the category directly.
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
