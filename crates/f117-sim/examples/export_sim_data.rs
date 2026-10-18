//! Write only `assets/sim/sim_data.bin` (the same step f117-convert runs).
//! `cargo run -p f117-sim --example export_sim_data [out_dir]`

fn main() -> anyhow::Result<()> {
    let game = f117_formats::GameDir::default_location().ok_or_else(|| anyhow::anyhow!("game dir not found"))?;
    let out = std::env::args().nth(1).unwrap_or_else(|| "assets".into());
    let dir = std::path::Path::new(&out).join("sim");
    std::fs::create_dir_all(&dir)?;
    let bytes = f117_sim::SimData::load(&game)?.to_postcard()?;
    std::fs::write(dir.join("sim_data.bin"), &bytes)?;
    println!("{} bytes -> {}", bytes.len(), dir.join("sim_data.bin").display());
    Ok(())
}
