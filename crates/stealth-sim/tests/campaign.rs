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
    sim.spawn_fighter(enemy, t.x, t.y, 8, false, stealth_sim::ai_loadout::FighterTask::Patrol);
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
fn fighters_use_their_guns() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x401, Some("Neuberg"));
    let to = find(&sim, 0x001, Some("Tabor"));
    let enemy = find(&sim, 0x001, Some("Konmarno"));
    sim.spawn_insertion(from, to, 7, 0);
    run(&mut sim, &idle(), 20 * 60);
    let t = sim.objects()[to];
    let f =
        sim.spawn_fighter(enemy, t.x, t.y, 8, false, stealth_sim::ai_loadout::FighterTask::Patrol);
    // No missiles: only its gun.
    sim.set_fighter_loadout(f, Default::default());
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
    assert!(lost, "the fighter never gunned down the helicopter");
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
            sim.spawn_fighter(
                enemy,
                t.x,
                t.y,
                tag + 1,
                false,
                stealth_sim::ai_loadout::FighterTask::Patrol,
            );
            tag += 2;
        }
        run(&mut sim, &idle(), 60);
    }
    assert!(sim.flights().len() < 40, "{} flight units", sim.flights().len());
    assert!(sim.ai_missiles().len() < 50);
    assert!(sim.event_log().len() < 500);
}

use stealth_sim::aaa::{AaaKind, PATRIOT, STINGER};
use stealth_sim::ai_loadout::{AIM9, AIM120, FighterTask};
use stealth_sim::campaign::{CAP_BUBBLE, CAP_LEASH};

/// Runs until `tag` is shot down (true) or `secs` are up; every AI missile seen goes to `seen`.
fn until_down(sim: &mut Sim, tag: u32, secs: i32, seen: &mut Vec<i32>) -> bool {
    for _ in 0..secs {
        let ev = run(sim, &idle(), 1);
        seen.extend(sim.ai_missiles().iter().map(|m| m.spec));
        if ev
            .iter()
            .any(|e| matches!(e, SimEvent::CampaignFlightDestroyed { tag: t, .. } if *t == tag))
        {
            return true;
        }
    }
    false
}

/// Runs until flight `f` is this close to (x, y).
fn until_near(sim: &mut Sim, f: usize, (x, y): (i32, i32), d: i32) {
    for _ in 0..40 * 60 {
        let u = sim.flights()[f];
        if (u.x - x).abs().max((u.y - y).abs()) < d {
            return;
        }
        run(sim, &idle(), 1);
    }
    panic!("never got near");
}

/// An enemy patrol out of Konmarno over Neuberg (tag 8), and Neuberg.
fn raid(sim: &mut Sim) -> (usize, usize) {
    let home = find(sim, 0x401, Some("Neuberg"));
    let enemy = find(sim, 0x001, Some("Konmarno"));
    let t = sim.objects()[home];
    let f = sim.spawn_fighter(enemy, t.x, t.y, 8, false, FighterTask::Patrol);
    // Unarmed: what is tested is what shoots it down.
    sim.set_fighter_loadout(f, Default::default());
    (home, f)
}

#[test]
fn a_patriot_battery_shoots_down_raiders() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let (home, _) = raid(&mut sim);
    let o = sim.objects()[home];
    sim.place_patriot(o.x + 0x100, o.y);
    let kinds: Vec<AaaKind> = sim.aaa_units().iter().map(|u| u.kind).collect();
    assert_eq!(kinds.iter().filter(|&&k| k == AaaKind::PatriotLauncher).count(), 3);
    assert_eq!(kinds.iter().filter(|&&k| k == AaaKind::PatriotRadar).count(), 1);
    assert!((0..kinds.len()).all(|k| sim.aaa_blue(k)));
    let mut seen = Vec::new();
    assert!(until_down(&mut sim, 8, 40 * 60, &mut seen), "the Patriots never got it");
    assert!(seen.contains(&PATRIOT));
}

