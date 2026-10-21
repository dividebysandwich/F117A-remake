use std::path::Path;
use std::sync::OnceLock;

use stealth_mission::*;

fn data() -> &'static GameData {
    static D: OnceLock<GameData> = OnceLock::new();
    D.get_or_init(|| {
        GameData::load_json(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/mission"))
            .expect("assets/mission")
    })
}

/// A fixed Central Europe mission: bases, night and departure time stay stable.
#[test]
fn central_europe_reference_briefing() {
    let data = data();
    let s = MissionSettings {
        theater: 3,
        roe: 2,
        kind: MissionKind::Strike,
        opponents: 1,
        ..Default::default()
    };
    let input =
        GenInput { holdrand: 0x5A41C429, weapon_unavailable: [0; 20], stale_special_start: 0 };
    let m = generate_exact(data, &s, &input).unwrap();
    eprintln!("{}", m.briefing.lines.join("\n"));
    eprintln!("orders: {:?}", m.orders);
    assert!(m.takeoff.name.starts_with("Ahlhorn"), "{}", m.takeoff.name);
    assert!(m.landing.name.starts_with("Gutersloh"), "{}", m.landing.name);
    assert!(m.night);
    assert_eq!(m.time.depart, "18:50");
    let json = serde_json::to_string(&m).unwrap();
    let back: Mission = serde_json::from_str(&json).unwrap();
    assert_eq!(handoff_block(&back), handoff_block(&m));
}

/// A chosen start hour sets the clock and day or night, and leaves the rest of the mission.
#[test]
fn chosen_start_hour() {
    let data = data();
    let input =
        GenInput { holdrand: 0x5A41C429, weapon_unavailable: [0; 20], stale_special_start: 0 };
    let base = MissionSettings { theater: 3, roe: 2, ..Default::default() };
    let rolled = generate_exact(data, &base, &input).unwrap();
    for (hour, night) in [(13, false), (23, true), (5, true)] {
        let s = MissionSettings { start_hour: Some(hour), ..base.clone() };
        let m = generate_exact(data, &s, &input).unwrap();
        assert_eq!((m.time.base_hour, m.time.tod_ticks, m.night), (hour, 0, night));
        assert_eq!(m.time.depart, format!("{hour:02}:00"));
        assert!(m.briefing.depart_line.contains(&m.time.depart), "{}", m.briefing.depart_line);
        assert_eq!(m.orders.primary, rolled.orders.primary);
        assert_eq!(m.primary.object_index, rolled.primary.object_index);
    }
}

/// 200 random missions per theater with random settings must generate without panics.
#[test]
fn random_missions_do_not_panic() {
    let data = data();
    // simple LCG for the settings; seeds via srand like a fresh game start
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
                weather: if i % 3 == 0 { None } else { Some(stealth_mission::Sky::Clear) },
                cloud_cover: None,
                start_hour: None,
            };
            let seed = if i % 2 == 0 { Seed::Srand(next() as u16) } else { Seed::Holdrand(next()) };
            let m = generate(data, &s, seed).unwrap_or_else(|e| panic!("{s:?} {seed:?}: {e}"));
            assert_eq!(
                handoff_block(&m).len(),
                handoff_names_offset(&m) + NAMES_BYTES + 256 + 4 + 16 + 36
            );
            assert!(
                (1..=8).contains(&m.primary.mission_type)
                    && (1..=8).contains(&m.secondary.mission_type)
            );
            assert!(!m.orders.primary.is_empty() && !m.briefing.lines.is_empty());
            types[m.primary.mission_type as usize] += 1;
            types[m.secondary.mission_type as usize] += 1;
        }
    }
    eprintln!("mission types seen (index = type): {types:?}");
}

/// The briefing wording is fixed by the seed and varies between seeds.
#[test]
fn briefing_wording_varies_by_seed() {
    let data = data();
    let s = MissionSettings {
        theater: 3,
        roe: 1,
        kind: MissionKind::Strike,
        opponents: 1,
        ..Default::default()
    };
    let brief = |h: u32| generate(data, &s, Seed::Holdrand(h)).unwrap();
    let a = brief(0x1234_5678);
    assert_eq!(a.briefing, brief(0x1234_5678).briefing);
    let mut summaries = std::collections::HashMap::<i16, std::collections::HashSet<String>>::new();
    for h in 0..400u32 {
        let m = brief(h.wrapping_mul(0x0101_0F0F));
        assert!(!m.briefing.primary_summary.is_empty() && !m.briefing.primary_objective.is_empty());
        assert!(
            !m.briefing.lines.iter().any(|l| l.contains('{')),
            "unfilled placeholder: {:?}",
            m.briefing.lines
        );
        summaries.entry(m.primary.row).or_default().insert(m.briefing.primary_summary.clone());
    }
    assert!(summaries.values().any(|v| v.len() > 1), "no row showed more than one summary");
}
