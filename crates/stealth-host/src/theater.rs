//! Theater data the simulation needs, without the renderer.

use anyhow::{Context, Result};
use serde::Deserialize;
use stealth_formats::terrain::{Grid, TileSets};
use stealth_mission::GameData;
use stealth_sim::{SimData, TheaterTiles};

/// The part of a remake theater's file (`assets/theaters/<code>.ron`) the simulation uses.
#[derive(Deserialize)]
struct TheaterFile {
    grid: Grid,
    tiles: TileSets,
    #[serde(default)]
    model_attr: Vec<u8>,
}

/// Tile data of a remake theater.
pub fn load_tiles(code: &str) -> Result<TheaterTiles> {
    let rel = format!("theaters/{}.ron", code.to_ascii_lowercase());
    let t: TheaterFile = ron::from_str(&crate::assets::read_to_string(&rel)?).context(rel)?;
    Ok(TheaterTiles::from_tilesets(&t.grid, &t.tiles, &t.model_attr))
}

/// The simulation's tables, with the tiles of theater `index` (`code`) if they are not among
/// the sim data's.
pub fn sim_data(index: usize, code: &str) -> Result<SimData> {
    let d = sim_tables()?;
    if d.theaters.get(index).is_some() {
        return Ok(d);
    }
    let tiles = load_tiles(code)?;
    Ok(d.with_theater(index, || tiles))
}

/// The mission generator's tables.
pub fn game_data() -> Result<GameData> {
    GameData::load_with(|name| crate::assets::read(&format!("mission/{name}")))
}

/// The simulation's tables and the classic theaters' tiles (`assets/sim`).
pub fn sim_tables() -> Result<SimData> {
    SimData::load_with(|name| crate::assets::read(&format!("sim/{name}")))
}

/// Index of a theater code ("ce").
pub fn theater_index(code: &str) -> Option<usize> {
    stealth_formats::ALL_THEATERS.iter().position(|t| t.eq_ignore_ascii_case(code))
}

/// The theater codes, lower case.
pub fn codes() -> Vec<String> {
    stealth_formats::ALL_THEATERS.iter().map(|t| t.to_ascii_lowercase()).collect()
}

/// The classic theaters first, then the remake's own (file prefix, display name).
pub const THEATERS: [(&str, &str); 10] = [
    ("lb", "Libya (1986)"),
    ("pg", "Persian Gulf (1984)"),
    ("nc", "North Cape"),
    ("ce", "Central Europe"),
    ("vn", "Vietnam"),
    ("me", "Middle East"),
    ("ku", "Desert Storm"),
    ("cu", "Cuba"),
    ("ko", "Korea"),
    ("ua", "Ukraine (2026)"),
];

/// A theater's display name ("ce": "Central Europe"; the code itself if unknown).
pub fn theater_name(code: &str) -> String {
    THEATERS
        .iter()
        .find(|t| t.0.eq_ignore_ascii_case(code))
        .map_or_else(|| code.to_ascii_uppercase(), |t| t.1.to_string())
}
