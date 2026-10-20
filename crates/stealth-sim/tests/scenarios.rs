//! Scenario tests on the shipped game data (`assets/sim`, `assets/mission`).

use std::sync::OnceLock;

use stealth_mission::{GameData, Mission, MissionKind, MissionSettings, Seed, Weather};
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

fn mission(d: &Data, theater: usize, seed: u16, opponents: u8) -> Mission {
    let s = MissionSettings { theater, roe: 2, opponents, kind: MissionKind::Strike, ..Default::default() };
    stealth_mission::generate(&d.gdata, &s, Seed::Srand(seed)).unwrap()
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
    let d = data();
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
}

#[test]
fn site_detects_and_launches() {
    let d = data();
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

/// The radar warning receiver's list: sites in their detection range, hostile and alive,
/// ranked with a launching site first.
#[test]
fn emitters_ranked() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    let (mx, my) = sandbox(&mut sim, &m);
    // SA-2 10 km north, SA-6 20 km east, a destroyed and a friendly one near, one out of range.
    place_site(&mut sim, 3, 1, mx, my - 640);
    place_site(&mut sim, 4, 6, mx + 1280, my);
    place_site(&mut sim, 5, 1, mx - 640, my);
    sim.objects_mut()[5].flags |= 0x0080;
    place_site(&mut sim, 6, 1, mx, my + 640);
    sim.objects_mut()[6].flags |= 0x0400;
    place_site(&mut sim, 7, 1, mx, my + 64 * 250);
    let mut p = player_at(mx, my, 6000, 0x4000, 200);
    sim.step(&p, 15);
    let e = sim.emitters();
    let sites: Vec<EmitterSource> = e.iter().map(|e| e.source).collect();
    assert_eq!(sites.len(), 2, "{e:?}");
    assert!(sites.contains(&EmitterSource::Site(3)) && sites.contains(&EmitterSource::Site(4)));
    let sa2 = e.iter().find(|e| e.source == EmitterSource::Site(3)).unwrap();
    assert_eq!((sa2.range, sa2.radar_type), (10, 1));
    assert!(sa2.bearing < 0x100 || sa2.bearing > 0xFF00, "north: {:#x}", sa2.bearing);
    assert_eq!(sim.threat_name(1), "SA-2");
    // Detected, tracked and fired at: the SA-2 comes first.
    p.cmd.toggle_bay = true;
    for _ in 0..15 * 120 {
        sim.step(&p, 15);
        p.cmd = Commands::default();
        if sim.projectiles()[3].ttl > 0 {
            break;
        }
    }
    let e = sim.emitters();
    assert_eq!(e[0].source, EmitterSource::Site(3));
    assert!(e[0].launch && e[0].tracking, "{:?}", e[0]);
}

/// A display's cursor lock (HSD, HARM) holds without the camera page, and survives the HUD
/// mode change that comes with it.
#[test]
fn display_lock_holds() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    let (mx, my) = sandbox(&mut sim, &m);
    place_site(&mut sim, 3, 1, mx, my - 640);
    let mut p = player_at(mx, my, 6000, 0, 300);
    p.camera_mfd = false;
    p.hud_mode = HudMode::Gnd;
    p.cmd.lock_object = Some(3);
    sim.step(&p, 15);
    p.cmd = Commands::default();
    for _ in 0..60 {
        sim.step(&p, 15);
    }
    assert_eq!(sim.targeting().ground, Some(3));
    // A target key releases it to the normal cycling.
    p.cmd.clear_target = true;
    sim.step(&p, 15);
    assert_eq!(sim.targeting().ground, None);
}

/// External stores (F-16): the bay is open from the start and stays open, which the EMV shows.
#[test]
fn external_stores_always_open() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let opts = SimOptions { external_stores: true, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    let mut p = player_at(mx, my, 3000, 0, 300);
    sim.step(&p, 15);
    assert!(sim.bay_doors_open());
    let open_emv = sim.emv();
    p.cmd.toggle_bay = true;
    for _ in 0..30 {
        sim.step(&p, 15);
        p.cmd = Commands::default();
    }
    assert!(sim.bay_doors_open());
    let mut closed = Sim::new(&d.sim, &m);
    sandbox(&mut closed, &m);
    closed.step(&player_at(mx, my, 3000, 0, 300), 15);
    assert!(!closed.bay_doors_open());
    assert_eq!(open_emv, closed.emv() + 24);
}

/// Low, fast-ish, bays closed over flat ground: the same SA-2 never detects.
#[test]
fn stealthy_player_not_detected() {
    let d = data();
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
    let d = data();
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
    let d = data();
    // SA-2 is class 1 (beam rider): ECM jams it at any range.
    let (mut sim, mut p) = engaged(d, 1);
    p.cmd.toggle_ecm = true;
    sim.step(&p, 15);
    assert!(sim.ecm());
    assert!(!sim.projectiles()[3].locked || sim.projectiles()[3].ttl == 0);
}

