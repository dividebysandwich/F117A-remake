//! The remake's own theaters (shipped JSON only): every situation generates missions with
//! targets in enemy territory and friendly takeoff bases.

use std::path::Path;

use stealth_mission::*;

fn data() -> GameData {
    GameData::load_json(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/mission"))
        .expect("assets/mission")
}

#[test]
fn ukraine_generates_missions() {
    let data = data();
    let theater = stealth_formats::ALL_THEATERS.iter().position(|t| *t == "UA").unwrap();
    let th = &data.theaters[theater];
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    for kind in MissionKind::ALL {
        for roe in 0..3 {
            for seed in 0..40u16 {
                let s = MissionSettings {
                    theater,
                    roe,
                    kind,
                    opponents: (seed % 4) as u8,
                    ..Default::default()
                };
                let m = generate(&data, &s, Seed::Srand(seed * 977 + 13))
                    .unwrap_or_else(|e| panic!("{kind:?} roe {roe} seed {seed}: {e:#}"));
                assert_eq!(m.theater_name, "Ukraine");
                assert!(!m.takeoff.carrier, "no carriers in Ukraine");
                let base = &th.world.objects[m.takeoff.object_index as usize];
                assert!(
                    base.flags & world::flags::FRIENDLY != 0,
                    "takeoff from {} is not friendly",
                    m.takeoff.name
                );
                assert!(m.briefing.date.ends_with("2026"), "{}", m.briefing.date);
                if kind.is_strike() {
                    for t in [&m.primary, &m.secondary] {
                        let (col, row) = (t.position.x >> 11, t.position.y >> 11);
                        let g = th.world.grid[(row * 16 + col) as usize];
                        assert_eq!(
                            g & 3,
                            0,
                            "{} at {:?} is not in enemy territory",
                            t.shape_name,
                            t.position
                        );
                    }
                }
                *kinds.entry(m.primary.shape_name.clone()).or_default() += 1;
            }
        }
    }
    eprintln!("primary targets: {kinds:?}");
    assert!(kinds.len() >= 8, "too few kinds of targets: {kinds:?}");
}

#[test]
fn ukraine_drone_defense() {
    let data = data();
    let theater = stealth_formats::ALL_THEATERS.iter().position(|t| *t == "UA").unwrap();
    let mut nuclear = 0;
    let mut sizes = std::collections::BTreeSet::new();
    for opponents in 0..4u8 {
        for seed in 0..30u16 {
            let s = MissionSettings {
                theater,
                roe: (seed % 3) as u8,
                kind: MissionKind::DroneDefense,
                opponents,
                ..Default::default()
            };
            let m = generate(&data, &s, Seed::Srand(seed * 131 + 7))
                .unwrap_or_else(|e| panic!("seed {seed}: {e:#}"));
            let w = m.drones.as_ref().expect("a drone wave");
            assert!(w.defend && (4..=12).contains(&w.count) && w.allowance as usize <= w.count / 2);
            assert_eq!(w.first_unit + w.count, m.world.flights.len());
            assert_eq!(m.primary.mission_type, drones::DRONE_INTERCEPT);
            assert_eq!(m.primary.object_index, w.target);
            let target = m.world.objects[w.target as usize];
            assert!(
                target.flags & world::flags::FRIENDLY != 0,
                "{} is not friendly",
                w.target_name
            );
            // The secondary is a launch site, the base a friendly airfield.
            assert_eq!(m.secondary.mission_type, 2);
            assert!(data.theaters[theater].launch_sites.contains(&m.secondary.object_index));
            assert!(
                m.world.objects[m.takeoff.object_index as usize].flags & world::flags::FRIENDLY
                    != 0
            );
            assert!(m.special_aircraft.is_none());
            for f in &m.world.flights[w.first_unit..] {
                assert!(f.plane_type == drones::GERAN2 || f.plane_type == drones::GERAN3);
                assert_eq!(f.waypoint, w.target);
                assert_eq!(f.flags, 3);
            }
            assert!(m.orders.primary.starts_with("STOP GERAN DRONES"), "{}", m.orders.primary);
            nuclear += w.nuclear as usize;
            sizes.insert(w.count);
        }
    }
    assert!(nuclear > 0, "never a nuclear plant");
    assert!(sizes.len() >= 4, "wave sizes {sizes:?}");
    // The classic theaters have no launch sites.
    let s = MissionSettings { theater: 3, kind: MissionKind::DroneDefense, ..Default::default() };
    assert!(generate(&data, &s, Seed::Srand(1)).is_err());
    let s = MissionSettings { theater: 3, kind: MissionKind::Strike, ..Default::default() };
    assert!(generate(&data, &s, Seed::Srand(1)).unwrap().drones.is_none());
    // Ukraine strike missions: a few drones crossing.
    let s = MissionSettings { theater, kind: MissionKind::Strike, ..Default::default() };
    let w = generate(&data, &s, Seed::Srand(5)).unwrap().drones.unwrap();
    assert!(!w.defend && (1..=3).contains(&w.count));
}