#[test]
fn patriot_launchers_need_their_radar() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let (home, _) = raid(&mut sim);
    let o = sim.objects()[home];
    sim.place_patriot(o.x + 0x100, o.y);
    for u in sim.aaa_units_mut().iter_mut() {
        if u.kind == AaaKind::PatriotRadar {
            u.destroyed = true;
        }
    }
    let mut seen = Vec::new();
    assert!(!until_down(&mut sim, 8, 30 * 60, &mut seen));
    assert!(seen.is_empty(), "{seen:?}");
}

#[test]
fn friendly_guns_and_stingers_defend_an_airfield() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let (home, f) = raid(&mut sim);
    let o = sim.objects()[home];
    sim.place_defences(o.x, o.y, true, true);
    let kinds: Vec<AaaKind> = sim.aaa_units().iter().map(|u| u.kind).collect();
    assert!(kinds.contains(&AaaKind::Gepard) && kinds.contains(&AaaKind::Manpads));
    assert!(!kinds.contains(&AaaKind::Zsu));
    // Low over the field, in the guns' reach (its patrol would circle it out of their reach).
    run(&mut sim, &idle(), 60);
    let mut seen = Vec::new();
    let mut down = false;
    for _ in 0..10 * 60 {
        let u = &mut sim.flights_mut()[f];
        if u.flags & fflags::DESTROYED == 0 {
            (u.x, u.y, u.alt) = (o.x + 0x30, o.y, 1500);
            (u.wx, u.wy) = (u.x << 5, u.y << 5);
        }
        let ev = run(&mut sim, &idle(), 1);
        seen.extend(sim.ai_missiles().iter().map(|m| m.spec));
        if ev.iter().any(|e| matches!(e, SimEvent::CampaignFlightDestroyed { tag: 8, .. })) {
            down = true;
            break;
        }
    }
    assert!(down, "the airfield's guns never got it");
    assert!(seen.iter().all(|&s| s == STINGER), "{seen:?}");
}

#[test]
fn a_cap_defends_its_airfield_with_missiles() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let (home, raider) = raid(&mut sim);
    let o = sim.objects()[home];
    // Scrambled as the campaign does, with the raider 30 km out.
    until_near(&mut sim, raider, (o.x, o.y), 0x780);
    let cap = sim.spawn_fighter(home, o.x, o.y, 9, true, FighterTask::Cap);
    let f = sim.campaign_flight(cap).unwrap();
    assert_eq!(f.task, FighterTask::Cap);
    assert_eq!(f.anchor, (o.x, o.y));
    assert!(f.loadout.stores.iter().all(|s| s.count == 0 || [AIM120, AIM9].contains(&s.missile)));
    let mut seen = Vec::new();
    assert!(until_down(&mut sim, 8, 15 * 60, &mut seen), "the CAP never got the raider");
    assert!(seen.iter().any(|s| [AIM120, AIM9].contains(s)), "{seen:?}");
}

#[test]
fn a_cap_stays_home_and_lands() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let home = find(&sim, 0x401, Some("Neuberg"));
    let enemy = find(&sim, 0x001, Some("Konmarno"));
    let (o, e) = (sim.objects()[home], sim.objects()[enemy]);
    assert!((e.x - o.x).abs().max((e.y - o.y).abs()) > CAP_LEASH, "Konmarno too close");
    // An enemy patrol over its own base, out of the bubble.
    sim.spawn_fighter(enemy, e.x, e.y, 8, false, FighterTask::Patrol);
    let cap = sim.spawn_fighter(home, o.x, o.y, 9, true, FighterTask::Cap);
    let mut landed = false;
    for _ in 0..30 * 60 {
        for ev in run(&mut sim, &idle(), 1) {
            match ev {
                SimEvent::CampaignFlightDestroyed { tag, .. } => panic!("{tag} shot down"),
                SimEvent::CampaignFlightLanded { tag: 9, .. } => landed = true,
                _ => {}
            }
        }
        if landed {
            break;
        }
        if let Some(f) = sim.campaign_flight(cap) {
            assert!(f.target.is_none(), "went for an aircraft out of its bubble");
            let u = sim.flights()[cap];
            assert!(
                (u.x - o.x).abs().max((u.y - o.y).abs()) < CAP_BUBBLE,
                "strayed {},{} from home",
                u.x - o.x,
                u.y - o.y
            );
        }
    }
    assert!(landed, "the CAP never landed");
}

