use std::path::{Path, PathBuf};

use f117_formats::GameDir;
use f117_mission::*;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn game() -> Option<GameDir> {
    let g = root().join("re-work/game-mis");
    if g.join("START.EXE").exists() {
        return Some(GameDir::new(g));
    }
    GameDir::default_location().filter(|g| g.path("START.EXE").is_ok())
}

fn data() -> Option<GameData> {
    let Some(g) = game() else {
        eprintln!("original game not found, skipping");
        return None;
    };
    Some(GameData::load(&g).expect("loading game data"))
}

fn rd16(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([b[o], b[o + 1]])
}

fn check_run(data: &GameData, dir: &Path) -> Result<(), String> {
    let read = |n: &str| std::fs::read(dir.join(n)).map_err(|e| format!("{n}: {e}"));
    let (pre, prec, ds, comm) = (read("pre_ds.bin")?, read("pre_comm.bin")?, read("ds.bin")?, read("comm.bin")?);
    let gw = |o: usize| rd16(&prec, 0x1272 + o);
    let settings = MissionSettings {
        theater: gw(0x38) as usize,
        roe: gw(0x3A) as u8,
        kind: MissionKind::from_index(gw(0x3C) as u8).ok_or("bad kind")?,
        opponents: gw(0x3E) as u8,
        plane: if gw(0x42) != 0 { Plane::Lockheed } else { Plane::MicroProse },
        ..Default::default()
    };
    let input = GenInput {
        holdrand: u32::from_le_bytes(pre[0xAE8C..0xAE90].try_into().unwrap()),
        weapon_unavailable: pre[0xD878..0xD878 + 20].try_into().unwrap(),
        stale_cbdc: rd16(&pre, 0xCBDC),
    };
    let m = generate_exact(data, &settings, &input).map_err(|e| e.to_string())?;
    let blk = handoff_block(&m);

    // the same block assembled from the DS dump (DS addresses from spec §3 / §7)
    let (n, nf) = (m.world.objects.len(), m.world.flights.len());
    let mut r: Vec<u8> = Vec::new();
    for (o, l) in [(0xD88C, 2), (0xD33C, 2), (0xCACE, 2), (0xE084, 2), (0xCBDE, 16 * n), (0xD890, 2), (0xD3BE, 36 * nf)] {
        r.extend_from_slice(&ds[o..o + l]);
    }
    for (o, l) in [(0xD7F6, 128), (0xD33E, 128), (0xDD70, 750), (0xCAD8, 256), (0xD876, 2), (0xCABC, 2), (0x15B2, 16), (0xE05E, 36)] {
        r.extend_from_slice(&ds[o..o + l]);
    }
    let mut errs = Vec::new();
    if r.len() != blk.len() {
        errs.push(format!("block length {} != {}", blk.len(), r.len()));
    }
    let names = handoff_names_offset(&m);
    let stale = names + m.world.names_raw.len()..names + NAMES_BYTES;
    let diffs: Vec<usize> = (0..blk.len().min(r.len())).filter(|i| !stale.contains(i) && blk[*i] != r[*i]).collect();
    if !diffs.is_empty() {
        errs.push(format!("{} block bytes differ, first at {:#x}", diffs.len(), diffs[0]));
    }
    let rs = u32::from_le_bytes(ds[0xAE8C..0xAE90].try_into().unwrap());
    if rs != m.rng.holdrand_exit {
        errs.push(format!("rand state {:08X} != {rs:08X}", m.rng.holdrand_exit));
    }
    let cw = |o: usize| rd16(&comm, o);
    let wt: Vec<i16> = m.loadout.bays.iter().map(|b| b.weapon).collect();
    let qty: Vec<i16> = m.loadout.bays.iter().map(|b| b.count as i16).collect();
    if (0..4).map(|k| cw(0x38 + 2 * k)).collect::<Vec<_>>() != wt {
        errs.push(format!("weapons {wt:?}"));
    }
    if (0..4).map(|k| cw(0x40 + 2 * k)).collect::<Vec<_>>() != qty {
        errs.push(format!("qty {qty:?}"));
    }
    if cw(0x2E) != m.fuel_lbs {
        errs.push("fuel".into());
    }
    if comm[0x33] != m.night as u8 {
        errs.push("night".into());
    }
    for (off, val) in [(0xD7C, &m.time.deadline), (0xD82, &m.time.deadline_plus_15), (0xD88, &m.time.depart)] {
        if &ds[off..off + 5] != val.as_bytes() {
            errs.push(format!("time {off:#x}: {val} != {}", String::from_utf8_lossy(&ds[off..off + 5])));
        }
    }
    eprintln!(
        "{}: {} P:{} [{}] S:{} [{}] {} -> {} {}",
        dir.file_name().unwrap().to_string_lossy(),
        m.theater_code,
        m.primary.mission_type,
        m.primary.objective,
        m.secondary.mission_type,
        m.secondary.objective,
        m.takeoff.name,
        m.landing.name,
        if errs.is_empty() { "OK".to_string() } else { format!("MISMATCH {errs:?}") }
    );
    if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
}

