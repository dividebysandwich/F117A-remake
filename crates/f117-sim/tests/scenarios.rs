//! Scenario tests on real game data (skipped when the original game is not installed).

use std::sync::OnceLock;

use f117_mission::{GameData, Mission, MissionKind, MissionSettings, Seed, Sky, Weather};
use f117_sim::*;

struct Data {
    sim: SimData,
    gdata: GameData,
}

fn data() -> Option<&'static Data> {
    static D: OnceLock<Option<Data>> = OnceLock::new();
    D.get_or_init(|| {
        let game = f117_formats::GameDir::default_location()?;
        let sim = SimData::load(&game).ok()?;
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/mission");
        let gdata = GameData::load_json(&p).or_else(|_| GameData::load(&game)).ok()?;
        Some(Data { sim, gdata })
    })
    .as_ref()
    .or_else(|| {
        eprintln!("original game not found, skipping");
        None
    })
}

fn mission(d: &Data, theater: usize, seed: u16, opponents: u8) -> Mission {
    let s = MissionSettings { theater, roe: 2, opponents, kind: MissionKind::Strike, ..Default::default() };
    f117_mission::generate(&d.gdata, &s, Seed::Srand(seed)).unwrap()
}

/// A clean sandbox: no radars, no aircraft, no friendly bases; returns a map cell centre with
/// flat / open terrain.
fn sandbox(sim: &mut Sim, m: &Mission) -> (i32, i32) {
    for o in sim.objects_mut().iter_mut() {
        o.unit_type = 0;
        o.flags &= !0x0501;
        o.alert = 0;
    }
    for u in sim.flights_mut().iter_mut() {
        u.flags = 0;
    }
    let g = &m.world.grid;
    let c = (0..256).find(|&i| g[i] & 0x0C >= 8 && (3..13).contains(&(i % 16)) && (3..13).contains(&(i / 16))).expect("flat cell");
    (((c % 16) as i32) * 2048 + 1024, ((c / 16) as i32) * 2048 + 1024)
}

fn player_at(mx: i32, my: i32, alt: u16, heading: u16, knots: u16) -> PlayerState {
    let (x, y) = Sim::map_to_world(mx, my);
    PlayerState {
        x,
        y,
        altitude: alt,
        heading,
        velocity: knots * 27,
        gear_up: true,
        fuel: 10000,
        hud_visible: true,
        ..Default::default()
    }
}

fn place_site(sim: &mut Sim, i: usize, ty: i32, x: i32, y: i32) {
    let o = &mut sim.objects_mut()[i];
    o.unit_type = ty;
    o.x = x;
    o.y = y;
    o.flags = 0;
    o.alert = 0;
    o.timer = 0;
}

#[test]
fn sim_data_tables() {
    let Some(d) = data() else { return };
    let s = &d.sim;
    assert_eq!(s.threats[1].name, "SA-2");
    assert_eq!((s.threats[4].lethality, s.threats[4].tier, s.threats[4].flags), (320, 7, 1));
    assert_eq!(s.threats[21].lethality, 500);
    assert_eq!(s.aircraft[4].name, "Su-27");
    assert_eq!((s.aircraft[2].max_speed, s.aircraft[2].agility, s.aircraft[2].radar, s.aircraft[2].model), (700, 5, 19, 16));
    assert_eq!((s.missiles[1].lock_range, s.missiles[1].max_speed, s.missiles[1].class, s.missiles[1].turn), (125, 2000, 1, 4));
    assert_eq!(s.missiles[38].name, "Equip.");
    assert_eq!(s.weapons[6].long_name, "Paveway");
    assert_eq!(s.weapons[16].spec, -1);
    assert_eq!(s.compat[6], [4, 1, 2, 4, 5, 5, 4, 4, 5, 4, 4, 0, 3, 4, 4, 0]);
    assert_eq!(s.compat[18], [2, 0, 3, 2, 0, 1, 2, 3, 2, 2, 3, 0, 1, 0, 2, 0]);
    assert_eq!(s.trig.lut[64], 32767);
    assert_eq!(&s.maneuver[..8], &[3, 3, 2, 1, 0, -1, -2, -1]);
    assert_eq!(s.stores_init, [0, 12, 18, 3]);
    assert_eq!(s.theaters.len(), 9);
    let rt = SimData::from_postcard(&s.to_postcard().unwrap()).unwrap();
    assert_eq!(rt.compat, s.compat);
}