#[test]
fn chaff_breaks_semi_active_lock() {
    let d = data();
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
    let d = data();
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
    let d = data();
    assert!(aim9_shot(d, Weather::default()));
}

/// An overcast deck: weather is not part of the original game.
fn deck(base_ft: i32, top_ft: i32) -> Weather {
    Weather::stratus_layer(base_ft, top_ft, 100, 1)
}

#[test]
fn clouds_blind_the_sidewinder() {
    let d = data();
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
    let d = data();
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
        // The An-72 has no radar and never saw the player.
        let r = sim.mission_report();
        assert_eq!((r.air_kills, r.unseen_kills, r.weapons_fired), (1, 1, 1));
        assert!(r.timeline.iter().any(|t| t.text == "Shot down An-72"), "{:?}", r.timeline);
    }
    killed
}

/// Retarded bomb dropped straight onto the primary: kill iff compat << 6 / (diff + 2) > distance.
#[test]
fn bomb_vs_target_uses_effectiveness_table() {
    let d = data();
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
            let r = sim.mission_report();
            assert!(r.primary_done && r.training && r.ground_kills >= 1);
            assert!(r.timeline.iter().any(|t| t.text.ends_with("(primary)")), "{:?}", r.timeline);
            // Campaign damage: the next mission starts with the target destroyed.
            let damage = sim.world_damage();
            assert!(damage.objects.contains(&(t as u16)) || t < 3);
            assert!(damage.near(o.x, o.y, 0x10) && !damage.tiles.is_empty());
            let json = serde_json::to_string(&damage).unwrap();
            let back: WorldDamage = serde_json::from_str(&json).unwrap();
            assert_eq!(back, damage);
            let next = Sim::with_options(&d.sim, &m, SimOptions { prior_damage: Some(back), ..Default::default() });
            if t >= 3 {
                assert!(next.objects()[t].flags & oflags::DESTROYED != 0);
            }
            assert_eq!(next.world_damage(), damage);
        } else {
            assert!(msgs.iter().any(|s| s.ends_with(" misses ")), "{msgs:?}");
        }
    }
}

