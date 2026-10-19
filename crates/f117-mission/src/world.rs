use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const NAMES_BYTES: usize = 750;
/// Size of the shape-indexed tables (buf7, buf8, objectTypeTable) and of the name table.
pub const TABLE: usize = 128;

/// Object flags (spec §3).
pub mod flags {
    pub const AIRFIELD: i16 = 0x001;
    pub const SHIP: i16 = 0x008;
    pub const OFF_THE_AIR: i16 = 0x040;
    pub const UNDER_REPAIR: i16 = 0x080;
    pub const NEUTRAL: i16 = 0x100;
    pub const CARRIER: i16 = 0x200;
    pub const FRIENDLY: i16 = 0x400;
    pub const DISABLED: i16 = 0x800;
}

/// 16-byte world object (spec §3 "WorldObject"). Values are kept as the raw signed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorldObject {
    /// Place name index.
    pub name: i16,
    /// Map units, east.
    pub x: i16,
    /// Map units, south.
    pub y: i16,
    /// SAM / radar type, 0 = none.
    pub unit_type: i16,
    pub flags: i16,
    /// Fighter type based here.
    pub occupant_type: i16,
    /// Squadrons based here.
    pub patrol_count: i16,
    /// Low 7 bits: shape (= type name index); +0x100: placed by the generator.
    pub object_idx: i16,
}

impl WorldObject {
    pub fn shape(&self) -> usize {
        (self.object_idx & 0x7F) as usize
    }
}

/// 36-byte flight unit (spec §3 "FlightUnit").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FlightUnit {
    /// Waypoint object index.
    pub waypoint: i16,
    pub x: i16,
    pub y: i16,
    pub alt: i16,
    pub xp: i32,
    pub yp: i32,
    pub heading: i16,
    pub pitch: i16,
    pub roll: i16,
    pub plane_type: i16,
    /// 0x80 patrol near the route, 0x40 wide range, 0x100 escort of the special aircraft,
    /// 0x4 special aircraft heading to its waypoint.
    pub flags: i16,
    pub speed: i16,
    pub fuel: i16,
    /// Not touched by START.
    pub reserved: [i16; 3],
}

/// The theater world that is handed to VGAME (spec §7), after the generator's edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionWorld {
    /// lo = land rubble shape, hi = water rubble shape.
    pub header: u16,
    pub ground_unit_count: u16,
    /// `worldObjectCount`: objects `[woc, n)` are bases/landmarks.
    pub world_object_count: u16,
    pub objects: Vec<WorldObject>,
    pub flights: Vec<FlightUnit>,
    /// Per shape; low nibble = weapon suitability class.
    pub buf7: Vec<u8>,
    pub buf8: Vec<u8>,
    /// objectTypeTable: target category per shape (0 = not a target). Not exported to VGAME.
    pub object_types: Vec<i8>,
    /// 16x16 territory grid, `grid[row*16+col]`; `&3 == 0` enemy, 0x10 hostile concentration.
    pub grid: Vec<u8>,
    /// Name bytes as read from the file (at most 750; the hand-off pads with zeros).
    pub names_raw: Vec<u8>,
    /// Name table as START/VGAME index it: entry i is the string after the i-th NUL.
    pub names: Vec<String>,
}

impl MissionWorld {

    /// Name table entry, "" when out of range.
    pub fn name(&self, i: i32) -> &str {
        if (0..TABLE as i32).contains(&i) { self.names.get(i as usize).map_or("", String::as_str) } else { "" }
    }

    /// Category of a shape (objectTypeTable), 0 if out of range.
    pub fn ott(&self, shape: usize) -> i32 {
        self.object_types.get(shape).map_or(0, |&v| v as i32)
    }
}

/// One entry of the `findRandomObjectOfShape` candidate list (spec §4).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) struct Candidate {
    pub ox: i16,
    pub oy: i16,
    /// Tile column / row in the level's units (LOD3: already `col << 4`).
    pub a: i32,
    pub e: i32,
    pub level: u8,
}

