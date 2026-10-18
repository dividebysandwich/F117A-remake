use anyhow::{Context, Result, ensure};
use f117_formats::{GameDir, THEATERS, terrain::TileSets};
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
    pub fn parse(d: &[u8]) -> Result<Self> {
        let u = |o: usize| -> Result<u16> {
            d.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).context("WLD truncated")
        };
        let s = |o: usize| -> Result<i16> { u(o).map(|v| v as i16) };
        let i = |o: usize| -> Result<i32> {
            d.get(o..o + 4).map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]])).context("WLD truncated")
        };
        let header = u(0)?;
        let n = u(2)? as usize;
        let ground_unit_count = u(4)?;
        let world_object_count = u(6)?;
        ensure!(n <= TABLE && (world_object_count as usize) <= n, "WLD: bad object counts");
        let mut o = 8;
        let mut objects = Vec::with_capacity(n);
        for _ in 0..n {
            objects.push(WorldObject {
                name: s(o)?,
                x: s(o + 2)?,
                y: s(o + 4)?,
                unit_type: s(o + 6)?,
                flags: s(o + 8)?,
                occupant_type: s(o + 10)?,
                patrol_count: s(o + 12)?,
                object_idx: s(o + 14)?,
            });
            o += 16;
        }
        let nf = u(o)? as usize;
        o += 2;
        let mut flights = Vec::with_capacity(nf);
        for _ in 0..nf {
            flights.push(FlightUnit {
                waypoint: s(o)?,
                x: s(o + 2)?,
                y: s(o + 4)?,
                alt: s(o + 6)?,
                xp: i(o + 8)?,
                yp: i(o + 12)?,
                heading: s(o + 16)?,
                pitch: s(o + 18)?,
                roll: s(o + 20)?,
                plane_type: s(o + 22)?,
                flags: s(o + 24)?,
                speed: s(o + 26)?,
                fuel: s(o + 28)?,
                reserved: [s(o + 30)?, s(o + 32)?, s(o + 34)?],
            });
            o += 36;
        }
        ensure!(nf >= 4, "WLD: fewer than 4 flight units");
        let mut take = |len: usize| -> Result<Vec<u8>> {
            let v = d.get(o..o + len).context("WLD truncated")?.to_vec();
            o += len;
            Ok(v)
        };
        let buf7 = take(TABLE)?;
        let buf8 = take(TABLE)?;
        let object_types = take(TABLE)?.into_iter().map(|b| b as i8).collect();
        let grid = take(256)?;
        let names_raw = d[o..d.len().min(o + NAMES_BYTES)].to_vec();
        let names = name_table(&names_raw);
        Ok(Self { header, ground_unit_count, world_object_count, objects, flights, buf7, buf8, object_types, grid, names_raw, names })
    }

    /// Name table entry, "" when out of range.
    pub fn name(&self, i: i32) -> &str {
        if (0..TABLE as i32).contains(&i) { self.names.get(i as usize).map_or("", String::as_str) } else { "" }
    }

    /// Category of a shape (objectTypeTable), 0 if out of range.
    pub fn ott(&self, shape: usize) -> i32 {
        self.object_types.get(shape).map_or(0, |&v| v as i32)
    }
}

/// Name pointer table: entry 0 is the start of the block, entry i the string after the i-th
/// NUL (START parseWorld; VGAME V:4AE1 builds it the same way).
fn name_table(raw: &[u8]) -> Vec<String> {
    let s = |from: usize| -> String {
        raw[from.min(raw.len())..].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
    };
    let mut names = vec![s(0)];
    for (i, &c) in raw.iter().enumerate() {
        if c == 0 && names.len() < TABLE {
            names.push(s(i + 1));
        }
    }
    names.resize(TABLE, String::new());
    names
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
    pub index: usize,
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
    /// Load `<theater>.WLD/.3DG/.3DT`; `grid_size` is START's table DS:107E.
    pub fn load(game: &GameDir, theater: usize, grid_size: [i16; 5]) -> Result<Self> {
        let code = THEATERS.get(theater).context("theater index out of range")?;
        let world = MissionWorld::parse(&game.read(&format!("{code}.WLD"))?).with_context(|| format!("{code}.WLD"))?;
        let g = game.read(&format!("{code}.3DG"))?;
        ensure!(g.len() >= 1810 && &g[0..2] == b"22", "{code}.3DG: bad file");
        let g = [g[1298..1810].to_vec(), g[786..1298].to_vec(), g[274..786].to_vec(), g[18..274].to_vec(), g[2..18].to_vec()];
        let ts = TileSets::parse(&game.read(&format!("{code}.3DT"))?).with_context(|| format!("{code}.3DT"))?;
        let tiles = ts
            .tiles
            .iter()
            .map(|lv| lv.iter().map(|t| t.iter().map(|o| (o.x, o.y, o.shape)).collect()).collect())
            .collect();
        let mut td = Self { index: theater, world, g, tiles, cand_lod12: vec![Vec::new(); 256], cand_lod3: vec![Vec::new(); 256], grid_size };
        td.build_candidates();
        Ok(td)
    }

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