#[test]
fn bay_closed_and_altitude_gates() {
    let d = data();
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
    let d = data();
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
    let d = data();
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
                let h = stealth_sim::math::bearing(((tx as i64 - wx) >> 6) as i32, ((ty as i64 - wy) >> 6) as i32);
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
                    nearest_target: r == 110,
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
    let d = data();
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

#[test]
fn nearest_target_skips_friendlies_and_waits_for_hud_mode() {
    let d = data();
    let m = mission(d, 3, 0x1234, 0);
    let mut sim = Sim::new(&d.sim, &m);
    let (cx, cy) = sandbox(&mut sim, &m);
    for o in sim.objects_mut().iter_mut() {
        o.flags |= 0x80;
    }
    // Ahead (north): a friendly object at 300, a hostile one at 600 map units.
    place_site(&mut sim, 1, 0, cx, cy - 300);
    sim.objects_mut()[1].flags = 0x0400;
    place_site(&mut sim, 2, 0, cx, cy - 600);
    sim.set_bay(0, 9, 2);
    sim.select_bay(0);
    let mut p = player_at(cx, cy, 2000, 0, 300);
    p.camera_mfd = true;
    p.cmd.nearest_target = true;
    // Still in NAV: the request waits for the HUD to switch.
    sim.step(&p, 15);
    p.cmd = Commands::default();
    sim.step(&p, 15);
    p.hud_mode = HudMode::Gnd;
    sim.step(&p, 15);
    assert_eq!(sim.targeting().ground, Some(2));
}

fn drone_mission(d: &Data, seed: u16, opponents: u8) -> Mission {
    let theater = stealth_formats::ALL_THEATERS.iter().position(|t| *t == "UA").unwrap();
    let s = MissionSettings { theater, roe: 1, opponents, kind: MissionKind::DroneDefense, ..Default::default() };
    stealth_mission::generate(&d.gdata, &s, Seed::Srand(seed)).unwrap()
}

/// Nobody stops the wave: every drone reaches the target and the defense fails.
#[test]
fn unopposed_drone_wave_gets_through() {
    let d = data();
    let m = drone_mission(d, 42, 1);
    let w = m.drones.clone().unwrap();
    let mut sim = Sim::new(&d.sim, &m);
    sim.set_training(true);
    // Out of the way, high over the base.
    let p = player_at(m.takeoff.position.x, m.takeoff.position.y, 30000, 0, 300);
    let mut impacts = 0;
    for _ in 0..15 * 60 * 40 {
        impacts += sim.step(&p, 15).iter().filter(|e| matches!(e, SimEvent::DroneImpact { .. })).count();
        if sim.drones().unwrap().resolved {
            break;
        }
    }
    let s = sim.drones().unwrap();
    assert!(s.resolved, "after {} s: {s:?}", sim.mission_tick());
    assert_eq!((s.leaked, s.killed, impacts), (w.count as i32, 0, w.count));
    assert!(!sim.objectives().primary_done);
    let r = sim.mission_report();
    assert_eq!(r.drones_leaked, w.count as i32);
    // They arrive spread out, 9 minutes and more after the start.
    assert!(sim.mission_tick() > 9 * 60, "{}", sim.mission_tick());
}

/// A Sidewinder from behind kills a drone; it counts toward the defense.
#[test]
fn sidewinder_kills_a_drone() {
    let d = data();
    let m = drone_mission(d, 7, 0);
    let w = m.drones.clone().unwrap();
    let mut sim = Sim::new(&d.sim, &m);
    sim.set_training(true);
    let u = sim.flights()[w.first_unit];
    // 2 km behind the first drone, at its altitude, on its heading.
    let (hs, hc) = ((u.heading as u16 as f64 / 65536.0 * std::f64::consts::TAU).sin(), (u.heading as u16 as f64 / 65536.0 * std::f64::consts::TAU).cos());
    let (px, py) = (u.x - (hs * 128.0) as i32, u.y + (hc * 128.0) as i32);
    sim.set_bay(0, 0, 4);
    sim.select_bay(0);
    let mut p = player_at(px, py, u.alt as u16, u.heading as u16, 200);
    p.cmd.toggle_bay = true;
    sim.step(&p, 15);
    p.cmd = Commands::default();
    for _ in 0..5 {
        sim.step(&p, 15);
    }
    p.cmd.fire = true;
    sim.step(&p, 15);
    p.cmd = Commands::default();
    for _ in 0..15 * 20 {
        sim.step(&p, 15);
        if sim.drones().unwrap().killed > 0 {
            break;
        }
    }
    assert_eq!(sim.drones().unwrap().killed, 1, "{:?}", sim.flights()[w.first_unit]);
    assert_eq!(sim.mission_report().drones_killed, 1);
}

/// Ejects at 2000 ft over map point (x, y) and steps until the chute is down.
fn eject_at(sim: &mut Sim, x: i32, y: i32) -> PlayerState {
    let mut p = player_at(x, y, 2000, 0, 250);
    p.cmd.eject = true;
    let ev = sim.step(&p, 15);
    assert!(ev.contains(&SimEvent::Ejected { success: true }), "{ev:?}");
    p.cmd = Commands::default();
    for _ in 0..15 * 60 {
        sim.step(&p, 15);
        if sim.csar().is_some() || sim.mission_end().is_some() {
            break;
        }
    }
    p
}

fn enemy_cell(m: &Mission) -> (i32, i32) {
    let g = &m.world.grid;
    let c = (0..256).find(|&i| g[i] & 3 == 0 && g[i] & 0x0C >= 8 && (3..13).contains(&(i % 16)) && (3..13).contains(&(i / 16))).expect("enemy land");
    (((c % 16) as i32) * 2048 + 1024, ((c / 16) as i32) * 2048 + 1024)
}

fn disable_airfields(sim: &mut Sim, friendly: bool) {
    for o in sim.objects_mut().iter_mut() {
        if o.flags & oflags::AIRFIELD != 0 && (o.flags & oflags::FRIENDLY != 0) == friendly {
            o.flags |= oflags::DISABLED;
        }
    }
}

/// Down over enemy territory with no Hinds about: the UH-60 comes, the flare brings it in.
#[test]
fn csar_rescues_the_pilot() {
    let d = data();
    let m = mission(d, 3, 0x2345, 0);
    let mut sim = Sim::new(&d.sim, &m);
    disable_airfields(&mut sim, false);
    let (x, y) = enemy_cell(&m);
    let mut p = eject_at(&mut sim, x, y);
    let c = sim.csar().expect("rescue started").clone();
    assert!(c.hunters.is_empty());
    let mut flared = false;
    for _ in 0..15 * 60 * 15 {
        let r = sim.flights()[c.rescue];
        p.cmd.flare = !flared && (r.x - x).abs() + (r.y - y).abs() < 0x100;
        flared |= p.cmd.flare;
        sim.step(&p, 15);
        if sim.mission_end().is_some() {
            break;
        }
    }
    let e = sim.mission_end().expect("mission over");
    assert_eq!((e.outcome, e.landing_type), (0, 4), "{:?}", sim.csar());
    assert!(flared);
    assert_eq!(sim.mission_report().outcome, score::Outcome::Rescued);
}

/// Down next to an enemy airfield after showing a flare: the Hinds get there first.
#[test]
fn csar_hinds_capture_the_pilot() {
    let d = data();
    let m = mission(d, 3, 0x2345, 3);
    let mut sim = Sim::new(&d.sim, &m);
    let objects = sim.objects().to_vec();
    let (bx, by) = objects
        .iter()
        .filter(|o| o.flags & (oflags::AIRFIELD | oflags::FRIENDLY | oflags::NEUTRAL | oflags::CARRIER) == oflags::AIRFIELD)
        .map(|o| (o.x, o.y))
        .find(|&(x, y)| m.world.grid[((y >> 11) * 16 + (x >> 11)) as usize] & 3 == 0)
        .expect("enemy airfield in enemy territory");
    let mut p = eject_at(&mut sim, bx + 0x60, by);
    assert!(!sim.csar().expect("rescue started").hunters.is_empty());
    p.cmd.flare = true;
    sim.step(&p, 15);
    p.cmd = Commands::default();
    for _ in 0..15 * 60 * 15 {
        sim.step(&p, 15);
        if sim.mission_end().is_some() {
            break;
        }
    }
    assert_eq!(sim.mission_end().map(|e| e.outcome), Some(8), "{:?}", sim.csar());
    assert_eq!(sim.mission_report().outcome, score::Outcome::Captured);
}

#[test]
fn space_steps_through_the_weapons_then_the_gun() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    sim.set_loadout(&[(0, 1), (-1, 0), (6, 1), (0, 1)]);
    let p = player_at(0x4000, 0x4000, 3000, 0, 300);
    let mut seen = Vec::new();
    for _ in 0..4 {
        let (groups, sel) = sim.weapon_groups();
        seen.push(if sim.gun_selected() { -1 } else { groups[sel.unwrap()].0 });
        let mut q = p.clone();
        q.cmd.next_bay = true;
        sim.step(&q, 15);
    }
    assert_eq!(seen, [0, 6, -1, 0]);
    assert_eq!(sim.weapon_groups().0, [(0, 2), (6, 1)]);
}

