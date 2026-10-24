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
    let s = MissionSettings {
        theater,
        roe: 2,
        opponents,
        kind: MissionKind::Strike,
        ..Default::default()
    };
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
    let c = (0..256)
        .find(|&i| g[i] & 0x0C >= 8 && (3..13).contains(&(i % 16)) && (3..13).contains(&(i / 16)))
        .expect("flat cell");
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
    assert_eq!(
        (s.aircraft[2].max_speed, s.aircraft[2].agility, s.aircraft[2].radar, s.aircraft[2].model),
        (700, 5, 19, 16)
    );
    assert_eq!(
        (
            s.missiles[1].lock_range,
            s.missiles[1].max_speed,
            s.missiles[1].class,
            s.missiles[1].turn
        ),
        (125, 2000, 1, 4)
    );
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
                SimEvent::MissileLaunched { slot, spec, launcher: Launcher::Site(3) } => {
                    launched = Some((f, *slot, *spec))
                }
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

/// An overcast deck.
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
        *u = FlightUnit {
            x: mx,
            y: my - 64 * 3,
            alt: 5000,
            spec: 9,
            flags: 0x43,
            speed: 200,
            timer: 30000,
            wp: 3,
            ..Default::default()
        };
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
    let (m, seed) = (1..400u16)
        .map(|s| (mission(d, 3, s, 0), s))
        .find(|(m, _)| m.primary.mission_type == 2)
        .unwrap();
    let t = m.primary.object_index as usize;
    let cat = (m.world.shape_class[m.world.objects[t].shape()] & 0xF) as i32;
    // Retarded weapons (spec 29): Rockeye 7, Durandal 8, Snakeye 10, Mk 35 incendiary 14, mines 15.
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
            assert!(
                sim.event_log()
                    .iter()
                    .any(|e| e.kind & 0x80 != 0 && matches!(e.kind & 0x3F, 1 | 12))
            );
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
            let next = Sim::with_options(
                &d.sim,
                &m,
                SimOptions { prior_damage: Some(back), ..Default::default() },
            );
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

