//! Campaign mode (`SimOptions::campaign`) on the shipped game data.

use std::sync::OnceLock;

use stealth_mission::{GameData, Mission, MissionKind, MissionSettings, Seed};
use stealth_sim::*;

struct Data {
    sim: SimData,
    gdata: GameData,
}

fn data() -> &'static Data {
    static D: OnceLock<Data> = OnceLock::new();
    D.get_or_init(|| {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let sim = SimData::load_json(&assets.join("sim")).expect("assets/sim");
        let gdata = GameData::load_json(&assets.join("mission")).expect("assets/mission");
        Data { sim, gdata }
    })
}

/// A Central Europe campaign simulation on the theater's own world.
fn campaign() -> (Mission, Sim) {
    let d = data();
    let s = MissionSettings {
        theater: 3,
        roe: 2,
        opponents: 1,
        kind: MissionKind::Strike,
        ..Default::default()
    };
    let mut m = stealth_mission::generate(&d.gdata, &s, Seed::Srand(77)).unwrap();
    m.world = d.gdata.theaters[3].world.clone();
    m.weather = Default::default();
    let sim = Sim::with_options(&d.sim, &m, SimOptions { campaign: true, ..Default::default() });
    (m, sim)
}

fn idle() -> PlayerState {
    PlayerState { on_ground: true, fuel: 10000, ..Default::default() }
}

fn find(sim: &Sim, flags: i32, name_like: Option<&str>) -> usize {
    (3..sim.objects().len())
        .find(|&i| {
            sim.objects()[i].flags & 0xF0F == flags
                && name_like.is_none_or(|n| sim.place_name(i).eq_ignore_ascii_case(n))
        })
        .expect("object")
}

/// No radars, no guns.
fn quiet(sim: &mut Sim) {
    for o in sim.objects_mut().iter_mut() {
        o.unit_type = 0;
    }
    sim.aaa_units_mut().clear();
}

fn run(sim: &mut Sim, ps: &PlayerState, secs: i32) -> Vec<SimEvent> {
    let mut ev = Vec::new();
    for _ in 0..secs * 15 {
        ev.extend(sim.step(ps, 15));
    }
    ev
}

#[test]
fn the_absent_player_is_left_alone() {
    let (_, mut sim) = campaign();
    assert!(sim.player_absent());
    // Hovering over an enemy SAM site's position would get him shot at, were he there.
    let o = sim.objects()[3];
    let (x, y) = Sim::map_to_world(o.x, o.y);
    let ps = PlayerState {
        x,
        y,
        altitude: 3000,
        velocity: 400 * 27,
        gear_up: true,
        fuel: 10000,
        ..Default::default()
    };
    let ev = run(&mut sim, &ps, 120);
    assert!(
        !ev.iter().any(|e| matches!(
            e,
            SimEvent::MissileLaunched { .. } | SimEvent::Rwr(_) | SimEvent::PlayerHit { .. }
        )),
        "{ev:?}"
    );
    assert!(sim.mission_end().is_none());
    assert!(sim.emitters().is_empty() || sim.player_absent());
}

#[test]
fn a_crash_ends_the_sortie_not_the_simulation() {
    let (_, mut sim) = campaign();
    let base = find(&sim, 0x401, None);
    let st = sim.respawn_player(base, None);
    assert!(!sim.player_absent());
    run(&mut sim, &st, 2);
    // Fast on the ground, off the runway: a crash.
    let o = sim.objects()[base];
    let (x, y) = Sim::map_to_world(o.x + 0x200, o.y);
    let ps = PlayerState { x, y, velocity: 200 * 27, fuel: 10000, ..Default::default() };
    let airborne = PlayerState { altitude: 500, on_ground: false, ..ps.clone() };
    run(&mut sim, &airborne, 2);
    let ps = PlayerState { on_ground: true, ..ps };
    let ev = run(&mut sim, &ps, 2);
    let end = ev.iter().find_map(|e| match e {
        SimEvent::SortieEnded(e) => Some(*e),
        _ => None,
    });
    assert_eq!(end.map(|e| e.outcome), Some(1), "{ev:?}");
    assert!(sim.player_absent());
    assert!(sim.mission_end().is_none());
    // The world goes on, and the next sortie starts clean.
    run(&mut sim, &idle(), 5);
    let st = sim.respawn_player(base, None);
    assert_eq!(sim.damage_effects().bits, 0);
    assert!(st.on_ground);
}

