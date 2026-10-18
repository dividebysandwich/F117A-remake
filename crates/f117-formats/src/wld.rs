//! `.WLD` theater world / target database (START's `parseWorld`, with 128-entry tables in
//! F-117A). Format: docs/re/formats-3d.md section 8 and docs/re/f15se2-gamelogic.md.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const TABLE: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldObject {
    /// Name index.
    pub name: u16,
    /// Map units (world = map << 5; map y grows southward).
    pub x: u16,
    pub y: u16,
    pub unit_type: i16,
    /// 0x100 airbase, 0x200 large, 0x400 waypoint, 0x800 disabled.
    pub target_flags: i16,
    pub occupant_type: i16,
    pub patrol_count: i16,
    pub object_idx: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub header: u16,
    pub ground_unit_count: u16,
    pub world_object_count: u16,
    pub objects: Vec<WorldObject>,
    /// Raw 36-byte flight unit records (layout in f15se2-gamelogic.md).
    pub flights: Vec<Vec<u8>>,
    pub table7: Vec<u8>,
    pub table8: Vec<u8>,
    pub object_types: Vec<u8>,
    /// 16x16 terrain class grid (one cell per LOD3 tile); low 2 bits = class, 0 = land.
    pub terrain: Vec<u8>,
    pub names: Vec<String>,
}

impl World {
    pub fn parse(b: &[u8]) -> Result<Self> {
        let rd = |p: usize| -> Result<u16> {
            b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| anyhow::anyhow!("WLD truncated"))
        };
        let header = rd(0)?;
        let n = rd(2)? as usize;
        let ground_unit_count = rd(4)?;
        let world_object_count = rd(6)?;
        let mut p = 8;
        let mut objects = Vec::with_capacity(n);
        for _ in 0..n {
            objects.push(WorldObject {
                name: rd(p)?,
                x: rd(p + 2)?,
                y: rd(p + 4)?,
                unit_type: rd(p + 6)? as i16,
                target_flags: rd(p + 8)? as i16,
                occupant_type: rd(p + 10)? as i16,
                patrol_count: rd(p + 12)? as i16,
                object_idx: rd(p + 14)? as i16,
            });
            p += 16;
        }
        let nf = rd(p)? as usize;
        p += 2;
        let flights = (0..nf).map(|i| b[p + 36 * i..p + 36 * (i + 1)].to_vec()).collect();
        p += 36 * nf;
        let mut take = |n: usize| -> Result<Vec<u8>> {
            ensure!(p + n <= b.len(), "WLD truncated");
            p += n;
            Ok(b[p - n..p].to_vec())
        };
        let table7 = take(TABLE)?;
        let table8 = take(TABLE)?;
        let object_types = take(TABLE)?;
        let terrain = take(256)?;
        let names: Vec<String> =
            b[p..].split(|&c| c == 0).map(|s| String::from_utf8_lossy(s).into_owned()).take(TABLE).collect();
        Ok(Self { header, ground_unit_count, world_object_count, objects, flights, table7, table8, object_types, terrain, names })
    }

    pub fn name(&self, i: u16) -> &str {
        self.names.get(i as usize).map_or("", String::as_str)
    }
}