#[test]
fn site_detects_and_launches() {
    let Some(d) = data() else { return };
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    let (mx, my) = sandbox(&mut sim, &m);
    // SA-2 site 10 km north of the player; slow, high, bays open: high EMV.
    place_site(&mut sim, 3, 1, mx, my - 640);
    let mut p = player_at(mx, my, 6000, 0x4000, 200);
    p.cmd.toggle_bay = true;
    let mut launched = None;
    let mut detected_msg = false;
    for f in 0..15 * 120 {
        let ev = sim.step(&p, 15);
        p.cmd = Commands::default();
        for e in &ev {
            match e {
                SimEvent::MissileLaunched { slot, spec, launcher: Launcher::Site(3) } => launched = Some((f, *slot, *spec)),
                SimEvent::Message(s) if s.ends_with(" detected") => detected_msg = true,
                _ => {}
            }
        }
        if launched.is_some() {
            break;
        }
    }
    assert!(sim.emv() > 40, "emv {}", sim.emv());
    assert!(detected_msg);
    let (_, slot, spec) = launched.expect("SA-2 launch");
    assert_eq!(spec, 1);
    assert_eq!(slot, 3);
    assert!(sim.site_alert(3) > 0xC0);
    assert!(sim.objects()[3].flags & 0x1000 != 0, "Radar ID before the launch");
    // The missile flies and eventually hits the (stationary) player.
    let mut hit = false;
    for _ in 0..15 * 120 {
        for e in sim.step(&p, 15) {
            if matches!(e, SimEvent::PlayerHit { .. }) {
                hit = true;
            }
        }
        if hit {
            break;
        }
    }
    assert!(hit, "missile {:?}", sim.projectiles()[3]);
    assert!(sim.damage_effects().hits > 2);
}

/// Low, fast-ish, bays closed over flat ground: the same SA-2 never detects.
#[test]
fn stealthy_player_not_detected() {
    let Some(d) = data() else { return };
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    let (mx, my) = sandbox(&mut sim, &m);
    place_site(&mut sim, 3, 1, mx, my - 64 * 60);
    let p = player_at(mx, my, 200, 0, 300);
    for _ in 0..15 * 60 {
        for e in sim.step(&p, 15) {
            assert!(!matches!(e, SimEvent::MissileLaunched { .. }));
        }
    }
    assert_eq!(sim.site_alert(3), 0);
}

/// Run until a site missile is in the air and locked.
fn engaged(d: &Data, ty: i32) -> (Sim, PlayerState) {
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    let (mx, my) = sandbox(&mut sim, &m);
    place_site(&mut sim, 3, ty, mx, my - 64 * 12);
    let mut p = player_at(mx, my, 6000, 0x4000, 200);
    p.cmd.toggle_bay = true;
    for _ in 0..15 * 300 {
        sim.step(&p, 15);
        p.cmd = Commands::default();
        let m = sim.projectiles()[3];
        if m.ttl > 0 && m.locked {
            return (sim, p);
        }
    }
    panic!("no locked missile of type {ty}");
}

#[test]
fn decoy_soaks_the_missile() {
    let Some(d) = data() else { return };
    let (mut sim, mut p) = engaged(d, 1);
    sim.set_training(false);
    p.cmd.decoy = true;
    let mut hit_msg = false;
    for _ in 0..15 * 50 {
        for e in sim.step(&p, 15) {
            assert!(!matches!(e, SimEvent::PlayerHit { .. }), "decoy must soak the hit");
            if matches!(&e, SimEvent::Message(s) if s.starts_with("Hit by")) {
                hit_msg = true;
            }
        }
        p.cmd = Commands::default();
        assert!(!sim.lights().rad, "RAD light stays dark while the decoy runs");
    }
    assert!(!hit_msg);
    assert_eq!(sim.stores().2, 2);
}

