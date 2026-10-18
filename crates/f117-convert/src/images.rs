//! Palettes -> RON, PIC -> PNG (RGBA with the matching palette, plus an 8-bit index PNG so the
//! game can re-colour images with other palettes).

use std::path::Path;

use anyhow::{Context, Result};
use f117_formats::{GameDir, pal::Palette, pic};

pub fn convert(game: &GameDir, out: &Path) -> Result<()> {
    let pal_dir = out.join("palettes");
    let img_dir = out.join("images");
    std::fs::create_dir_all(&pal_dir)?;
    std::fs::create_dir_all(&img_dir)?;

    let flight = game.flight_palette()?;
    write_ron(&pal_dir.join("vgame_base.ron"), &flight)?;
    for name in game.list("PAL")? {
        let data = game.read(&name)?;
        let stem = stem(&name).to_ascii_lowercase();
        for (i, p) in Palette::many_from_vga(&data)?.iter().enumerate() {
            let file = if data.len() > 768 { format!("{stem}_{i}.ron") } else { format!("{stem}.ron") };
            write_ron(&pal_dir.join(file), p)?;
        }
    }

    let mut n = 0;
    for name in game.list("PIC")? {
        let stem = stem(&name);
        let picture = pic::decode(&game.read(&name)?).with_context(|| name.clone())?;
        // Pictures with a same-named .PAL use it; everything else (cockpit, maps) uses the flight palette.
        let palette = match game.read(&format!("{stem}.PAL")) {
            Ok(d) => Palette::from_vga(&d)?,
            Err(_) => flight.clone(),
        };
        let rgba: Vec<u8> =
            picture.pixels.iter().flat_map(|&i| { let [r, g, b] = palette.rgb8(i); [r, g, b, 255] }).collect();
        let lower = stem.to_ascii_lowercase();
        write_png(&img_dir.join(format!("{lower}.png")), &rgba, png::ColorType::Rgba)?;
        write_png(&img_dir.join(format!("{lower}.idx.png")), &picture.pixels, png::ColorType::Grayscale)?;
        std::fs::write(img_dir.join(format!("{lower}.idx")), &picture.pixels)?;
        n += 1;
    }
    println!("images: {n} pictures, palettes written to {}", pal_dir.display());
    Ok(())
}

fn stem(name: &str) -> String {
    name.rsplit_once('.').map_or(name, |(s, _)| s).to_string()
}

pub fn write_ron<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let s = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default().compact_arrays(true))?;
    std::fs::write(path, s).with_context(|| format!("writing {}", path.display()))
}

fn write_png(path: &Path, data: &[u8], color: png::ColorType) -> Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(f, pic::WIDTH as u32, pic::HEIGHT as u32);
    enc.set_color(color);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(data)?;
    Ok(())
}
