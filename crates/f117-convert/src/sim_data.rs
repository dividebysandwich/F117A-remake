//! Combat simulation tables (VGAME.EXE + theater tile objects) -> assets/sim/sim_data.bin (postcard).

use std::path::Path;

use anyhow::Result;
use f117_formats::GameDir;

pub fn convert(game: &GameDir, out: &Path) -> Result<()> {
    let data = f117_sim::SimData::load(game)?;
    let dir = out.join("sim");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("sim_data.bin"), data.to_postcard()?)?;
    println!("sim: combat tables written to {}", dir.join("sim_data.bin").display());
    Ok(())
}