/// Ten simulated minutes of one theater with a scripted flight and random commands.
fn ten_minute_mission(d: &Data, th: usize, k: u16, opp: u8, opts: SimOptions) -> Sim {
    let trig = &d.sim.trig;
    let m = mission(d, th, 0x100 + th as u16 * 7 + k, opp);
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let mut p = sim.start_state();
    let mut rng = Rng::new(th as u32 * 77 + k as u32);
    let (mut wx, mut wy) = (p.x as i64, p.y as i64);
    let mut alt = 0i32;
    let mut ended = false;
    for f in 0..15 * 600 {
        // Climb out and fly towards the primary, then the secondary.
        let wp = m.waypoints[if f < 15 * 300 { 1 } else { 2 }];
        let (tx, ty) = Sim::map_to_world(wp.x, wp.y);
        let h = stealth_sim::math::bearing(
            ((tx as i64 - wx) >> 6) as i32,
            ((ty as i64 - wy) >> 6) as i32,
        );
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
    sim
}

/// Ten simulated minutes of every theater with a scripted flight and random commands.
#[test]
fn ten_minute_missions_all_theaters() {
    let d = data();
    for th in 0..9 {
        for (k, opp) in [(0u16, 0u8), (1, 3)] {
            let sim = ten_minute_mission(d, th, k, opp, SimOptions::default());
            eprintln!(
                "theater {th}: {} s, end {:?}, launches {}, log {}",
                sim.mission_tick(),
                sim.mission_end(),
                sim.launches(),
                sim.event_log().len()
            );
            assert!(sim.mission_tick() > 0);
        }
    }
}

/// FNV-1a over the serialized state.
fn state_hash(sim: &Sim) -> u64 {
    serde_json::to_vec(sim)
        .unwrap()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// Classic behaviour is pinned: the end state of the ten-minute missions with default options
/// must not change when optional features are added.
#[test]
fn classic_state_golden() {
    const GOLDEN: [u64; 18] = [
        0x26bec1741250c262,
        0xb0a05c2703a62333,
        0x8b253708c58e4a18,
        0x91369bedb474057c,
        0xcd9266f3a3ef8711,
        0x443f6790c107d6a5,
        0x620bb333986c8954,
        0x9629c627252309ec,
        0x27e911ff5ba3ca7b,
        0x8d84039dc5a1d4b9,
        0x638e1187ecbd2f98,
        0x2f2669b82e5aae26,
        0xce82c1aa2d456393,
        0x4260c4d304b6e272,
        0x7bffb6892ef438af,
        0xb73cd7b9c71c2443,
        0x8c140fbee4673d51,
        0x50a790c2e8094406,
    ];
    let d = data();
    let mut got = Vec::new();
    for th in 0..9 {
        for (k, opp) in [(0u16, 0u8), (1, 3)] {
            got.push(state_hash(&ten_minute_mission(d, th, k, opp, SimOptions::default())));
        }
    }
    let hex: Vec<String> = got.iter().map(|h| format!("0x{h:016x}")).collect();
    assert_eq!(got, GOLDEN, "new hashes: [{}]", hex.join(", "));
}

/// Supply drop: unreachable in classic mode (spec vs class test), fixed behind `fix_classic_bugs`.
#[test]
fn supply_drop_fixed_and_classic() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    for fix in [true, false] {
        let mut sim = Sim::with_options(
            &d.sim,
            &m,
            SimOptions { fix_classic_bugs: fix, ..Default::default() },
        );
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
    let s = MissionSettings {
        theater,
        roe: 1,
        opponents,
        kind: MissionKind::DroneDefense,
        ..Default::default()
    };
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
        impacts +=
            sim.step(&p, 15).iter().filter(|e| matches!(e, SimEvent::DroneImpact { .. })).count();
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
    let (hs, hc) = (
        (u.heading as u16 as f64 / 65536.0 * std::f64::consts::TAU).sin(),
        (u.heading as u16 as f64 / 65536.0 * std::f64::consts::TAU).cos(),
    );
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
    let c = (0..256)
        .find(|&i| {
            g[i] & 3 == 0
                && g[i] & 0x0C >= 8
                && (3..13).contains(&(i % 16))
                && (3..13).contains(&(i / 16))
        })
        .expect("enemy land");
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
        .filter(|o| {
            o.flags & (oflags::AIRFIELD | oflags::FRIENDLY | oflags::NEUTRAL | oflags::CARRIER)
                == oflags::AIRFIELD
        })
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
            *u = FlightUnit {
                x: mx,
                y: my - 38,
                alt: 3000,
                spec: 9,
                flags: 0x43,
                speed: 200,
                timer: 30000,
                wp: 3,
                ..Default::default()
            };
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
    // One round (the gun fires every frame).
    p.cmd.gun = true;
    sim.step(&p, 15);
    p.cmd = Commands::default();
    let (b0, _) = sim.bullets();
    assert_eq!(b0.iter().filter(|b| b.x != 0).count(), 1);
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

/// Bombs released in level flight at 500 ft and 450 knots land on the CCIP point predicted at
/// release: a slick bomb far ahead, a retarded one on a shorter arc (not straight down).
#[test]
fn bombs_land_on_the_ccip_point() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let mut ahead = Vec::new();
    for w in [9, 10] {
        let mut sim = Sim::new(&d.sim, &m);
        let (mx, my) = sandbox(&mut sim, &m);
        sim.set_training(true);
        sim.set_loadout(&[(w, 2)]);
        let mut p = player_at(mx, my, 500, 0, 450);
        p.hud_mode = HudMode::Gnd;
        p.cmd.toggle_bay = true;
        for _ in 0..4 {
            sim.step(&p, 15);
            p.cmd = Commands::default();
        }
        let ccip = sim.targeting().ccip.expect("CCIP point");
        p.cmd.fire = true;
        sim.step(&p, 15);
        p.cmd = Commands::default();
        let slot = (8..12).find(|&s| sim.projectiles()[s].ttl != 0).expect("bomb released");
        let mut frames = 0;
        while sim.projectiles()[slot].ttl != 0 && frames < 15 * 30 {
            sim.step(&p, 15);
            frames += 1;
        }
        let (x, y, alt) = sim.projectiles()[slot].map_pos();
        assert!(alt.abs() < 1e-6, "weapon {w}: landed at {alt}");
        let (x, y) = (x as i32, y as i32);
        assert!(
            (x - ccip.0).abs() <= 1 && (y - ccip.1).abs() <= 1,
            "weapon {w}: impact {x},{y} ccip {ccip:?}"
        );
        assert!((x - mx).abs() <= 1);
        ahead.push(my - y);
    }
    // 1 map unit is 32 ft: the slick bomb flies about 4,000 ft, the retarded one under 3,000.
    assert!(ahead[0] > 100 && ahead[1] > 40 && ahead[1] < ahead[0] * 3 / 4, "{ahead:?}");
}

/// CCRP: flying level at a locked target with the fire button held, the bomb is not released
/// on the press but when the target passes the release point, and lands on the target.
#[test]
fn ccrp_releases_at_the_release_point() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    for w in [9, 10] {
        let mut sim = Sim::new(&d.sim, &m);
        let (mx, my) = sandbox(&mut sim, &m);
        sim.set_training(true);
        sim.set_loadout(&[(w, 2)]);
        // The target 400 map units (12,800 ft) north of the start.
        let t = 5;
        {
            let o = &mut sim.objects_mut()[t];
            o.x = mx;
            o.y = my - 400;
            o.flags = 0;
        }
        let knots = 450u16;
        let mut p = player_at(mx, my, 500, 0, knots);
        p.hud_mode = HudMode::Gnd;
        p.ccrp = true;
        p.cmd.toggle_bay = true;
        p.cmd.lock_object = Some(t as u16);
        let step = knots as f64 * 27.0 / 16.0 / 15.0;
        let mut y = p.y as f64;
        let mut released_at = None;
        for f in 0..15 * 40 {
            if f == 5 {
                p.cmd.fire = true;
            }
            p.cmd.trigger = f >= 5;
            sim.step(&p, 15);
            p.cmd = Commands { trigger: p.cmd.trigger, ..Default::default() };
            if released_at.is_none() && sim.weapon_groups().0[0].1 < 2 {
                released_at = Some(f);
                assert!(f > 5, "weapon {w}: released on the press");
            }
            if released_at.is_some() && (8..12).all(|s| sim.projectiles()[s].ttl == 0) {
                break;
            }
            y += step;
            p.y = y as i32;
        }
        assert!(released_at.is_some(), "weapon {w}: not released");
        let slot = (8..12).find(|&s| sim.projectiles()[s].fall.is_some()).unwrap();
        let (x, y, _) = sim.projectiles()[slot].map_pos();
        let o = sim.objects()[t];
        assert!(
            (x - (o.x as f64 + 0.5)).abs() < 2.0 && (y - (o.y as f64 + 0.5)).abs() < 3.0,
            "weapon {w}: impact {x},{y} target {},{}",
            o.x,
            o.y
        );
        assert_eq!(sim.weapon_groups().0[0].1, 1, "weapon {w}: one bomb");
    }
}

/// An SA-2 site 10 km north of a slow, high player with open bays fires until a hit; returns
/// the simulation after the first hit (None: no hit in 4 minutes).
fn sa2_hit(opts: SimOptions) -> Option<(Sim, PlayerState)> {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    place_site(&mut sim, 3, 1, mx, my - 640);
    let mut p = player_at(mx, my, 6000, 0x4000, 200);
    p.cmd.toggle_bay = true;
    for _ in 0..15 * 240 {
        let ev = sim.step(&p, 15);
        p.cmd = Commands::default();
        if ev.iter().any(|e| matches!(e, SimEvent::PlayerHit { .. })) {
            return Some((sim, p));
        }
    }
    None
}

/// Realistic damage: some missile hits destroy the aircraft (it explodes seconds later, ending
/// the mission), some start fires, control hits bend the controls; classic damage never ends
/// the mission.
#[test]
fn realistic_missile_damage() {
    let (mut destroyed, mut exploded, mut bent, mut survived) = (0, 0, 0, 0);
    for seed in 0..24u32 {
        let opts = SimOptions {
            seed: Some(seed * 7919 + 1),
            realistic_damage: true,
            ..Default::default()
        };
        let Some((mut sim, p)) = sa2_hit(opts) else { continue };
        let fx = sim.damage_effects();
        destroyed += fx.destroyed as i32;
        if fx.destroyed {
            assert_eq!(fx.throttle_cap, Some(0), "the engine dies with the airframe");
            assert!(fx.controls != ControlFault::default());
        }
        bent += (fx.controls != ControlFault::default()) as i32;
        // The site is gone: only the damage runs its course.
        sim.objects_mut()[3].unit_type = 0;
        for _ in 0..15 * 90 {
            sim.step(&p, 15);
        }
        match sim.mission_end() {
            Some(e) => {
                assert_eq!(e.outcome, 1);
                exploded += 1;
            }
            None => {
                assert!(!sim.damage_effects().destroyed);
                survived += 1;
            }
        }
    }
    assert!(
        destroyed > 0 && exploded >= destroyed && survived > 0,
        "{destroyed} {exploded} {survived}"
    );
    assert!(bent > 0);

    for seed in 0..8u32 {
        let opts = SimOptions { seed: Some(seed * 7919 + 1), ..Default::default() };
        let Some((mut sim, p)) = sa2_hit(opts) else { continue };
        assert!(!sim.damage_effects().destroyed);
        assert_eq!(sim.damage_effects().controls, ControlFault::default());
        sim.objects_mut()[3].unit_type = 0;
        for _ in 0..15 * 90 {
            sim.step(&p, 15);
        }
        assert!(sim.mission_end().is_none(), "classic damage is never fatal in the air");
    }
}

/// Realistic guidance: an SA-2 site launches at a player cruising past at 400 kt; the missile
/// flies on its own flight (no map-unit steps), trails smoke only while its motor burns, and
/// hits the moving aircraft.
#[test]
fn guided_sam_hits_a_cruising_player() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let opts = SimOptions { realistic_guidance: true, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    place_site(&mut sim, 3, 1, mx, my - 640);
    let mut p = player_at(mx - 300, my, 6000, 0x4000, 400);
    p.cmd.toggle_bay = true;
    let (mut x, mut y) = (p.x as f64, p.y as f64);
    let (mut launched, mut hit, mut smoke_out) = (false, false, false);
    for _ in 0..15 * 240 {
        let ev = sim.step(&p, 15);
        p.cmd = Commands::default();
        // The player flies on east.
        let v = ballistics::release_velocity(p.heading as i32, p.velocity as i32, 0);
        x += v[0] / 15.0;
        y -= v[1] / 15.0;
        (p.x, p.y) = (x as i32, y as i32);
        let m = sim.projectiles()[3];
        if m.ttl > 0 && m.spec == 1 {
            assert!(m.flight.is_some(), "guided flight");
            launched = true;
            smoke_out |= !m.motor_visible();
        }
        if ev.iter().any(|e| matches!(e, SimEvent::PlayerHit { .. })) {
            hit = true;
            break;
        }
    }
    assert!(launched && hit, "launched {launched} hit {hit}");
    // An SA-2 at 10 km gets there with its sustainer still burning.
    assert!(!smoke_out);
}

/// Visual identification range: the mission's day or night, or the light level.
#[test]
fn visual_range_follows_the_light() {
    let d = data();
    for seed in [0x1234u16, 0x2345, 0x3456] {
        let m = mission(d, 3, seed, 1);
        let mut sim = Sim::new(&d.sim, &m);
        sim.set_visual_light(Some(255));
        let day = sim.visual_range();
        // A storm halves every range.
        let shift = (day == 4) as i32;
        assert_eq!(day, 8 >> shift);
        sim.set_visual_light(Some(0));
        assert_eq!(sim.visual_range(), 2 >> shift);
        sim.set_visual_light(Some(85));
        assert_eq!(sim.visual_range(), 4 >> shift);
        sim.set_visual_light(None);
        assert_eq!(sim.visual_range(), (8 >> m.night as i32) >> shift);
    }
}

fn aaa_mission(d: &Data) -> (Mission, Sim) {
    let s = MissionSettings {
        theater: 3,
        roe: 2,
        opponents: 2,
        kind: MissionKind::Strike,
        weather: Some(stealth_mission::Sky::Clear),
        ..Default::default()
    };
    let m = stealth_mission::generate(&d.gdata, &s, Seed::Srand(0x4321)).unwrap();
    let sim = Sim::with_options(&d.sim, &m, SimOptions { air_defence: true, ..Default::default() });
    (m, sim)
}

/// One unit of `kind` in a sandbox, the player hovering `north` map units north of it.
fn aaa_duel(kind: aaa::AaaKind, north: i32, alt: u16, light: u8) -> Vec<SimEvent> {
    let d = data();
    let (m, mut sim) = aaa_mission(d);
    let (x, y) = sandbox(&mut sim, &m);
    sim.set_visual_light(Some(light));
    let unit = aaa::AaaUnit {
        kind,
        x,
        y,
        destroyed: false,
        tracking: false,
        aim_az: 0.0,
        aim_el: 0.0,
        burst: 0,
        cooldown: 0,
        rounds: 0,
    };
    *sim.aaa_units_mut() = vec![unit];
    let p = player_at(x, y - north, alt, 0x4000, 0);
    let mut events = Vec::new();
    for _ in 0..15 * 30 {
        events.extend(sim.step(&p, 15));
    }
    events
}

#[test]
fn air_defence_only_with_the_option() {
    let d = data();
    let (m, sim) = aaa_mission(d);
    let units = sim.aaa_units();
    assert!(units.iter().any(|u| u.kind == aaa::AaaKind::Zsu), "{units:?}");
    assert!(units.iter().any(|u| u.kind == aaa::AaaKind::Manpads));
    let plain = Sim::new(&d.sim, &m);
    assert!(plain.aaa_units().is_empty());
    // The same mission places the same units.
    assert_eq!(aaa_mission(d).1.aaa_units(), units);
}

#[test]
fn shilka_hits_low_and_slow_but_not_high() {
    // 1.5 km away at 600 ft: hit within 30 s.
    let low = aaa_duel(aaa::AaaKind::Zsu, 96, 600, 255);
    assert!(low.contains(&SimEvent::Message("Hit by AAA".into())), "never hit");
    // 20000 ft: above its reach, it does not even fire.
    let high = aaa_duel(aaa::AaaKind::Zsu, 96, 20000, 255);
    assert!(!high.iter().any(|e| matches!(e, SimEvent::Message(m) if m.starts_with("Hit"))));
}

#[test]
fn s60_bursts_flak() {
    let ev = aaa_duel(aaa::AaaKind::S60, 160, 8000, 255);
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Flak { .. })));
}