/// A burst at an An-72 600 m ahead: the fire button fires the gun only when it is selected.
#[test]
fn trigger_fires_the_selected_gun() {
    let d = data();
    for selected in [false, true] {
        let m = mission(d, 3, 0x1234, 1);
        let mut sim = Sim::new(&d.sim, &m);
        let (mx, my) = sandbox(&mut sim, &m);
        {
            let u = &mut sim.flights_mut()[1];
            *u = FlightUnit { x: mx, y: my - 38, alt: 3000, spec: 9, flags: 0x43, speed: 200, timer: 30000, wp: 3, ..Default::default() };
            u.wx = u.x << 5;
            u.wy = u.y << 5;
        }
        sim.set_loadout(&[(0, 4)]);
        let mut p = player_at(mx, my, 3000, 0, 300);
        if selected {
            p.cmd.next_bay = true;
            sim.step(&p, 15);
            assert!(sim.gun_selected());
        }
        let ammo = sim.gun_ammo();
        p.cmd = Commands { trigger: true, ..Default::default() };
        let mut killed = false;
        for _ in 0..30 {
            for e in sim.step(&p, 15) {
                killed |= matches!(e, SimEvent::AircraftDestroyed { unit: 1, .. });
            }
        }
        assert_eq!(killed, selected, "selected {selected}");
        assert_eq!(sim.gun_ammo() < ammo, selected);
    }
}

/// Rounds keep the aircraft's speed and fall: 2 s after leaving a level aircraft a round is
/// about 20 m lower and (1050 m/s + the aircraft's speed) x 2 s ahead.
#[test]
fn gun_rounds_fly_ballistic() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    let (mx, my) = sandbox(&mut sim, &m);
    let mut p = player_at(mx, my, 5000, 0, 400);
    p.cmd.gun = true;
    sim.step(&p, 15);
    sim.step(&p, 15);
    p.cmd = Commands::default();
    let (b0, _) = sim.bullets();
    let i = b0.iter().position(|b| b.x != 0).expect("a round");
    let start = b0[i].map_pos();
    for _ in 0..30 {
        sim.step(&p, 15);
    }
    let end = sim.bullets().0[i].map_pos();
    let wu = WU_METRES;
    let ahead = (start.1 - end.1) * 32.0 * wu;
    let drop = (start.2 - end.2) * wu;
    let want = (GUN_MUZZLE_SPEED + 400.0 * 0.514_444) * 2.0;
    assert!((ahead - want).abs() < want * 0.01, "{ahead} m ahead, want {want}");
    assert!((drop - 0.5 * GUN_GRAVITY * 4.0).abs() < 2.0, "{drop} m drop");
}
