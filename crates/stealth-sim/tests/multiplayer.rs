//! Several pilots in one campaign simulation (coop).

use std::sync::OnceLock;

use stealth_mission::{GameData, MissionKind, MissionSettings, Seed};
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

/// A quiet Central Europe campaign (no radars, no guns) with two pilots on a friendly base.
fn two_pilots() -> (Sim, usize) {
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
    let mut sim =
        Sim::with_options(&d.sim, &m, SimOptions { campaign: true, ..Default::default() });
    for o in sim.objects_mut().iter_mut() {
        o.unit_type = 0;
    }
    sim.aaa_units_mut().clear();
    let base = (3..sim.objects().len())
        .find(|&i| sim.objects()[i].flags & 0xF0F == 0x401)
        .expect("friendly base");
    sim.add_pilot(1);
    sim.respawn_pilot(0, base, None, false, false);
    sim.respawn_pilot(1, base, None, false, false);
    (sim, base)
}

fn flying(mx: i32, my: i32, alt: u16, heading: u16, knots: u16) -> PlayerState {
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

/// Pilot 0 waits on the runway, pilot 1 flies high and slow with the bay open past an SA-2
/// far away: the site goes for pilot 1.
fn site_engages_pilot_one() -> (Sim, Vec<(u8, PlayerState)>, usize) {
    let (mut sim, base) = two_pilots();
    let b = sim.objects()[base];
    let (sx, sy) = (b.x + 0x1800, b.y);
    {
        let o = &mut sim.objects_mut()[3];
        o.unit_type = 1;
        o.x = sx;
        o.y = sy;
        o.flags = 0;
        o.alert = 0;
        o.timer = 0;
    }
    let st = sim.start_state_at(base);
    let ground = PlayerState { on_ground: true, ..st };
    let mut air = flying(sx, sy + 640, 6000, 0x4000, 200);
    air.cmd.toggle_bay = true;
    let mut inputs = vec![(0, ground), (1, air)];
    for _ in 0..15 * 300 {
        let ev = sim.step_pilots(&inputs, 15);
        inputs[1].1.cmd = Commands::default();
        if let Some((slot, pilot)) = ev.iter().find_map(|e| match e.event {
            SimEvent::MissileLaunched { slot, launcher: Launcher::Site(3), .. } => {
                Some((slot, e.pilot))
            }
            _ => None,
        }) {
            assert_eq!(pilot, 1, "the launch is pilot 1's news");
            return (sim, inputs, slot);
        }
    }
    panic!("the SA-2 never fired");
}

#[test]
fn a_site_goes_for_the_pilot_it_sees() {
    let (sim, _, slot) = site_engages_pilot_one();
    assert_eq!(sim.enemy_slot_target(slot), 1);
    // Pilot 0's warning receiver shows nothing of it.
    assert!(sim.emitters().iter().all(|e| !e.launch && !e.tracking), "{:?}", sim.emitters());
}

#[test]
fn private_events_reach_their_pilot_only() {
    let (mut sim, mut inputs, _) = site_engages_pilot_one();
    let mut warned = [false; 2];
    for _ in 0..15 * 10 {
        for e in sim.step_pilots(&inputs, 15) {
            if matches!(e.event, SimEvent::Rwr(_)) {
                warned[e.pilot as usize] = true;
            }
        }
        inputs[1].1.cmd = Commands::default();
    }
    assert_eq!(warned, [false, true]);
}

#[test]
fn one_pilot_leaving_keeps_the_others_threats() {
    let (mut sim, _, slot) = site_engages_pilot_one();
    assert!(sim.projectiles()[slot].ttl != 0);
    sim.end_sortie_pilot(0, 7);
    assert!(sim.pilot_absent(0) && !sim.pilot_absent(1));
    assert!(sim.projectiles()[slot].ttl != 0, "pilot 1's missile is still coming");
    sim.end_sortie_pilot(1, 7);
    assert!(sim.projectiles()[slot].ttl == 0);
}

#[test]
fn a_kill_is_the_shooters() {
    let (mut sim, base) = two_pilots();
    let b = sim.objects()[base];
    let (mx, my) = (b.x + 0x1000, b.y - 0x800);
    // A slow An-72 3 km ahead of pilot 1, flying north like him.
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
    sim.set_pilot_loadout(1, &[(0, 4), (-1, 0), (-1, 0), (-1, 0)], 0);
    let st = sim.start_state_at(base);
    let mut inputs =
        vec![(0, PlayerState { on_ground: true, ..st }), (1, flying(mx, my, 5000, 0, 300))];
    inputs[1].1.cmd.toggle_bay = true;
    sim.step_pilots(&inputs, 15);
    inputs[1].1.cmd = Commands::default();
    for _ in 0..5 {
        sim.step_pilots(&inputs, 15);
    }
    inputs[1].1.cmd.fire = true;
    sim.step_pilots(&inputs, 15);
    inputs[1].1.cmd = Commands::default();
    let mut by = None;
    for _ in 0..15 * 20 {
        for e in sim.step_pilots(&inputs, 15) {
            if let SimEvent::AircraftDestroyed { unit: 1, by: b, .. } = e.event {
                by = Some(b);
            }
        }
    }
    assert_eq!(by, Some(Some(1)));
}

#[test]
fn pilots_keep_their_own_state() {
    let (mut sim, base) = two_pilots();
    let st = sim.start_state_at(base);
    let b = sim.objects()[base];
    let inputs = vec![
        (0, PlayerState { on_ground: true, ..st }),
        (1, flying(b.x, b.y - 0x400, 3000, 0, 300)),
    ];
    sim.set_pilot_loadout(1, &[(0, 2), (-1, 0), (-1, 0), (-1, 0)], 0);
    for _ in 0..30 {
        sim.step_pilots(&inputs, 15);
    }
    assert_eq!(sim.active_pilot(), 0);
    assert!(sim.player().alt < 100, "the accessors show pilot 0");
    assert_eq!(sim.pilot_view(1).map(|p| p.alt), Some(3000));
    assert_ne!(sim.bays().0.first(), Some(&(0, 2)), "pilot 1's stores are his");
}

#[test]
fn a_mirror_follows_the_host() {
    let (mut host, mut inputs, _) = site_engages_pilot_one();
    host.record_shells(true);
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
    let mut mirror =
        Sim::with_options(&d.sim, &m, SimOptions { campaign: true, ..Default::default() });
    mirror.make_mirror(1);
    let mut base = stealth_sim::net::SlowBase::default();
    for f in 0..15 * 20 {
        if f == 30 {
            inputs[1].1.cmd.flare = true;
        }
        host.step_pilots(&inputs, 15);
        inputs[1].1.cmd = Commands::default();
        let mut slow = host.world_slow(&mut base);
        (slow.shells, slow.ai_shells) = host.take_new_shells();
        let fast: stealth_sim::net::WorldFast =
            serde_json::from_str(&serde_json::to_string(&host.world_fast()).unwrap()).unwrap();
        let slow: stealth_sim::net::WorldSlow =
            serde_json::from_str(&serde_json::to_string(&slow).unwrap()).unwrap();
        let mine: stealth_sim::net::PilotSnap =
            serde_json::from_str(&serde_json::to_string(&host.pilot_snap(1)).unwrap()).unwrap();
        mirror.set_local_player(&inputs[1].1);
        mirror.apply_world_slow(&slow);
        mirror.apply_world_fast(&fast);
        mirror.apply_pilot_snap(&mine);
        mirror.mirror_frame();
        let (emitters, lights, targeting, damage, stores) = host.as_pilot(1, |s| {
            (s.emitters(), s.lights(), s.targeting(), s.damage_effects(), s.stores())
        });
        assert_eq!(mirror.emitters(), emitters, "frame {f}");
        assert_eq!(mirror.lights(), lights);
        assert_eq!(mirror.targeting(), targeting);
        assert_eq!(mirror.damage_effects(), damage);
        assert_eq!(mirror.stores(), stores);
        assert_eq!(mirror.flights(), host.flights());
        assert_eq!(mirror.objects()[1..], host.objects()[1..]);
        let enemy = |s: &Sim| -> Vec<_> {
            s.active_projectiles().into_iter().filter(|p| p.0 < 8).collect()
        };
        assert_eq!(enemy(&mirror), enemy(&host));
        assert_eq!(mirror.countermeasures(), &host.as_pilot(1, |s| *s.countermeasures()));
        assert_eq!(mirror.aaa_units().len(), host.aaa_units().len());
        let others = mirror.remote_pilots();
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].id, 0);
    }
    assert!(mirror.stores().0 < 12, "the flare went");
}

