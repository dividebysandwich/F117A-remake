//! MGRAPHIC.EXE bitmap fonts -> `assets/fonts/fonts.ron` (glyph bitmaps + metrics).

use std::path::Path;

use anyhow::Result;
use f117_formats::{GameDir, font};

pub fn convert(game: &GameDir, out: &Path) -> Result<()> {
    let exe = game.read("MGRAPHIC.EXE")?;
    let fonts = font::parse_fonts(font::mz_image(&exe)?)?;
    let dir = out.join("fonts");
    std::fs::create_dir_all(&dir)?;
    crate::images::write_ron(&dir.join("fonts.ron"), &fonts)?;
    println!("fonts: {} fonts written to {}", fonts.len(), dir.display());
    Ok(())
}