/// Everything START loads for one theater.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TheaterData {
    /// Original theater whose START rules apply (mission table rows, dates, grid references):
    /// its own index for the original theaters.
    pub index: usize,
    /// Briefing start date (month, day, year) of a remake theater.
    #[serde(default)]
    pub date: Option<(u8, u8, u16)>,
    pub world: MissionWorld,
    /// `.3DG` grids: g4 (4x4), g3 (16x16), g2/g1/g0 (32 blocks of 4x4).
    pub(crate) g: [Vec<u8>; 5],
    /// `tiles[level][tile type]` = (x, y, shape).
    pub(crate) tiles: Vec<Vec<Vec<(i16, i16, u8)>>>,
    /// Pre-built candidate lists per shape for the LOD1/LOD2 search and the LOD3 search
    /// (same order as the original scan; spec §4 findRandomObjectOfShape). Derived, so not
    /// serialised: rebuilt by [`TheaterData::from_json`].
    #[serde(skip)]
    pub(crate) cand_lod12: Vec<Vec<Candidate>>,
    #[serde(skip)]
    pub(crate) cand_lod3: Vec<Vec<Candidate>>,
    /// Grid size per LOD level (START DS:107E).
    pub(crate) grid_size: [i16; 5],
}

impl TheaterData {

    /// Decode a theater exported with [`serde_json`] and rebuild the candidate lists.
    pub fn from_json(json: &[u8]) -> Result<Self> {
        let mut td: Self = serde_json::from_slice(json)?;
        td.build_candidates();
        Ok(td)
    }

    /// `lookupGrid(level, col, row)` (1A18:7C56); -1 outside the grid.
    pub(crate) fn lookup(&self, lv: usize, col: i32, row: i32) -> i32 {
        let gs = self.grid_size[lv] as i32;
        if col < 0 || row < 0 || gs <= col || gs <= row {
            return -1;
        }
        let at = |v: &Vec<u8>, i: i32| if i < 0 { -1 } else { v.get(i as usize).map_or(-1, |&b| b as i32) };
        match lv {
            4 => at(&self.g[4], col + row * 4),
            3 => at(&self.g[3], col + row * 16),
            _ => {
                let p = self.lookup(lv + 1, col >> 2, row >> 2);
                at(&self.g[lv], (col & 3) + (row & 3) * 4 + p * 16)
            }
        }
    }

    pub(crate) fn tile_objects(&self, lv: usize, cell: i32) -> &[(i16, i16, u8)] {
        self.tiles.get(lv).and_then(|l| l.get(cell as usize)).map_or(&[], Vec::as_slice)
    }

    /// Scan the enemy tiles once and bucket the objects by shape, keeping the scan order of
    /// `findRandomObjectOfShape` (1A18:7904). The territory bits of the grid are never changed
    /// by the generator, so the lists are valid for every mission of the theater.
    fn build_candidates(&mut self) {
        let grid = &self.world.grid;
        let enemy = |col: i32, row_north_up: i32| grid[((15 - row_north_up) * 16 + col) as usize] & 3 == 0;
        let mut l3 = vec![Vec::new(); 256];
        for a in 0..16 {
            for e in 0..16 {
                if !enemy(a, e) {
                    continue;
                }
                let c = self.lookup(3, a, e);
                if c == -1 {
                    continue;
                }
                for &(ox, oy, sh) in self.tile_objects(3, c) {
                    l3[sh as usize].push(Candidate { ox, oy, a: (a << 4) & 0xFF, e: (e << 4) & 0xFF, level: 3 });
                }
            }
        }
        let mut l12 = vec![Vec::new(); 256];
        for (lv, step) in [(1usize, 1usize), (2, 4)] {
            for a in (0..256).step_by(step) {
                for e in (0..256).step_by(step) {
                    if !enemy(a >> 4, e >> 4) {
                        continue;
                    }
                    let c = if lv == 1 { self.lookup(1, a, e) } else { self.lookup(2, a >> 2, e >> 2) };
                    if c == -1 {
                        continue;
                    }
                    for &(ox, oy, sh) in self.tile_objects(lv, c) {
                        l12[sh as usize].push(Candidate { ox, oy, a, e, level: lv as u8 });
                    }
                }
            }
        }
        self.cand_lod3 = l3;
        self.cand_lod12 = l12;
    }
}
