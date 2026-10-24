use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Size of the shape-indexed tables (shape classes, object types) and of the name table.
pub const TABLE: usize = 128;

/// World object flags.
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

/// World object (base, landmark or ground unit). Values are kept as the raw signed words.
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

/// Flight unit: an aircraft group with its position, waypoint and state.
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
    /// Not touched by the generator.
    pub reserved: [i16; 3],
}

/// The theater world that is handed to the flight, after the generator's edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionWorld {
    /// Shape left by a destroyed object on land.
    pub land_rubble: u8,
    /// Shape left by a destroyed object at sea.
    pub water_rubble: u8,
    pub ground_unit_count: u16,
    /// `worldObjectCount`: objects `[woc, n)` are bases/landmarks.
    pub world_object_count: u16,
    pub objects: Vec<WorldObject>,
    pub flights: Vec<FlightUnit>,
    /// Per shape; low nibble = weapon suitability class, 0 = not a tile target.
    pub shape_class: Vec<u8>,
    /// Target category per shape (0 = not a target). Not part of the hand-off block.
    pub object_types: Vec<i8>,
    /// 16x16 territory grid, `grid[row*16+col]`; `&3 == 0` enemy, 0x10 hostile concentration.
    pub grid: Vec<u8>,
    /// Place and type names, by name index.
    pub names: Vec<String>,
}

impl MissionWorld {
    /// Name table entry, "" when out of range.
    pub fn name(&self, i: i32) -> &str {
        if (0..TABLE as i32).contains(&i) {
            self.names.get(i as usize).map_or("", String::as_str)
        } else {
            ""
        }
    }

    /// Target category of a shape, 0 if out of range.
    pub fn ott(&self, shape: usize) -> i32 {
        self.object_types.get(shape).map_or(0, |&v| v as i32)
    }
}

/// Point (px, py) in or on triangle `t` (`TheaterData::ground`).
fn inside(t: &[f32; 7], px: f64, py: f64) -> bool {
    let v = |i: usize| (t[2 * i] as f64, t[2 * i + 1] as f64);
    let side =
        |(ax, ay): (f64, f64), (bx, by): (f64, f64)| (bx - ax) * (py - ay) - (by - ay) * (px - ax);
    let (d0, d1, d2) = (side(v(0), v(1)), side(v(1), v(2)), side(v(2), v(0)));
    !((d0 < 0.0 || d1 < 0.0 || d2 < 0.0) && (d0 > 0.0 || d1 > 0.0 || d2 > 0.0))
}

/// One entry of the random-object-of-shape candidate list.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) struct Candidate {
    pub ox: i16,
    pub oy: i16,
    /// Tile column / row in the level's units (LOD3: already `col << 4`).
    pub a: i32,
    pub e: i32,
    pub level: u8,
}

/// Everything the generator loads for one theater.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TheaterData {
    /// Classic theater whose generator rules apply (mission table rows, dates, grid references):
    /// its own index for the classic theaters.
    pub index: usize,
    /// Briefing start date (month, day, year) of a remake theater.
    #[serde(default)]
    pub date: Option<(u8, u8, u16)>,
    /// World objects the enemy launches attack drones from (remake theaters).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launch_sites: Vec<i16>,
    pub world: MissionWorld,
    /// Terrain tile grids: g4 (4x4), g3 (16x16), g2/g1/g0 (32 blocks of 4x4).
    pub(crate) g: [Vec<u8>; 5],
    /// `tiles[level][tile type]` = (x, y, shape).
    pub(crate) tiles: Vec<Vec<Vec<(i16, i16, u8)>>>,
    /// Pre-built candidate lists per shape for the LOD1/LOD2 search and the LOD3 search
    /// (in the random object search's scan order). Derived, so not
    /// serialised: rebuilt by [`TheaterData::from_json`].
    #[serde(skip)]
    pub(crate) cand_lod12: Vec<Vec<Candidate>>,
    #[serde(skip)]
    pub(crate) cand_lod3: Vec<Vec<Candidate>>,
    /// Grid size per LOD level.
    pub(crate) grid_size: [i16; 5],
    /// Per shape, its flat triangles as the 3D view draws them, in drawing order:
    /// `[x0, y0, x1, y1, x2, y2, water]` in the level's units (x east, y north), water 1 for
    /// the sea and lakes (`tools/models/water_shapes.py`). Empty for a theater without water.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) ground: Vec<Vec<[f32; 7]>>,
}