#[test]
fn ecm_jams_beam_rider() {
    let Some(d) = data() else { return };
    // SA-2 is class 1 (beam rider): ECM jams it at any range.
    let (mut sim, mut p) = engaged(d, 1);
    p.cmd.toggle_ecm = true;
    sim.step(&p, 15);
    assert!(sim.ecm());
    assert!(!sim.projectiles()[3].locked || sim.projectiles()[3].ttl == 0);
}

#[test]
fn chaff_breaks_semi_active_lock() {
    let Some(d) = data() else { return };
    // SA-8B: class 2 semi-active, fooled by chaff in its seeker cone.
    let (mut sim, mut p) = engaged(d, 3);
    p.cmd.chaff = true;
    let ev = sim.step(&p, 15);
    assert!(ev.contains(&SimEvent::Message("Chaff released".into())));
    assert!(!sim.projectiles()[3].locked);
    assert!(sim.lights().chaff);
}

#[test]
fn ir_jammer_vs_first_generation_ir() {
    let Some(d) = data() else { return };
    // SA-N-5 is class -1 (IR 1st generation): the IR jammer defeats it at any range.
    let (mut sim, mut p) = engaged(d, 9);
    p.cmd.toggle_irj = true;
    sim.step(&p, 15);
    assert!(sim.ir_jammer());
    assert!(!sim.projectiles()[3].locked || sim.projectiles()[3].ttl == 0);
    // Auto-off after 20 s.
    p.cmd = Commands::default();
    for _ in 0..15 * 21 {
        sim.step(&p, 15);
    }
    assert!(!sim.ir_jammer());
}

#[test]
fn aim9_kills_fighter() {
    let Some(d) = data() else { return };
    assert!(aim9_shot(d, Weather::default()));
}

/// An overcast deck: weather is not part of the original game.
fn deck(base_ft: i32, top_ft: i32) -> Weather {
    Weather { sky: Sky::Overcast, base_ft, top_ft, vis_km: 20, seed: 1 }
}

#[test]
fn clouds_blind_the_sidewinder() {
    let Some(d) = data() else { return };
    // Shooter and target inside the deck: no IR lock. A deck below both does not matter.
    assert!(!aim9_shot(d, deck(4000, 6000)));
    assert!(aim9_shot(d, deck(1000, 3000)));
}

/// Whether a radar site's missile locks on the player (6000 ft) within five minutes.
fn site_missile_locks(d: &Data, ty: i32, weather: Weather) -> bool {
    let mut m = mission(d, 3, 0x1234, 1);
    m.weather = weather;
    let mut sim = Sim::new(&d.sim, &m);
    let (mx, my) = sandbox(&mut sim, &m);
    place_site(&mut sim, 3, ty, mx, my - 64 * 12);
    let mut p = player_at(mx, my, 6000, 0x4000, 200);
    p.cmd.toggle_bay = true;
    for _ in 0..15 * 300 {
        sim.step(&p, 15);
        p.cmd = Commands::default();
        if sim.projectiles().iter().any(|m| m.ttl > 0 && m.locked) {
            return true;
        }
    }
    false
}

#[test]
fn clouds_block_ir_but_not_radar_seekers() {
    let Some(d) = data() else { return };
    // SA-N-5 (IR) from the ground at a player above a 2000-4000 ft deck; SA-8B (radar) does lock.
    assert!(site_missile_locks(d, 9, Weather::default()));
    assert!(!site_missile_locks(d, 9, deck(2000, 4000)));
    assert!(site_missile_locks(d, 3, deck(2000, 4000)));
}