#[test]
fn a_mirror_flies_the_other_pilots_rounds_from_their_trigger() {
    let (mut host, base) = two_pilots();
    let b = host.objects()[base];
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
    let mut mirror =
        Sim::with_options(&d.sim, &m, SimOptions { campaign: true, ..Default::default() });
    mirror.make_mirror(1);
    let mut inputs = vec![
        (0, flying(b.x, b.y - 0x800, 4000, 0, 400)),
        (1, flying(b.x + 0x400, b.y - 0x800, 4000, 0, 400)),
    ];
    inputs[0].1.cmd.gun = true;
    for _ in 0..10 {
        host.step_pilots(&inputs, 15);
        mirror.apply_world_fast(&host.world_fast());
        mirror.mirror_frame();
    }
    // Pilot 0's rounds on the host (pilot 1's view of them: the host has pilot 0 active).
    let (bullets, n) = host.bullets();
    let theirs: Vec<Bullet> = bullets[..n].iter().copied().filter(|b| b.x != 0).collect();
    let seen = mirror.other_rounds();
    assert!(!theirs.is_empty() && seen.len() == theirs.len(), "{} {}", seen.len(), theirs.len());
    // Flown from the same aircraft one frame later: within a frame's travel of the host's.
    let lead = |v: &[Bullet]| *v.iter().max_by_key(|b| b.y.abs_diff(v[0].y)).unwrap();
    let (a, b) = (lead(&theirs), lead(&seen));
    assert!((a.x - b.x).abs() + (a.y - b.y).abs() < 4 * a.vy.abs().max(a.vx.abs()), "{a:?} {b:?}");
    assert!(host.as_pilot(1, |s| s.other_rounds()).len() == theirs.len(), "the host's view");
}
