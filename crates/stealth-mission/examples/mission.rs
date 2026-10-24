//! Generate a mission from the JSON data (`assets/mission`) and print the briefing and the orders page.
//!
//! `cargo run -p stealth-mission --example mission -- MISSION_DIR THEATER ROE KIND OPP PLANE SRAND_HEX [SORTIES] [--json] [--helicopter]`

use stealth_mission::*;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        a.len() >= 7,
        "usage: MISSION_DIR THEATER ROE KIND OPP PLANE SRAND_HEX [SORTIES] [--json]"
    );
    let n = |i: usize| a[i].parse::<u8>().unwrap();
    let s = MissionSettings {
        theater: n(1) as usize,
        roe: n(2),
        kind: MissionKind::from_index(n(3)).unwrap(),
        opponents: n(4),
        plane: if n(5) != 0 { Plane::Lockheed } else { Plane::Fictional },
        sorties: a.get(7).and_then(|v| v.parse().ok()).unwrap_or(0),
        helicopter: a.iter().any(|v| v == "--helicopter"),
        ..Default::default()
    };
    let seed = Seed::Srand(u16::from_str_radix(&a[6], 16)?);
    let m = generate(&GameData::load_json(std::path::Path::new(&a[0]))?, &s, seed)?;
    if a.iter().any(|v| v == "--json") {
        println!("{}", serde_json::to_string_pretty(&m)?);
        return Ok(());
    }
    println!("{}", m.briefing.lines.join("\n"));
    println!("---");
    for l in &m.orders.lines {
        println!("{}", l.text);
    }
    println!("--- {}", m.rng.holdrand_exit);
    Ok(())
}