impl TheaterData {
    /// Decode a theater exported with [`serde_json`] and rebuild the candidate lists.
    pub fn from_json(json: &[u8]) -> Result<Self> {
        let mut td: Self = serde_json::from_slice(json)?;
        td.build_candidates();
        Ok(td)
    }

    /// Tile type at (col, row) on a LOD level; -1 outside the grid.
    pub(crate) fn lookup(&self, lv: usize, col: i32, row: i32) -> i32 {
        let gs = self.grid_size[lv] as i32;
        if col < 0 || row < 0 || gs <= col || gs <= row {
            return -1;
        }
        let at = |v: &Vec<u8>, i: i32| {
            if i < 0 { -1 } else { v.get(i as usize).map_or(-1, |&b| b as i32) }
        };
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

    /// Water at a map point (x east, y south): the topmost flat triangle there is the sea or a
    /// lake, finer levels drawn over coarser ones and later tile objects over earlier ones (as
    /// `scenery::ground` sees the 3D view). Bare ground is land.
    pub fn is_water(&self, x: i32, y: i32) -> bool {
        if self.ground.is_empty() {
            return false;
        }
        // World units, north up.
        let (wx, wy) = ((x as f64) * 32.0, ((0x8000 - y) as f64) * 32.0);
        let mut top: Option<((usize, usize, usize), bool)> = None;
        for lv in 1..=4usize {
            let unit = [0.0, 1.0, 4.0, 16.0, 64.0][lv];
            let size = 4096.0 * unit;
            let (col, row) = ((wx / size).floor() as i32, (wy / size).floor() as i32);
            // Neighbouring tiles' objects may reach over the edge.
            for (dc, dr) in (-1..=1).flat_map(|dc| (-1..=1).map(move |dr| (dc, dr))) {
                let (c, r) = (col + dc, row + dr);
                let cell = self.lookup(lv, c, r);
                if cell == -1 {
                    continue;
                }
                let (cx, cy) =
                    ((c as f64 * 4096.0 + 2048.0) * unit, (r as f64 * 4096.0 + 2048.0) * unit);
                for (k, &(ox, oy, sh)) in self.tile_objects(lv, cell).iter().enumerate() {
                    let Some(tris) = self.ground.get(sh as usize) else { continue };
                    let (px, py) = ((wx - cx) / unit - ox as f64, (wy - cy) / unit - oy as f64);
                    for (j, t) in tris.iter().enumerate() {
                        let order = (4 - lv, k, j);
                        if top.is_some_and(|(o, _)| o > order) || !inside(t, px, py) {
                            continue;
                        }
                        top = Some((order, t[6] != 0.0));
                    }
                }
            }
        }
        top.is_some_and(|(_, water)| water)
    }

    /// Scan the enemy tiles once and bucket the objects by shape, keeping the scan order of
    /// the random object search. The territory bits of the grid are never changed
    /// by the generator, so the lists are valid for every mission of the theater.
    fn build_candidates(&mut self) {
        let grid = &self.world.grid;
        let enemy =
            |col: i32, row_north_up: i32| grid[((15 - row_north_up) * 16 + col) as usize] & 3 == 0;
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
                    l3[sh as usize].push(Candidate {
                        ox,
                        oy,
                        a: (a << 4) & 0xFF,
                        e: (e << 4) & 0xFF,
                        level: 3,
                    });
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
                    let c =
                        if lv == 1 { self.lookup(1, a, e) } else { self.lookup(2, a >> 2, e >> 2) };
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