#[test]
fn a_cap_does_not_follow_into_enemy_territory() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let (home, raider) = raid(&mut sim);
    let o = sim.objects()[home];
    until_near(&mut sim, raider, (o.x, o.y), 0x780);
    let cap = sim.spawn_fighter(home, o.x, o.y, 9, true, FighterTask::Cap);
    sim.set_fighter_loadout(cap, Default::default());
    // Wait until the CAP has the raider.
    let mut engaged = false;
    for _ in 0..30 * 60 {
        run(&mut sim, &idle(), 1);
        if sim.campaign_flight(cap).and_then(|f| f.target) == Some(raider) {
            engaged = true;
            break;
        }
    }
    assert!(engaged, "the CAP never went for the raider");
    // Dragged out past the leash: it lets go.
    let u = &mut sim.flights_mut()[cap];
    u.x = o.x + CAP_LEASH + 0x100;
    u.wx = u.x << 5;
    run(&mut sim, &idle(), 2);
    assert_eq!(sim.campaign_flight(cap).and_then(|f| f.target), None);
}

#[test]
fn a_friendly_fighter_leaves_the_player_alone() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let home = find(&sim, 0x401, Some("Neuberg"));
    let o = sim.objects()[home];
    let cap = sim.spawn_fighter(home, o.x, o.y, 9, true, FighterTask::Cap);
    let st = sim.respawn_player(home, None);
    let (x, y) = Sim::map_to_world(o.x + 0x100, o.y);
    let ps = PlayerState {
        x,
        y,
        altitude: 3000,
        velocity: 400 * 27,
        gear_up: true,
        on_ground: false,
        fuel: 10000,
        ..st
    };
    for _ in 0..120 {
        sim.raise_alarm();
        run(&mut sim, &ps, 1);
        assert_ne!(sim.flights()[cap].mode, 1, "the CAP went for the player");
    }
}

#[test]
fn an_enemy_strike_bombs_the_airfield_and_flies_home() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x001, Some("Tabor"));
    let to = find(&sim, 0x401, Some("Neuberg"));
    let i = sim.spawn_striker(from, to, 55, false);
    assert!(!sim.campaign_flight(i).unwrap().blue);
    let (mut bombed, mut landed) = (false, false);
    for _ in 0..60 * 60 {
        for e in run(&mut sim, &idle(), 1) {
            match e {
                SimEvent::GroundDestroyed { object, by: None, .. } if object == to => bombed = true,
                SimEvent::CampaignFlightLanded { tag: 55, .. } => landed = true,
                _ => {}
            }
        }
        if landed {
            break;
        }
    }
    assert!(bombed, "the airfield was not bombed");
    assert!(sim.objects()[to].flags & oflags::DESTROYED != 0);
    assert!(landed, "the strike aircraft did not get home");
}

#[test]
fn an_enemy_helicopter_lands_at_a_friendly_place() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x001, Some("Tabor"));
    let to = find(&sim, 0x401, Some("Neuberg"));
    let i = sim.spawn_insertion_for(from, to, 9, 0, false);
    let f = sim.campaign_flight(i).unwrap();
    assert!(!f.blue && !sim.friendly_unit(i, sim.flights()[i].spec));
    let mut landed = false;
    for _ in 0..40 * 60 {
        landed |= run(&mut sim, &idle(), 1)
            .iter()
            .any(|e| matches!(e, SimEvent::CampaignFlightLanded { tag: 9, .. }));
        if landed {
            break;
        }
    }
    assert!(landed, "the Hind did not land within 40 minutes");
}

