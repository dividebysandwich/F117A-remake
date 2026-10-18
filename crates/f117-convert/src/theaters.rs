//! Per-theater world data -> assets/theaters/<id>.ron: tile grid (.3DG + LOD4 table), tile
//! object lists (.3DT) and the world/target database (.WLD). The ground itself is made of
//! theater models placed per tile; their geometry is in the theater's .glb.

use std::path::Path;

use anyhow::{Context, Result};
use f117_formats::{GameDir, THEATERS, terrain, wld};
use serde::Serialize;

#[derive(Serialize)]
struct Theater {
    id: String,
    /// Index used by the original game (order of the theater tables in VGAME/START).
    index: usize,
    grid: terrain::Grid,
    tiles: terrain::TileSets,
    world: wld::World,
    /// Number of models in the theater's .3DX (shapes beyond it are appended specials).
    model_count: usize,
}

pub fn convert(game: &GameDir, out: &Path) -> Result<()> {
    let dir = out.join("theaters");
    std::fs::create_dir_all(&dir)?;
    for (index, id) in THEATERS.iter().enumerate() {
        let t = Theater {
            id: id.to_ascii_lowercase(),
            index,
            grid: game.grid(id)?,
            tiles: terrain::TileSets::parse(&game.read(&format!("{id}.3DT"))?).context(*id)?,
            world: wld::World::parse(&game.read(&format!("{id}.WLD"))?).context(*id)?,
            model_count: f117_formats::x3d::parse(&game.read(&format!("{id}.3DX"))?)?.models.len(),
        };
        crate::images::write_ron(&dir.join(format!("{}.ron", t.id)), &t)?;
    }
    println!("theaters: {} written to {}", THEATERS.len(), dir.display());
    Ok(())
}