#[test]
fn emulator_dumps_match_byte_exact() {
    let runs = root().join("re-work/mis/runs");
    let Ok(rd) = std::fs::read_dir(&runs) else {
        eprintln!("{} not found, skipping", runs.display());
        return;
    };
    let Some(data) = data() else { return };
    let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("ds.bin").exists()).collect();
    dirs.sort();
    assert!(!dirs.is_empty());
    let failed: Vec<String> =
        dirs.iter().filter_map(|d| check_run(&data, d).err().map(|e| format!("{}: {e}", d.display()))).collect();
    eprintln!("{}/{} runs match", dirs.len() - failed.len(), dirs.len());
    assert!(failed.is_empty(), "mismatches:\n{}", failed.join("\n"));
}

/// The natural Central Europe mission checked against the game's own briefing (spec §9).
#[test]
fn central_europe_reference_briefing() {
    let Some(data) = data() else { return };
    let s = MissionSettings { theater: 3, roe: 2, kind: MissionKind::Strike, opponents: 1, ..Default::default() };
    let input = GenInput { holdrand: 0x5A41C429, weapon_unavailable: [0; 20], stale_cbdc: 0 };
    let m = generate_exact(&data, &s, &input).unwrap();
    eprintln!("{}", m.briefing.lines.join("\n"));
    eprintln!("orders: {:?}", m.orders);
    // The emulator run with the same seed may have had other weapons out of stock; the
    // targets, bases and times do not depend on that.
    assert!(m.takeoff.name.starts_with("Ahlhorn"), "{}", m.takeoff.name);
    assert!(m.landing.name.starts_with("Gutersloh"), "{}", m.landing.name);
    assert!(m.night);
    assert_eq!(m.time.depart, "18:50");
    let json = serde_json::to_string(&m).unwrap();
    let back: Mission = serde_json::from_str(&json).unwrap();
    assert_eq!(handoff_block(&back), handoff_block(&m));
}

/// 200 random missions per theater with random settings must generate without panics.
#[test]
fn random_missions_do_not_panic() {
    let Some(data) = data() else { return };
    // simple LCG for the settings; seeds via srand like a fresh START run
    let mut st: u64 = 0x1234_5678_9ABC_DEF0;
    let mut next = || {
        st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (st >> 33) as u32
    };
    let mut types = [0usize; 9];
    for theater in 0..9 {
        for i in 0..200 {
            let s = MissionSettings {
                theater,
                roe: (next() % 3) as u8,
                kind: MissionKind::from_index((next() % 4) as u8).unwrap(),
                opponents: (next() % 4) as u8,
                plane: if next() % 2 == 0 { Plane::MicroProse } else { Plane::Lockheed },
                sorties: (next() % 100) as u16,
                pilot: "Capt. Test".into(),
                weather: if i % 3 == 0 { None } else { Some(f117_mission::Sky::Clear) },
                cloud_cover: None,
            };
            let seed = if i % 2 == 0 { Seed::Srand(next() as u16) } else { Seed::Holdrand(next()) };
            let m = generate(&data, &s, seed).unwrap_or_else(|e| panic!("{s:?} {seed:?}: {e}"));
            assert_eq!(handoff_block(&m).len(), handoff_names_offset(&m) + NAMES_BYTES + 256 + 4 + 16 + 36);
            assert!((1..=8).contains(&m.primary.mission_type) && (1..=8).contains(&m.secondary.mission_type));
            assert!(!m.orders.primary.is_empty() && !m.briefing.lines.is_empty());
            types[m.primary.mission_type as usize] += 1;
            types[m.secondary.mission_type as usize] += 1;
        }
    }
    eprintln!("mission types seen (index = type): {types:?}");
}