#[test]
fn manpads_need_to_see_the_player() {
    let fired = |ev: &[SimEvent]| {
        ev.iter()
            .any(|e| matches!(e, SimEvent::MissileLaunched { launcher: Launcher::Manpads(0), .. }))
    };
    // 3 km away at 2000 ft: by day the team sees and fires, on a moonless night it does not.
    assert!(fired(&aaa_duel(aaa::AaaKind::Manpads, 192, 2000, 255)));
    assert!(!fired(&aaa_duel(aaa::AaaKind::Manpads, 192, 2000, 0)));
}

/// The scripted ten-minute flights with the air defences, in every theater.
#[test]
fn ten_minute_missions_with_air_defence() {
    let d = data();
    let opts = SimOptions { air_defence: true, ..Default::default() };
    let mut fired = 0;
    for th in 0..9 {
        let sim = ten_minute_mission(d, th, 1, 3, opts.clone());
        assert!(!sim.aaa_units().is_empty(), "theater {th}");
        fired += sim.aaa_units().iter().filter(|u| u.tracking).count();
    }
    eprintln!("units tracking at the end: {fired}");
}

/// The S-60s lead a crossing target: their shells burst around it, not where it was.
#[test]
fn flak_leads_a_moving_target() {
    let d = data();
    let (m, mut sim) = aaa_mission(d);
    let (x, y) = sandbox(&mut sim, &m);
    sim.set_visual_light(Some(255));
    let unit = aaa::AaaUnit {
        kind: aaa::AaaKind::S60,
        x,
        y,
        destroyed: false,
        tracking: false,
        aim_az: 0.0,
        aim_el: 0.0,
        burst: 0,
        cooldown: 0,
        rounds: 0,
    };
    *sim.aaa_units_mut() = vec![unit];
    let mut misses = Vec::new();
    // Flying east at 15 map units a second (234 m/s), 1500 ft, 2.5 km north of the gun.
    for f in 0..15 * 20 {
        let px = x - 200 + f;
        let p = player_at(px, y - 160, 1500, 0x4000, 450);
        for e in sim.step(&p, 15) {
            if let SimEvent::Flak { x: bx, y: by, alt } = e {
                let dx = (bx - px) as f64 * 15.625;
                let dy = (by - (y - 160)) as f64 * 15.625;
                let dz = (alt - 1500) as f64 * 0.3048;
                misses.push((dx * dx + dy * dy + dz * dz).sqrt());
            }
        }
    }
    misses.sort_by(f64::total_cmp);
    assert!(misses.len() > 5);
    let median = misses[misses.len() / 2];
    assert!(median < 80.0, "median miss {median:.0} m of {misses:?}");
}