/// Fires an AIM-9 at a slow transport 3 km ahead (both at 5000 ft); true if it is shot down.
fn aim9_shot(d: &Data, weather: Weather) -> bool {
    let mut m = mission(d, 3, 0x1234, 1);
    m.weather = weather;
    let mut sim = Sim::new(&d.sim, &m);
    let (mx, my) = sandbox(&mut sim, &m);
    // A slow An-72 (no radar) 3 km ahead, flying north like the player.
    {
        let u = &mut sim.flights_mut()[1];
        *u = FlightUnit { x: mx, y: my - 64 * 3, alt: 5000, spec: 9, flags: 0x43, speed: 200, timer: 30000, wp: 3, ..Default::default() };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    sim.set_bay(0, 0, 4);
    sim.select_bay(0);
    let mut p = player_at(mx, my, 5000, 0, 300);
    p.cmd.toggle_bay = true;
    sim.step(&p, 15);
    p.cmd = Commands::default();
    for _ in 0..5 {
        sim.step(&p, 15);
    }
    assert!(sim.bay_doors_open());
    p.cmd.fire = true;
    let ev = sim.step(&p, 15);
    assert!(ev.contains(&SimEvent::Message("Sidewinder fired".into())), "{ev:?}");
    p.cmd = Commands::default();
    let mut killed = false;
    for _ in 0..15 * 20 {
        for e in sim.step(&p, 15) {
            if matches!(e, SimEvent::AircraftDestroyed { unit: 1, .. }) {
                killed = true;
            }
        }
    }
    if killed {
        assert!(sim.flights()[1].flags & 0x20 != 0);
        assert!(sim.event_log().iter().any(|e| e.kind == 3 && e.arg == 9));
    }
    killed
}

/// Retarded bomb dropped straight onto the primary: kill iff compat << 6 / (diff + 2) > distance.
#[test]
fn bomb_vs_target_uses_effectiveness_table() {
    let Some(d) = data() else { return };
    let (m, seed) = (1..400u16).map(|s| (mission(d, 3, s, 0), s)).find(|(m, _)| m.primary.mission_type == 2).unwrap();
    let t = m.primary.object_index as usize;
    let cat = (m.world.buf7[m.world.objects[t].shape()] & 0xF) as i32;
    // Retarded weapons (spec 29): Rockeye 7, Durandal 8, Snakeye 10, IN Cluster 14, Minelets 15.
    let good = [7, 8, 10, 14, 15].into_iter().find(|&w| d.sim.compat_cat(w, cat) > 0);
    let bad = [7, 8, 10, 14, 15].into_iter().find(|&w| d.sim.compat_cat(w, cat) == 0);
    for (w, expect_kill) in [(good, true), (bad, false)] {
        let Some(w) = w else { continue };
        let mut sim = Sim::new(&d.sim, &m);
        sim.set_training(true);
        let o = sim.objects()[t];
        sim.set_bay(0, w, 2);
        sim.select_bay(0);
        let mut p = player_at(o.x, o.y, 500, 0, 0);
        p.cmd.toggle_bay = true;
        for _ in 0..4 {
            sim.step(&p, 15);
            p.cmd = Commands::default();
        }
        p.cmd.fire = true;
        let mut killed = false;
        let mut msgs = Vec::new();
        for _ in 0..15 * 10 {
            for e in sim.step(&p, 15) {
                match e {
                    SimEvent::GroundDestroyed { object, .. } if object == t => killed = true,
                    SimEvent::Message(s) => msgs.push(s),
                    _ => {}
                }
            }
            p.cmd = Commands::default();
        }
        assert_eq!(killed, expect_kill, "seed {seed} weapon {w} cat {cat}: {msgs:?}");
        if expect_kill {
            assert!(sim.objectives().primary_done);
            assert_eq!(sim.objectives().waypoint, 2);
            assert!(sim.event_log().iter().any(|e| e.kind & 0x80 != 0 && matches!(e.kind & 0x3F, 1 | 12)));
        } else {
            assert!(msgs.iter().any(|s| s.ends_with(" misses ")), "{msgs:?}");
        }
    }
}

#[test]
fn bay_closed_and_altitude_gates() {
    let Some(d) = data() else { return };
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    sim.set_bay(0, 9, 3);
    sim.select_bay(0);
    let mut p = player_at(0x4000, 0x4000, 3000, 0, 300);
    p.cmd.fire = true;
    let ev = sim.step(&p, 15);
    assert!(ev.contains(&SimEvent::Message("Bay closed".into())));
    assert_eq!(sim.bays().0[0].1, 3);
}

#[test]
fn countermeasure_stores_run_out() {
    let Some(d) = data() else { return };
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    let mut p = player_at(0x4000, 0x4000, 3000, 0, 300);
    let mut exhausted = false;
    for _ in 0..20 {
        p.cmd.flare = true;
        for e in sim.step(&p, 15) {
            if e == SimEvent::Message("Stores exhausted".into()) {
                exhausted = true;
            }
        }
    }
    assert!(exhausted);
    assert_eq!(sim.stores().0, 0);
}

/// Ten simulated minutes of every theater with a scripted flight and random commands.
#[test]
fn ten_minute_missions_all_theaters() {
    let Some(d) = data() else { return };
    let trig = &d.sim.trig;
    for th in 0..9 {
        for (k, opp) in [(0u16, 0u8), (1, 3)] {
            let m = mission(d, th, 0x100 + th as u16 * 7 + k, opp);
            let mut sim = Sim::new(&d.sim, &m);
            let mut p = sim.start_state();
            let mut rng = Rng::new(th as u32 * 77 + k as u32);
            let (mut wx, mut wy) = (p.x as i64, p.y as i64);
            let mut alt = 0i32;
            let mut ended = false;
            for f in 0..15 * 600 {
                // Climb out and fly towards the primary, then the secondary.
                let wp = m.waypoints[if f < 15 * 300 { 1 } else { 2 }];
                let (tx, ty) = Sim::map_to_world(wp.x, wp.y);
                let h = f117_sim::math::bearing(((tx as i64 - wx) >> 6) as i32, ((ty as i64 - wy) >> 6) as i32);
                p.heading = h as u16;
                alt = (alt + 20).min(if f < 15 * 400 { 3000 } else { 300 });
                p.altitude = alt as u16;
                p.velocity = 450 * 27;
                p.on_ground = false;
                p.gear_up = true;
                p.roll = ((f % 300) as i16 - 150) * 40;
                let v = (p.velocity as i32 * 4) / 15 / 64;
                wx += trig.sin_mul(h as i32, v) as i64 * 4;
                wy += trig.cos_mul(h as i32, v) as i64 * 4;
                p.x = wx as i32;
                p.y = wy as i32;
                p.hud_mode = [HudMode::Nav, HudMode::Air, HudMode::Gnd][(f / 900) % 3];
                p.camera_mfd = f % 1000 < 600;
                let r = rng.random(1000);
                p.cmd = Commands {
                    fire: r < 20,
                    gun: r % 7 == 0,
                    flare: r == 100,
                    chaff: r == 101,
                    decoy: r == 102,
                    toggle_ecm: r == 103,
                    toggle_irj: r == 104,
                    toggle_bay: r == 105,
                    next_bay: r == 106,
                    designate: r == 107,
                    next_target: r == 108,
                    camera_dir: (r == 109).then_some(0x4000),
                    ..Default::default()
                };
                for e in sim.step(&p, 15) {
                    if matches!(e, SimEvent::MissionEnd(_)) {
                        ended = true;
                    }
                }
                let _ = sim.targeting();
                let _ = sim.active_aircraft();
                if ended {
                    break;
                }
            }
            eprintln!("theater {th}: {} s, end {:?}, launches {}, log {}", sim.mission_tick(), sim.mission_end(), sim.launches, sim.event_log().len());
            assert!(sim.mission_tick() > 0);
        }
    }
}


/// Supply drop: unreachable in the original (spec vs class test), fixed behind `fix_original_bugs`.
#[test]
fn supply_drop_fixed_and_original() {
    let Some(d) = data() else { return };
    let m = mission(d, 3, 0x1234, 1);
    for fix in [true, false] {
        let mut sim = Sim::with_options(&d.sim, &m, SimOptions { fix_original_bugs: fix, ..Default::default() });
        sim.set_training(true);
        let o = sim.objects()[5];
        sim.set_deadline(1000);
        sim.target_slots_mut()[1] = TargetSlot { mission_type: 3, object: 5, base: 0, code: 0x12 };
        sim.set_bay(0, 18, 1);
        sim.select_bay(0);
        let mut p = player_at(o.x, o.y + 10, 1000, 0, 150);
        p.cmd.toggle_bay = true;
        for _ in 0..4 {
            sim.step(&p, 15);
            p.cmd = Commands::default();
        }
        p.cmd.fire = true;
        let mut delivered = false;
        for _ in 0..15 * 30 {
            for e in sim.step(&p, 15) {
                if e == SimEvent::Message("Equipment Delivered".into()) {
                    delivered = true;
                }
            }
            p.cmd = Commands::default();
        }
        assert_eq!(delivered, fix, "fix {fix}");
        assert_eq!(sim.objectives().secondary_done, delivered);
    }
}