#[test]
fn a_safe_landing_keeps_the_sortie_going() {
    let (_, mut sim) = campaign();
    let base = find(&sim, 0x401, None);
    let st = sim.respawn_player(base, None);
    let air = PlayerState {
        x: st.x + 0x8000,
        altitude: 800,
        on_ground: false,
        velocity: 250 * 27,
        gear_up: true,
        ..st.clone()
    };
    run(&mut sim, &air, 3);
    let ev = run(&mut sim, &st, 5);
    assert!(
        ev.iter().any(|e| matches!(e, SimEvent::LandedAtBase { object } if *object == base)),
        "{ev:?}"
    );
    assert!(!ev.iter().any(|e| matches!(e, SimEvent::SortieEnded(_))));
    assert!(!sim.player_absent());
}

#[test]
fn an_insertion_helicopter_lands_at_its_goal() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x401, Some("Neuberg"));
    let to = find(&sim, 0x001, Some("Tabor"));
    let i = sim.spawn_insertion(from, to, 42, 0);
    assert_eq!(sim.campaign_flight(i).map(|f| f.tag), Some(42));
    let mut landed = None;
    for _ in 0..40 * 60 {
        for e in run(&mut sim, &idle(), 1) {
            if let SimEvent::CampaignFlightLanded { tag, x, y, .. } = e {
                landed = Some((tag, x, y));
            }
        }
        if landed.is_some() {
            break;
        }
    }
    let (tag, x, y) = landed.expect("landed within 40 minutes");
    let t = sim.objects()[to];
    assert_eq!(tag, 42);
    assert!((x - t.x).abs() < 0x40 && (y - t.y).abs() < 0x40, "{x},{y} vs {},{}", t.x, t.y);
    // Its slot is free for the next one.
    assert!(sim.campaign_flight(i).is_none());
}

#[test]
fn enemy_defences_shoot_down_helicopters() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x401, Some("Neuberg"));
    let to = find(&sim, 0x001, Some("Tabor"));
    // A SAM site on the way.
    let (a, b) = (sim.objects()[from], sim.objects()[to]);
    let site = &mut sim.objects_mut()[3];
    site.unit_type = 7;
    site.x = (a.x + b.x) / 2;
    site.y = (a.y + b.y) / 2;
    site.flags = 0;
    for n in 0..4 {
        sim.spawn_insertion(from, to, 100 + n, n as usize);
    }
    let mut lost = Vec::new();
    for _ in 0..30 * 60 {
        for e in run(&mut sim, &idle(), 1) {
            if let SimEvent::CampaignFlightDestroyed { tag, by_player, .. } = e {
                assert!(!by_player);
                lost.push(tag);
            }
        }
        if !lost.is_empty() {
            break;
        }
    }
    assert!(!lost.is_empty(), "the SA-13 let them all through");
}

#[test]
fn fighters_go_for_the_helicopters() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x401, Some("Neuberg"));
    let to = find(&sim, 0x001, Some("Tabor"));
    let enemy = find(&sim, 0x001, Some("Konmarno"));
    sim.spawn_insertion(from, to, 7, 0);
    // Scrambled (as the campaign does) when the helicopter is 80 km out.
    run(&mut sim, &idle(), 20 * 60);
    let t = sim.objects()[to];
    sim.spawn_fighter(enemy, t.x, t.y, 8, false);
    let mut lost = false;
    for _ in 0..15 * 60 {
        if run(&mut sim, &idle(), 1)
            .iter()
            .any(|e| matches!(e, SimEvent::CampaignFlightDestroyed { tag: 7, .. }))
        {
            lost = true;
            break;
        }
    }
    assert!(lost, "the fighter never got the helicopter");
}