/// The recon cue: range and aim against the limit for the locked photo target, the step the
/// camera still needs, and gone once the target is photographed.
#[test]
fn photo_cue_guides_to_the_recon_target() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let mut sim = Sim::new(&d.sim, &m);
    sim.set_training(true);
    let (cx, cy) = sandbox(&mut sim, &m);
    place_site(&mut sim, 5, 0, cx, cy - 200);
    sim.target_slots_mut()[0] = TargetSlot { mission_type: 1, object: 5, base: 0, code: 0x12 };
    let camera = sim.weapon_types().iter().position(|w| w.spec == -1).unwrap();
    sim.set_bay(0, camera as i32, 1);
    sim.select_bay(0);
    let mut p = player_at(cx, cy, 1000, 0, 300);
    sim.step(&p, 15);
    assert_eq!(sim.targeting().photo, None);
    sim.set_ground_lock(Some(5));
    sim.step(&p, 15);
    let cue = sim.targeting().photo.unwrap();
    assert_eq!(cue.needs, PhotoNeeds::BayOpen);
    assert!((195..=205).contains(&cue.range) && cue.range + cue.aim < cue.limit, "{cue:?}");
    p.cmd.toggle_bay = true;
    sim.step(&p, 15);
    p.cmd = Commands::default();
    for _ in 0..30 {
        sim.step(&p, 15);
    }
    assert_eq!(sim.targeting().photo.unwrap().needs, PhotoNeeds::CameraPage);
    p.camera_mfd = true;
    sim.step(&p, 15);
    assert_eq!(sim.targeting().photo.unwrap().needs, PhotoNeeds::Nothing);
    p.cmd.fire = true;
    for _ in 0..6 {
        sim.step(&p, 15);
    }
    assert!(sim.objectives().primary_done);
    assert_eq!(sim.targeting().photo, None);
}