#[test]
fn lost_guns_are_wrecks_until_replaced() {
    let (_, mut sim) = campaign();
    sim.aaa_units_mut().clear();
    let o = sim.objects()[find(&sim, 0x001, Some("Tabor"))];
    sim.place_defences(o.x, o.y, true, false);
    let alive = |sim: &Sim| sim.aaa_units().iter().filter(|u| !u.destroyed).count();
    let n = alive(&sim);
    assert!(n >= 2);
    sim.wreck_defences(o.x, o.y, false, 2);
    assert_eq!(alive(&sim), n - 2);
    sim.restore_defence(o.x, o.y, false);
    assert_eq!(alive(&sim), n - 1);
}

#[test]
fn an_attack_drone_dives_into_its_target() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x001, Some("Tabor"));
    let to = find(&sim, 0x401, Some("Neuberg"));
    sim.spawn_drone(from, to, 77, false);
    let t = sim.objects()[to];
    let (mut hit, mut gone) = (false, false);
    for _ in 0..4 * 60 * 60 {
        for e in run(&mut sim, &idle(), 1) {
            match e {
                SimEvent::GroundDestroyed { object, by: None, .. } if object == to => hit = true,
                // It blows up on the target, on the ground (not short of it in the air).
                SimEvent::Explosion { x, y, alt, kind: -6 } => {
                    assert_eq!(alt, 0);
                    assert!((x - t.x).abs() < 0x20 && (y - t.y).abs() < 0x20, "{x},{y}");
                }
                SimEvent::CampaignFlightDestroyed { tag: 77, by_player, by, .. } => {
                    assert!(!by_player && by.is_none(), "nobody's kill");
                    gone = true;
                }
                SimEvent::CampaignFlightLanded { tag: 77, .. } => panic!("a drone does not land"),
                _ => {}
            }
        }
        if hit && gone {
            return;
        }
    }
    panic!("hit {hit}, gone {gone}");
}

#[test]
fn an_awacs_orbits_until_called_home() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x001, Some("Tabor"));
    let o = sim.objects()[from];
    let (gx, gy) = (o.x + 0x600, o.y);
    let i = sim.spawn_awacs(from, gx, gy, 88, false);
    run(&mut sim, &idle(), 20 * 60);
    let u = sim.flights()[i];
    assert!(sim.campaign_flight(i).is_some_and(|f| f.tag == 88), "still up");
    assert!((u.x - gx).abs() < 0x300 && (u.y - gy).abs() < 0x300, "{},{}", u.x, u.y);
    sim.recall_flight(88);
    let mut landed = false;
    for _ in 0..60 * 60 {
        landed |= run(&mut sim, &idle(), 1)
            .iter()
            .any(|e| matches!(e, SimEvent::CampaignFlightLanded { tag: 88, .. }));
        if landed {
            break;
        }
    }
    assert!(landed, "the AWACS did not land");
}

#[test]
fn a_drone_that_crashes_short_destroys_nothing() {
    let (_, mut sim) = campaign();
    quiet(&mut sim);
    let from = find(&sim, 0x001, Some("Tabor"));
    let to = find(&sim, 0x401, Some("Neuberg"));
    let i = sim.spawn_drone(from, to, 78, false);
    // On the ground far from its target: it crashes there.
    sim.flights_mut()[i].alt = 0;
    let mut gone = false;
    for _ in 0..60 {
        for e in run(&mut sim, &idle(), 1) {
            match e {
                SimEvent::GroundDestroyed { object, .. } if object == to => panic!("destroyed"),
                SimEvent::CampaignFlightDestroyed { tag: 78, .. } => gone = true,
                _ => {}
            }
        }
    }
    assert!(gone);
    assert_eq!(sim.objects()[to].flags & oflags::DESTROYED, 0);
}