#[test]
fn a_far_sam_site_leaves_helicopters_alone() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x401, Some("Neuberg"));
    let to = find(&sim, 0x001, Some("Tabor"));
    // An SA-11 20 km off the route: well in its reach, but too far to see a helicopter.
    let (a, b) = (sim.objects()[from], sim.objects()[to]);
    let site = &mut sim.objects_mut()[3];
    site.unit_type = 5;
    site.x = (a.x + b.x) / 2;
    site.y = (a.y + b.y) / 2 + 20 * 64;
    site.flags = 0;
    for n in 0..4 {
        sim.spawn_insertion(from, to, 100 + n, n as usize);
    }
    let mut landed = 0;
    for _ in 0..45 * 60 {
        for e in run(&mut sim, &idle(), 1) {
            match e {
                SimEvent::CampaignFlightDestroyed { tag, .. } => panic!("the SA-11 got {tag}"),
                SimEvent::CampaignFlightLanded { .. } => landed += 1,
                _ => {}
            }
        }
        if landed == 4 {
            break;
        }
    }
    assert_eq!(landed, 4);
}

#[test]
fn places_change_hands_and_get_repaired() {
    let (_, mut sim) = campaign();
    let tabor = find(&sim, 0x001, Some("Tabor"));
    let shape = sim.object_shapes()[tabor];
    sim.remove_object(tabor);
    assert!(sim.objects()[tabor].flags & oflags::DESTROYED != 0);
    assert_eq!(sim.world_damage().tiles.len(), 1);
    sim.restore_object(tabor);
    assert!(sim.world_damage().tiles.is_empty());
    sim.set_side(tabor, true, false);
    let o = sim.objects()[tabor];
    assert!(o.flags & oflags::DESTROYED == 0 && o.flags & oflags::FRIENDLY != 0);
    assert_eq!(sim.object_shapes()[tabor], shape);
    // Friendly guns there now, the enemy's gone.
    sim.place_defences(o.x, o.y, true, false);
    let n = sim.aaa_units().len();
    sim.place_defences(o.x, o.y, true, true);
    assert!((0..n).all(|k| sim.aaa_units()[k].destroyed || {
        let u = sim.aaa_units()[k];
        (u.x - o.x).abs() + (u.y - o.y).abs() > 0x200
    }));
    assert!((n..sim.aaa_units().len()).all(|k| sim.aaa_blue(k)));
    // A sortie can start there.
    let st = sim.respawn_player(tabor, None);
    assert!(st.on_ground);
}

#[test]
fn intelligence_shortens_detection() {
    let (_, mut sim) = campaign();
    let i = 3;
    let o = sim.objects()[i];
    let leth = 125; // SA-13 class range (km) is plenty for the test.
    sim.objects_mut()[i].unit_type = 7;
    let base = find(&sim, 0x401, None);
    sim.respawn_player(base, None);
    // 90% of the detection range out: seen without intel, not with it.
    let d = leth * 64 * 9 / 10;
    let (x, y) = Sim::map_to_world(o.x, o.y + d);
    let ps = PlayerState {
        x,
        y,
        altitude: 5000,
        velocity: 400 * 27,
        gear_up: true,
        fuel: 10000,
        ..Default::default()
    };
    run(&mut sim, &ps, 1);
    let seen = |sim: &Sim| sim.emitters().iter().any(|e| e.source == EmitterSource::Site(i));
    assert!(seen(&sim));
    sim.set_intel(i, true);
    run(&mut sim, &ps, 1);
    assert!(!seen(&sim));
}

#[test]
fn a_long_campaign_stays_bounded() {
    let (_, mut sim) = campaign();
    let from = find(&sim, 0x401, Some("Neuberg"));
    let to = find(&sim, 0x001, Some("Tabor"));
    let enemy = find(&sim, 0x001, Some("Konmarno"));
    let t = sim.objects()[to];
    let mut tag = 1;
    for minute in 0..30 {
        if minute % 3 == 0 {
            sim.spawn_insertion(from, to, tag, 0);
            sim.spawn_fighter(enemy, t.x, t.y, tag + 1, false);
            tag += 2;
        }
        run(&mut sim, &idle(), 60);
    }
    assert!(sim.flights().len() < 40, "{} flight units", sim.flights().len());
    assert!(sim.ai_missiles().len() < 50);
    assert!(sim.event_log().len() < 500);
}