/// A Maverick fired from 3000 ft at a target `range` map units northeast (classic guidance
/// does not steer for a bearing of exactly north), the nose on it: the
/// launch zone at the shot and whether it hit (None: no lock from there).
fn maverick_shot(guided: bool, range: i32) -> Option<(dlz::LaunchZone, bool)> {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let opts = SimOptions { realistic_guidance: guided, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    sim.set_training(true);
    sim.set_loadout(&[(5, 2)]);
    let t = 5;
    {
        let o = &mut sim.objects_mut()[t];
        let diag = (range as f64 / std::f64::consts::SQRT_2).round() as i32;
        o.x = mx + diag;
        o.y = my - diag;
        o.flags = 0;
    }
    let mut p = player_at(mx, my, 3000, 0x2000, 450);
    p.hud_mode = HudMode::Gnd;
    p.cmd.toggle_bay = true;
    p.cmd.lock_object = Some(t as u16);
    for _ in 0..4 {
        sim.step(&p, 15);
        p.cmd = Commands::default();
    }
    // Nose down onto the target until the oval locks.
    (0..96).map(|k| -k * 64).find(|&pitch| {
        p.pitch = pitch as i16;
        sim.step(&p, 15);
        sim.targeting().lock_valid
    })?;
    let zone = sim.launch_zone().expect("launch zone");
    p.cmd.fire = true;
    let mut hit = false;
    for _ in 0..15 * 90 {
        for e in sim.step(&p, 15) {
            if let SimEvent::Message(s) = e {
                hit |= s.contains("hit by AGM-65");
            }
        }
        p.cmd = Commands::default();
    }
    Some((zone, hit))
}

#[test]
fn launch_zone_predicts_maverick_hits() {
    for guided in [false, true] {
        let shots: Vec<_> =
            (1..=40).filter_map(|k| maverick_shot(guided, k * 50).map(|s| (k * 50, s))).collect();
        assert!(shots.len() > 20, "guided {guided}: {} locks", shots.len());
        let mut hits = 0;
        for &(range, (z, hit)) in &shots {
            let (lo, hi) = z.zone.expect("a zone");
            assert!(hi <= z.lock_max, "guided {guided}: {z:?}");
            assert!((z.range - range).abs() <= range / 10 + 2, "{z:?}");
            // Right but for the edges.
            let near_edge = (z.range - lo).abs() < 30 || (z.range - hi).abs() < 30;
            assert!(
                z.in_zone() == hit || near_edge,
                "guided {guided} range {range}: {z:?} hit {hit}"
            );
            hits += hit as i32;
        }
        assert!(hits > 5, "guided {guided}: {hits} hits");
    }
}

/// Air-to-air store `w` fired from 5000 ft at a slow An-72 `range` map units northeast, flying
/// straight on `heading` (classic guidance does not steer for a bearing of exactly north): the
/// launch zone at the shot and whether it shot the An-72 down.
fn air_shot(guided: bool, w: i32, range: i32, heading: i32) -> (dlz::LaunchZone, bool) {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let opts = SimOptions { realistic_guidance: guided, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    sim.set_training(true);
    sim.set_loadout(&[(w, 2)]);
    let diag = (range as f64 / std::f64::consts::SQRT_2).round() as i32;
    let (x, y) = (mx + diag, my - diag);
    // It flies for its waypoint far ahead, at 1000 ft above the player.
    let wp = 6;
    {
        let ahead = if heading == 0x2000 { 1 } else { -1 } * 1000;
        let o = &mut sim.objects_mut()[wp];
        (o.x, o.y) = (x + ahead, y - ahead);
    }
    {
        let u = &mut sim.flights_mut()[1];
        *u = FlightUnit {
            x,
            y,
            alt: 6000,
            heading,
            spec: 9,
            flags: 0x43,
            speed: 200,
            timer: 30000,
            wp: wp as i32,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    let mut p = player_at(mx, my, 5000, 0x2000, 300);
    p.hud_mode = HudMode::Air;
    p.cmd.toggle_bay = true;
    p.cmd.lock_unit = Some(1);
    for _ in 0..6 {
        sim.step(&p, 15);
        p.cmd = Commands::default();
    }
    let zone = sim.launch_zone().expect("launch zone");
    p.cmd.fire = true;
    let mut killed = false;
    for _ in 0..15 * 120 {
        for e in sim.step(&p, 15) {
            killed |= matches!(e, SimEvent::AircraftDestroyed { unit: 1, .. });
        }
        p.cmd = Commands::default();
        if killed {
            break;
        }
    }
    (zone, killed)
}

#[test]
fn launch_zone_predicts_air_to_air_hits() {
    for guided in [false, true] {
        for w in [0, 1] {
            // Tail chase and head-on.
            for heading in [0x2000, 0xA000u16 as i16 as i32] {
                let mut hits = 0;
                for k in 1..=30 {
                    let range = k * 100;
                    let (z, hit) = air_shot(guided, w, range, heading);
                    let (lo, hi) = z.zone.unwrap_or((i32::MAX, i32::MAX));
                    let edge = |e: i32| (z.range - e).abs() < 40 + e / 20;
                    assert!(
                        z.in_zone() == hit || edge(lo) || edge(hi),
                        "guided {guided} weapon {w} heading {heading:#x} range {range}: {z:?} \
                         hit {hit}"
                    );
                    if let Some(ne) = z.no_escape {
                        assert!(ne <= hi, "{z:?}");
                    }
                    hits += hit as i32;
                }
                assert!(hits > 3, "guided {guided} weapon {w} heading {heading:#x}: {hits}");
            }
        }
    }
}
/// The air-to-air launch zone against an An-72 circling while the player closes, every 3
/// frames: its edges move smoothly (a near miss here and there in the zone does not cut it).
#[test]
fn launch_zone_holds_steady_on_a_turning_target() {
    let d = data();
    for guided in [false, true] {
        for w in [0, 1] {
            let m = mission(d, 3, 0x1234, 1);
            let opts = SimOptions { realistic_guidance: guided, ..Default::default() };
            let mut sim = Sim::with_options(&d.sim, &m, opts);
            let (mx, my) = sandbox(&mut sim, &m);
            sim.set_training(true);
            sim.set_loadout(&[(w, 2)]);
            {
                let u = &mut sim.flights_mut()[1];
                *u = FlightUnit {
                    x: mx + 1400,
                    y: my - 1400,
                    alt: 5000,
                    heading: 0x2000,
                    spec: 9,
                    flags: 0x43,
                    speed: 200,
                    timer: 30000,
                    wp: 3,
                    ..Default::default()
                };
                u.wx = u.x << 5;
                u.wy = u.y << 5;
            }
            let mut p = player_at(mx, my, 5000, 0x2000, 300);
            p.hud_mode = HudMode::Air;
            p.cmd.toggle_bay = true;
            p.cmd.lock_unit = Some(1);
            let mut y = p.y as f64;
            let mut last: Option<dlz::LaunchZone> = None;
            for f in 0..15 * 40 {
                sim.step(&p, 15);
                p.cmd = Commands::default();
                y += 300.0 * 27.0 / 16.0 / 15.0 * 0.7;
                p.y = y as i32;
                if f % 3 != 0 || f < 10 {
                    continue;
                }
                let z = sim.launch_zone().unwrap();
                let (lo, hi) = z.zone.expect("a zone");
                if let Some((l, h)) = last.and_then(|l| l.zone) {
                    let ne = |z: &dlz::LaunchZone| z.no_escape.unwrap_or(0);
                    let jump =
                        (lo - l).abs().max((hi - h).abs()).max((ne(&z) - ne(&last.unwrap())).abs());
                    assert!(jump < 400, "guided {guided} weapon {w} frame {f}: {last:?} -> {z:?}");
                }
                last = Some(z);
            }
        }
    }
}

/// A helicopter lands anywhere flat: stopped near an insertion target (mission type 9) the
/// troops get out after `INSERT_SECONDS`; far from it nothing happens, and the flight goes on.
#[test]
fn helicopter_inserts_troops_off_base() {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let mut sim =
        Sim::with_options(&d.sim, &m, SimOptions { helicopter: true, ..Default::default() });
    let (x, y) = sandbox(&mut sim, &m);
    sim.set_training(true);
    sim.objects_mut()[5].x = x;
    sim.objects_mut()[5].y = y;
    sim.target_slots_mut()[0] = TargetSlot { mission_type: 9, object: 5, base: 0, code: 0 };
    let landed = |mx: i32, my: i32| PlayerState {
        on_ground: true,
        gear_up: false,
        velocity: 0,
        ..player_at(mx, my, 0, 0, 0)
    };
    // Fly in, then land 1.5 km away: nothing.
    for _ in 0..15 * 2 {
        sim.step(&player_at(x + 0x60, y, 300, 0, 60), 15);
    }
    for _ in 0..15 * 15 {
        sim.step(&landed(x + 0x60, y), 15);
    }
    assert!(sim.mission_end().is_none(), "{:?}", sim.mission_end());
    assert!(!sim.objectives().primary_done);
    // 300 m from the target: the troops are out after 10 s, not before.
    let mut inserted = None;
    for f in 0..15 * 15 {
        for e in sim.step(&landed(x + 0x14, y), 15) {
            if e == SimEvent::Message("Troops inserted".into()) {
                inserted = Some(f);
            }
        }
    }
    let f = inserted.expect("troops inserted");
    assert!((15 * 9..15 * 12).contains(&f), "after {f} frames");
    assert!(sim.objectives().primary_done && sim.mission_end().is_none());
}

/// The AH-1Z (else the UH-60) in a sandbox, hovering 1000 ft up heading northeast with
/// Hellfires on a pylon.
fn helicopter(gunship: bool, guided: bool) -> (Sim, i32, i32, PlayerState) {
    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let opts = SimOptions {
        external_stores: true,
        helicopter: true,
        gunship,
        realistic_guidance: guided,
        ..Default::default()
    };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    sim.set_training(true);
    sim.set_loadout(&[(20, 4)]);
    let p = player_at(mx, my, 1000, 0x2000, 0);
    (sim, mx, my, p)
}

/// A Hellfire flies to wherever the targeting pod looks: fired at one target with the pod on
/// it, it hits the other one the pod is moved to during its flight.
#[test]
fn hellfire_follows_the_pod() {
    for guided in [false, true] {
        let (mut sim, mx, my, mut p) = helicopter(true, guided);
        let (first, second) = (5, 6);
        let place = |sim: &mut Sim, t: usize, x: i32, y: i32| {
            let o = &mut sim.objects_mut()[t];
            o.x = x;
            o.y = y;
            o.flags = 0;
        };
        place(&mut sim, first, mx + 100, my - 100);
        place(&mut sim, second, mx + 110, my - 88);
        p.altitude = 500;
        let pod =
            |x: i32, y: i32| Some(PodAim { heading: 0x2000, pitch: -0x400, ground: Some((x, y)) });
        p.pod = pod(mx + 100, my - 100);
        sim.step(&p, 15);
        p.cmd.fire = true;
        sim.step(&p, 15);
        p.cmd = Commands::default();
        let slot = (8..12).find(|&s| sim.projectiles()[s].ttl != 0).expect("Hellfire fired");
        assert_eq!(sim.projectiles()[slot].target, first as i32);
        let mut at = None;
        for frame in 0..15 * 30 {
            if frame == 15 {
                p.pod = pod(mx + 110, my - 88);
            }
            for e in sim.step(&p, 15) {
                if let SimEvent::Explosion { x, y, .. } = e {
                    at.get_or_insert((x, y));
                }
            }
            if at.is_some() {
                break;
            }
        }
        assert_eq!(at, Some((mx + 110, my - 88)), "guided {guided}");
    }
}

/// A Hellfire fired from the AH-1Z 500 ft up at a target `range` map units northeast, the pod's
/// spot on it: the launch zone at the shot and whether it hit.
fn hellfire_shot(guided: bool, range: i32) -> (Option<dlz::LaunchZone>, bool) {
    let (mut sim, mx, my, mut p) = helicopter(true, guided);
    let diag = (range as f64 / std::f64::consts::SQRT_2).round() as i32;
    let (tx, ty) = (mx + diag, my - diag);
    {
        let o = &mut sim.objects_mut()[5];
        o.x = tx;
        o.y = ty;
        o.flags = 0;
    }
    p.altitude = 500;
    p.hud_mode = HudMode::Gnd;
    p.pod = Some(PodAim { heading: 0x2000, pitch: -0x100, ground: Some((tx, ty)) });
    sim.step(&p, 15);
    let zone = sim.launch_zone();
    p.cmd.fire = true;
    let mut hit = false;
    for _ in 0..15 * 150 {
        for e in sim.step(&p, 15) {
            if let SimEvent::Explosion { x, y, .. } = e {
                hit |= (x - tx).abs() <= 2 && (y - ty).abs() <= 2;
            }
        }
        p.cmd = Commands::default();
    }
    (zone, hit)
}

/// The Hellfire's launch zone is at the pod's spot, without a lock; with realistic guidance it
/// glides only so far: it slows once burnt out, stalls and falls short.
#[test]
fn hellfire_launch_zone_at_the_spot() {
    for guided in [false, true] {
        let (z, hit) = hellfire_shot(guided, 400);
        let z = z.expect("a launch zone");
        let (lo, hi) = z.zone.expect("in reach");
        assert!(z.in_zone() && hit, "guided {guided}: {z:?} hit {hit}");
        assert!(lo < 100 && hi > 600, "guided {guided}: {z:?}");
        if guided {
            assert!(hi < 1200, "{z:?}");
            let (z, hit) = hellfire_shot(guided, hi + 100);
            assert!(!z.unwrap().in_zone() && !hit, "past the zone: {z:?} hit {hit}");
            let (z, hit) = hellfire_shot(guided, hi - 60);
            assert!(z.unwrap().in_zone() && hit, "inside the zone: {z:?} hit {hit}");
        }
    }
}

/// A point designated with the targeting pod away from any object is the ground lock itself
/// (not the nearest object, which may be kilometres away): it holds while rounds hit the ground
/// elsewhere, and the Hellfire's launch zone ranges to it. Next to an object, the object.
#[test]
fn pod_designates_a_ground_point() {
    let (mut sim, mx, my, mut p) = helicopter(true, true);
    let o = 5;
    {
        let ob = &mut sim.objects_mut()[o];
        ob.x = mx + 300;
        ob.y = my - 300;
        ob.flags = 0;
    }
    p.altitude = 500;
    p.hud_mode = HudMode::Gnd;
    let (x, y) = (mx + 150, my - 150);
    p.pod = Some(PodAim { heading: 0x2000, pitch: -0x100, ground: Some((x, y)) });
    p.cmd.designate_at = Some((x, y));
    sim.step(&p, 15);
    p.cmd = Commands::default();
    for _ in 0..30 {
        let mut q = p.clone();
        q.cmd.gun = true;
        sim.step(&q, 15);
    }
    let t = sim.targeting();
    assert_eq!(t.ground, Some(0), "{t:?}");
    let g = sim.objects()[0];
    assert_eq!((g.x, g.y, g.shape), (x, y, 0));
    let z = sim.launch_zone().expect("a launch zone");
    assert!((z.range - 212).abs() <= 3 && z.in_zone(), "{z:?}");
    // Close to the object: the object.
    p.cmd.designate_at = Some((mx + 303, my - 297));
    sim.step(&p, 15);
    assert_eq!(sim.targeting().ground, Some(o));
}

/// Slewing the pod moves the launch zone's target with it (the SPI): the Hellfire's, and a
/// Maverick's without a lock; the zone is against the lock again once the pod rests on it.
#[test]
fn launch_zone_follows_the_slewed_pod() {
    let (mut sim, mx, my, mut p) = helicopter(true, true);
    p.altitude = 500;
    p.hud_mode = HudMode::Gnd;
    let pod =
        |x: i32, y: i32| Some(PodAim { heading: 0x2000, pitch: -0x100, ground: Some((x, y)) });
    p.pod = pod(mx + 150, my - 150);
    p.cmd.designate_at = Some((mx + 150, my - 150));
    sim.step(&p, 15);
    p.cmd = Commands::default();
    let z = sim.launch_zone().unwrap();
    assert!(z.at_lock && (z.range - 212).abs() <= 3, "{z:?}");
    p.pod = pod(mx + 300, my - 300);
    sim.step(&p, 15);
    let z = sim.launch_zone().unwrap();
    assert!(!z.at_lock && (z.range - 424).abs() <= 3, "{z:?}");

    let d = data();
    let m = mission(d, 3, 0x1234, 1);
    let opts = SimOptions { realistic_guidance: true, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    sim.set_training(true);
    sim.set_loadout(&[(5, 2)]);
    let mut p = player_at(mx, my, 3000, 0x2000, 450);
    p.hud_mode = HudMode::Gnd;
    p.pod = pod(mx + 200, my - 200);
    sim.step(&p, 15);
    let z = sim.launch_zone().expect("a zone against the pod's point");
    assert!(!z.at_lock && (z.range - 283).abs() <= 3, "{z:?}");
}

/// The AH-1Z's cannon fires where the pod looks, as far as its turret turns; the UH-60 has none.
#[test]
fn turret_gun_follows_the_pod() {
    let round = |pod_heading: u16, gunship: bool| {
        let (mut sim, _, _, mut p) = helicopter(gunship, false);
        p.heading = 0;
        p.pod = Some(PodAim { heading: pod_heading, pitch: -0x800, ground: None });
        p.cmd.gun = true;
        sim.step(&p, 15);
        sim.bullets().0.iter().find(|b| b.x != 0).copied()
    };
    // East and down: the round flies east (x grows), falling.
    let b = round(0x4000, true).expect("a round");
    assert!(b.vx > 0 && b.vy.abs() < b.vx / 50 && b.vz < 0, "{b:?}");
    // Straight behind: the turret stops 110 degrees left (west and a little aft, south).
    let b = round(0x8000, true).expect("a round");
    assert!(b.vx < 0 && b.vy > 0 && b.vy < -b.vx, "{b:?}");
    assert!(round(0x4000, false).is_none(), "the UH-60 has no cannon");
}

/// The AH-1Z's cannon is two weapon entries: slaved to the helmet sight, then to the pod; its
/// turret follows the one it is slaved to.
#[test]
fn gunship_gun_slaved_to_helmet_then_pod() {
    let (mut sim, _, _, mut p) = helicopter(true, false);
    p.heading = 0;
    let space = |sim: &mut Sim, p: &PlayerState| {
        let mut q = p.clone();
        q.cmd.next_bay = true;
        sim.step(&q, 15);
    };
    // Hellfires, then the gun on the helmet, then on the pod, then the Hellfires again.
    assert!(!sim.gun_selected());
    space(&mut sim, &p);
    assert!(sim.gun_selected() && !sim.gun_on_pod());
    space(&mut sim, &p);
    assert!(sim.gun_selected() && sim.gun_on_pod());
    space(&mut sim, &p);
    assert!(!sim.gun_selected() && sim.gun_on_pod(), "the turret stays on the pod");
    space(&mut sim, &p);
    assert!(sim.gun_selected() && !sim.gun_on_pod(), "selected again: on the helmet");
    // The helmet looks east, the pod west.
    p.helmet = Some(PodAim { heading: 0x4000, pitch: -0x400, ground: None });
    p.pod = Some(PodAim { heading: 0xC000, pitch: -0x400, ground: None });
    let fire = |sim: &mut Sim, p: &PlayerState| {
        let mut q = p.clone();
        q.cmd.gun = true;
        sim.step(&q, 15);
        let b = sim.bullets().0.iter().find(|b| b.x != 0).copied().expect("a round");
        q.cmd.gun = false;
        for _ in 0..30 {
            sim.step(&q, 15);
        }
        b
    };
    assert!(fire(&mut sim, &p).vx > 0, "along the helmet");
    space(&mut sim, &p);
    assert!(fire(&mut sim, &p).vx < 0, "along the pod");
    // No helmet sight (an outside view): along the pod.
    space(&mut sim, &p);
    space(&mut sim, &p);
    assert!(!sim.gun_on_pod());
    p.helmet = None;
    assert!(fire(&mut sim, &p).vx < 0);
}

#[test]
fn helmet_seeker_locks_in_its_circle() {
    let d = data();
    let m = mission(d, 3, 0x1234, 2);
    let opts = SimOptions { helicopter: true, gunship: true, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    // Two An-72s: 3 km north, and 2 km away 30 degrees right of north.
    for (k, (dx, dy)) in [(0, -192), (64, -111)].into_iter().enumerate() {
        let u = &mut sim.flights_mut()[k + 1];
        *u = FlightUnit {
            x: mx + dx,
            y: my + dy,
            alt: 5000,
            spec: 9,
            flags: 0x43,
            speed: 200,
            timer: 30000,
            wp: 3,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    sim.set_bay(0, 0, 4);
    sim.select_bay(0);
    let mut p = player_at(mx, my, 5000, 0, 0);
    p.hud_mode = HudMode::Air;
    let mut look = |sim: &mut Sim, heading: u16| {
        p.helmet = Some(PodAim { heading, pitch: 0, ground: None });
        sim.step(&p, 15);
        sim.targeting()
    };
    // Looking north: the one ahead, the nearer one outside the circle.
    let t = look(&mut sim, 0);
    assert!(sim.helmet_seeker().is_some());
    assert_eq!(t.air, Some(1));
    // Towards the other: the first leaves the circle, the seeker takes the second.
    assert_eq!(look(&mut sim, 0x1555).air, Some(2));
    // Nothing in the circle: no lock.
    let t = look(&mut sim, 0x4000);
    assert_eq!(t.air, None);
    assert!(!t.lock_valid);
}

#[test]
fn helmet_seeker_stops_at_its_gimbals() {
    let d = data();
    let m = mission(d, 3, 0x1234, 2);
    let opts = SimOptions { helicopter: true, gunship: true, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    // An An-72 3 km off, 70 degrees right of the nose.
    {
        let u = &mut sim.flights_mut()[1];
        *u = FlightUnit {
            x: mx + 180,
            y: my - 66,
            alt: 5000,
            spec: 9,
            flags: 0x43,
            speed: 200,
            timer: 30000,
            wp: 3,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    sim.set_bay(0, 0, 4);
    sim.select_bay(0);
    let mut p = player_at(mx, my, 5000, 0, 0);
    p.hud_mode = HudMode::Air;
    // Looking at it: the seeker stops at its gimbal limit off the nose and sees nothing.
    let heading = (70.0f64 / 360.0 * 65536.0) as u16;
    p.helmet = Some(PodAim { heading, pitch: 0, ground: None });
    sim.step(&p, 15);
    let los = sim.helmet_seeker().expect("seeker");
    assert!((los[1].acos() - SEEKER_GIMBAL).abs() < 1e-3, "{los:?}");
    assert_eq!(sim.targeting().air, None);
}

#[test]
fn helmet_seeker_locks_the_one_it_looks_at() {
    let d = data();
    let m = mission(d, 3, 0x1234, 2);
    let opts = SimOptions { helicopter: true, gunship: true, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    // An An-72 2.3 km ahead, 4 degrees right, and another 4.7 km straight ahead behind it: both
    // in the circle.
    for (k, (dx, dy)) in [(10, -150), (0, -300)].into_iter().enumerate() {
        let u = &mut sim.flights_mut()[k + 1];
        *u = FlightUnit {
            x: mx + dx,
            y: my + dy,
            alt: 5000,
            spec: 9,
            flags: 0x43,
            speed: 200,
            timer: 30000,
            wp: 3,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    sim.set_bay(0, 0, 4);
    sim.select_bay(0);
    let mut p = player_at(mx, my, 5000, 0, 0);
    p.hud_mode = HudMode::Air;
    p.helmet = Some(PodAim { heading: 0, pitch: 0, ground: None });
    for _ in 0..3 {
        sim.step(&p, 15);
        assert_eq!(sim.targeting().air, Some(2));
    }
}

#[test]
fn helmet_seeker_locks_any_aircraft() {
    let d = data();
    let m = mission(d, 3, 0x1234, 2);
    let opts = SimOptions { helicopter: true, gunship: true, ..Default::default() };
    let mut sim = Sim::with_options(&d.sim, &m, opts);
    let (mx, my) = sandbox(&mut sim, &m);
    // An aircraft of a type the nearest-target key passes over, 3 km ahead.
    let spec = (0..sim.aircraft_types.len() as i32)
        .find(|&s| sim.friendly_aircraft(s))
        .expect("a friendly type");
    {
        let u = &mut sim.flights_mut()[1];
        *u = FlightUnit {
            x: mx,
            y: my - 192,
            alt: 5000,
            spec,
            flags: 0x43,
            speed: 200,
            timer: 30000,
            wp: 3,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    sim.set_bay(0, 0, 4);
    sim.select_bay(0);
    let mut p = player_at(mx, my, 5000, 0, 0);
    p.hud_mode = HudMode::Air;
    p.helmet = Some(PodAim { heading: 0, pitch: 0, ground: None });
    sim.step(&p, 15);
    assert_eq!(sim.targeting().air, Some(1));
}
