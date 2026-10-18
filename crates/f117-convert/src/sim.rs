//! Simulation tables read from VGAME.EXE -> assets/sim/tables.ron.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

/// DGROUP (DS) start inside the unpacked VGAME load module.
const DS: usize = (0x383B - 0x1A18) * 16;

#[derive(Serialize)]
struct SimTables {
    /// g-load (1/16 g) by |roll| >> 8 (DS:2D15), as in F-15 SE II's g_rollGeeTable.
    roll_gee: Vec<u8>,
    /// Runway direction per theater (DS:3D7C): +1 = start south of the base heading north,
    /// -1 = start north heading south (mission-generation.md "How VGAME uses it").
    start_dir: Vec<i16>,
}

pub fn convert(vgame_image: &[u8], out: &Path) -> Result<()> {
    let dir = out.join("sim");
    std::fs::create_dir_all(&dir)?;
    let start_dir = (0..9).map(|i| i16::from_le_bytes([vgame_image[DS + 0x3D7C + 2 * i], vgame_image[DS + 0x3D7D + 2 * i]])).collect();
    let t = SimTables { roll_gee: vgame_image[DS + 0x2D15..DS + 0x2D15 + 128].to_vec(), start_dir };
    crate::images::write_ron(&dir.join("tables.ron"), &t)
}
