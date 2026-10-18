//! Converts the original F-117A data files into assets for the Bevy game.
//!
//! `f117-convert --game ~/Games/DOS/F117 --out assets --src assets-src`

mod fonts;
mod glb;
mod images;
mod models;
mod sim;
mod sim_data;
mod theaters;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use f117_formats::GameDir;

#[derive(Parser)]
#[command(about = "Convert original F-117A data files into game assets")]
struct Args {
    /// Original game directory (defaults to $F117_GAME_DIR or ~/Games/DOS/F117).
    #[arg(long)]
    game: Option<PathBuf>,
    /// Output directory for runtime assets.
    #[arg(long, default_value = "assets")]
    out: PathBuf,
    /// Output directory for editable sources (raw .glb for the Blender import step).
    #[arg(long, default_value = "assets-src")]
    src: PathBuf,
    /// Only run the font export step.
    #[arg(long)]
    fonts_only: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let game = match args.game {
        Some(p) => GameDir::new(p),
        None => GameDir::default_location()
            .ok_or_else(|| anyhow::anyhow!("game dir not found; pass --game"))?,
    };
    std::fs::create_dir_all(&args.out)?;
    fonts::convert(&game, &args.out)?;
    if args.fonts_only {
        return Ok(());
    }
    std::fs::create_dir_all(&args.src)?;
    images::convert(&game, &args.out)?;

    // Model colour bytes go through VGAME's remap tables; the day table gives the preview
    // colours stored in the .glb materials.
    let vgame = game.vgame()?;
    let remaps = f117_formats::pal::remap_tables(&vgame.image);
    images::write_ron(&args.out.join("palettes/remap.ron"), &remaps)?;
    // Blender preview colours: Central Europe day palette.
    let base = f117_formats::pal::theater_palettes(&vgame.image, 3)?.day;
    for (i, t) in f117_formats::THEATERS.iter().enumerate() {
        let p = f117_formats::pal::theater_palettes(&vgame.image, i)?;
        images::write_ron(&args.out.join(format!("palettes/tod_{}.ron", t.to_ascii_lowercase())), &p)?;
    }
    let day = &remaps[0].entries;
    let model_palette = f117_formats::pal::Palette { colors: (0..256).map(|c| base.colors[(day[c] & 0xFF) as usize]).collect() };
    models::convert(&game, &model_palette, &args.src)?;
    theaters::convert(&game, &args.out)?;
    sim::convert(&vgame.image, &args.out)?;
    sim_data::convert(&game, &args.out)?;

    // Mission generator inputs (START.EXE tables + theater worlds) for the in-game generator.
    let data = f117_mission::GameData::load(&game)?;
    std::fs::create_dir_all(args.out.join("mission"))?;
    // Binary (postcard): the candidate tables are large.
    std::fs::write(args.out.join("mission/game_data.bin"), postcard::to_stdvec(&data)?)?;
    println!("mission: generator data written");
    Ok(())
}
