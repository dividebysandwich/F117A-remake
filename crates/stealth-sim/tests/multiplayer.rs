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
fn the_steps_go_on_with_nobody_flying() {
    let (mut sim, _) = two_pilots();
    sim.end_sortie_pilot(0, 7);
    sim.end_sortie_pilot(1, 7);
    let before = sim.step_count();
    for _ in 0..10 {
        sim.step_pilots(&[], 15);
    }
    assert_eq!(sim.step_count(), before + 10, "a server sends the world by its steps");
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
fn a_pilot_locks_another_while_he_flies() {
    let (mut sim, base) = two_pilots();
    let b = sim.objects()[base];
    let mut inputs = vec![
        (0, flying(b.x, b.y - 0x400, 3000, 0, 300)),
        (1, flying(b.x + 0x40, b.y - 0x400, 3000, 0, 300)),
    ];
    sim.step_pilots(&inputs, 15);
    inputs[0].1.cmd.lock_pilot = Some(1);
    sim.step_pilots(&inputs, 15);
    inputs[0].1.cmd = Commands::default();
    assert_eq!(sim.targeting().pilot, Some(1));
    assert_eq!(sim.targeting().air, None);
    sim.step_pilots(&inputs, 15);
    assert_eq!(sim.targeting().pilot, Some(1), "the lock holds");
    // He lands: the lock is gone.
    inputs[1].1 = PlayerState { on_ground: true, ..sim.start_state_at(base) };
    sim.step_pilots(&inputs, 15);
    assert_eq!(sim.targeting().pilot, None);
    // No lock on oneself.
    inputs[0].1.cmd.lock_pilot = Some(0);
    sim.step_pilots(&inputs, 15);
    assert_eq!(sim.targeting().pilot, None);
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
        mirror.apply_pilot_tick(&host.pilot_tick(1));
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

/// Pilot 1 (his client flies his rounds) fires his cannon (or not) behind an An-72 3 km
/// ahead (or `far` away); then `claim` from where the unit was at each step (step, wx, wy,
/// alt; the last is now).
fn claimed_kill(
    far: bool,
    fire: bool,
    claim: impl Fn(&[(u32, i32, i32, i32)]) -> claims::GunHit,
) -> Option<u8> {
    let (mut sim, base) = two_pilots();
    let b = sim.objects()[base];
    let (mx, my) = (b.x + 0x1000, b.y - 0x800);
    {
        let u = &mut sim.flights_mut()[1];
        let ahead = if far { 64 * 60 } else { 64 * 3 };
        *u = FlightUnit {
            x: mx,
            y: my - ahead,
            alt: 5000,
            spec: 9,
            flags: 0x43,
            speed: 400,
            timer: 30000,
            wp: 3,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    sim.set_client_rounds(1, true);
    let st = sim.start_state_at(base);
    let mut inputs =
        vec![(0, PlayerState { on_ground: true, ..st }), (1, flying(mx, my, 5000, 0, 300))];
    inputs[1].1.cmd.gun = fire;
    let mut track = Vec::new();
    for _ in 0..20 {
        sim.step_pilots(&inputs, 15);
        let u = sim.flights()[1];
        track.push((sim.step_count(), u.wx, u.wy, u.alt));
    }
    let mut by = None;
    sim.claim_hit(1, claim(&track));
    for e in sim.step_pilots(&inputs, 15) {
        if let SimEvent::AircraftDestroyed { unit: 1, by: b, .. } = e.event {
            by = b;
        }
    }
    by
}

/// The unit as it was `back` steps ago, the round `off` world units east of it.
fn seen(track: &[(u32, i32, i32, i32)], back: usize, off: i32) -> claims::GunHit {
    let (step, x, y, alt) = track[track.len() - 1 - back];
    claims::GunHit::Aircraft { unit: 1, step, at: (x + off, y, alt) }
}

#[test]
fn a_reported_hit_is_the_shooters_kill() {
    assert_eq!(claimed_kill(false, true, |t| seen(t, 0, 0)), Some(1));
    // Lag compensated: where it was on the client's screen a second ago.
    assert_eq!(claimed_kill(false, true, |t| seen(t, 15, 0)), Some(1));
    // (That is not where it is now.)
    let then_as_now = |t: &[(u32, i32, i32, i32)]| {
        let claims::GunHit::Aircraft { at, .. } = seen(t, 15, 0) else { unreachable!() };
        claims::GunHit::Aircraft { unit: 1, step: t[t.len() - 1].0, at }
    };
    assert_eq!(claimed_kill(false, true, then_as_now), None);
}

#[test]
fn doubtful_reports_are_turned_down() {
    // Not firing, out of the cannon's reach, far from where it was then.
    assert_eq!(claimed_kill(false, false, |t| seen(t, 0, 0)), None);
    assert_eq!(claimed_kill(true, true, |t| seen(t, 0, 0)), None);
    assert_eq!(claimed_kill(false, true, |t| seen(t, 0, 2000)), None);
    // Where it is now, but as of long ago (no record of it then).
    assert_eq!(
        claimed_kill(false, true, |t| {
            let (step, x, y, alt) = t[t.len() - 1];
            claims::GunHit::Aircraft { unit: 1, step: step.wrapping_sub(100), at: (x, y, alt) }
        }),
        None
    );
}

#[test]
fn a_client_flown_pilots_host_rounds_do_not_hit() {
    // Firing straight at it, no report: no kill.
    let (mut sim, base) = two_pilots();
    let b = sim.objects()[base];
    let (mx, my) = (b.x + 0x1000, b.y - 0x800);
    {
        let u = &mut sim.flights_mut()[1];
        *u = FlightUnit {
            x: mx,
            y: my - 6,
            alt: 5000,
            spec: 9,
            flags: 0x43,
            wp: 3,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    sim.set_client_rounds(1, true);
    let st = sim.start_state_at(base);
    let mut inputs =
        vec![(0, PlayerState { on_ground: true, ..st }), (1, flying(mx, my, 5000, 0, 300))];
    inputs[1].1.cmd.gun = true;
    for _ in 0..30 {
        for e in sim.step_pilots(&inputs, 15) {
            assert!(!matches!(e.event, SimEvent::AircraftDestroyed { unit: 1, .. }));
        }
    }
}

#[test]
fn the_client_reports_what_its_rounds_hit_and_the_host_agrees() {
    let (mut host, base) = two_pilots();
    let b = host.objects()[base];
    let (mx, my) = (b.x + 0x1000, b.y - 0x800);
    {
        let u = &mut host.flights_mut()[1];
        *u = FlightUnit {
            x: mx,
            y: my - 12,
            alt: 5000,
            spec: 9,
            flags: 0x43,
            wp: 3,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
    }
    host.set_client_rounds(1, true);
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
    let st = host.start_state_at(base);
    let mut inputs =
        vec![(0, PlayerState { on_ground: true, ..st }), (1, flying(mx, my, 5000, 0, 300))];
    inputs[1].1.cmd.gun = true;
    let mut by = None;
    for _ in 0..30 {
        for e in host.step_pilots(&inputs, 15) {
            if let SimEvent::AircraftDestroyed { unit: 1, by: b, .. } = e.event {
                by = b;
            }
        }
        mirror.apply_world_fast(&host.world_fast());
        mirror.set_host_step(host.step_count());
        mirror.apply_pilot_snap(&host.pilot_snap(1));
        mirror.set_local_player(&inputs[1].1);
        for hit in mirror.fly_own_rounds(&inputs[1].1) {
            host.claim_hit(1, hit);
        }
    }
    assert_eq!(by, Some(1), "the client's report is his kill");
}

#[test]
fn a_crashed_pilot_leaves_a_burning_wreck_for_everyone() {
    let (mut sim, base) = two_pilots();
    let b = sim.objects()[base];
    let inputs = vec![
        (0, flying(b.x, b.y - 0x800, 4000, 0, 400)),
        (1, flying(b.x + 0x400, b.y - 0x800, 100, 0, 300)),
    ];
    sim.step_pilots(&inputs, 15);
    // His flight model hit the ground (the host hears it between steps).
    sim.end_sortie_pilot(1, 1);
    let events = sim.step_pilots(&inputs[..1], 15);
    let blast = events.iter().find_map(|e| match e.event {
        SimEvent::Explosion { x, y, alt: 0, kind: -16 } => Some((x, y)),
        _ => None,
    });
    assert_eq!(blast, Some((b.x + 0x400, b.y - 0x800)));
    // Ending a sortie from the menu leaves nothing.
    sim.end_sortie_pilot(0, 7);
    let events = sim.step_pilots(&[], 15);
    assert!(!events.iter().any(|e| matches!(e.event, SimEvent::Explosion { .. })));
}

/// Pilot 1 behind pilot 0 (`ahead` map units), both flying north: pilot 1 locks him and fires
/// an air-to-air missile (or holds the cannon's trigger, his `client` flying his rounds) under
/// friendly fire `ff`; the bursts at pilot 0 and his hits.
fn shot_at_pilot(ff: bool, guided: bool, gun: bool, client: bool, ahead: i32) -> (usize, usize) {
    let (mut sim, base) = two_pilots();
    sim.opts.friendly_fire = ff;
    sim.opts.realistic_guidance = guided;
    let b = sim.objects()[base];
    let (mx, my) = (b.x + 0x1000, b.y - 0x800);
    sim.set_pilot_loadout(1, &[(0, 4), (-1, 0), (-1, 0), (-1, 0)], 0);
    sim.set_client_rounds(1, client);
    let mut inputs =
        vec![(0, flying(mx, my - ahead, 5000, 0, 200)), (1, flying(mx, my, 5000, 0, 300))];
    inputs[1].1.cmd.toggle_bay = !gun;
    inputs[1].1.cmd.lock_pilot = Some(0);
    sim.step_pilots(&inputs, 15);
    inputs[1].1.cmd = Commands::default();
    assert_eq!(sim.as_pilot(1, |s| s.targeting().pilot), Some(0));
    for _ in 0..5 {
        sim.step_pilots(&inputs, 15);
    }
    let mut mirror = None;
    if client {
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
        let mut sm =
            Sim::with_options(&d.sim, &m, SimOptions { campaign: true, ..Default::default() });
        sm.make_mirror(1);
        mirror = Some(sm);
    }
    if gun {
        inputs[1].1.cmd.gun = true;
    } else {
        inputs[1].1.cmd.fire = true;
    }
    let (mut bursts, mut hits) = (0, 0);
    for i in 0..15 * 20 {
        for e in sim.step_pilots(&inputs, 15) {
            match e.event {
                SimEvent::Explosion { x, y, kind, .. }
                    if (kind == 0x10 || kind == -1)
                        && (x - mx).abs() <= 2
                        && (y - (my - ahead)).abs() <= 2 =>
                {
                    bursts += 1
                }
                SimEvent::PlayerHit { .. } => {
                    assert_eq!(e.pilot, 0, "the hit is pilot 0's news");
                    hits += 1
                }
                _ => {}
            }
        }
        if !gun && i == 0 {
            inputs[1].1.cmd = Commands::default();
        }
        if gun && i == 15 {
            inputs[1].1.cmd.gun = false;
        }
        if let Some(m) = mirror.as_mut() {
            m.apply_world_fast(&sim.world_fast());
            m.set_host_step(sim.step_count());
            m.apply_pilot_snap(&sim.pilot_snap(1));
            m.set_local_player(&inputs[1].1);
            for hit in m.fly_own_rounds(&inputs[1].1) {
                sim.claim_hit(1, hit);
            }
        }
    }
    (bursts, hits)
}

#[test]
fn a_missile_fired_at_a_locked_pilot_hits_him() {
    for guided in [false, true] {
        let (bursts, hits) = shot_at_pilot(true, guided, false, false, 64 * 3);
        assert_eq!((bursts, hits), (1, 1), "guided {guided}");
        // Without friendly fire it bursts on him harmlessly.
        let (bursts, hits) = shot_at_pilot(false, guided, false, false, 64 * 3);
        assert_eq!((bursts, hits), (1, 0), "guided {guided}");
    }
}

#[test]
fn a_missile_on_a_pilot_lights_his_warnings() {
    let (mut sim, base) = two_pilots();
    let b = sim.objects()[base];
    let (mx, my) = (b.x + 0x1000, b.y - 0x800);
    sim.set_pilot_loadout(1, &[(0, 4), (-1, 0), (-1, 0), (-1, 0)], 0);
    let mut inputs =
        vec![(0, flying(mx, my - 64 * 3, 5000, 0, 200)), (1, flying(mx, my, 5000, 0, 300))];
    inputs[1].1.cmd.toggle_bay = true;
    inputs[1].1.cmd.lock_pilot = Some(0);
    sim.step_pilots(&inputs, 15);
    inputs[1].1.cmd = Commands::default();
    for _ in 0..5 {
        sim.step_pilots(&inputs, 15);
    }
    inputs[1].1.cmd.fire = true;
    sim.step_pilots(&inputs, 15);
    inputs[1].1.cmd = Commands::default();
    let mut warned = false;
    for _ in 0..15 {
        sim.step_pilots(&inputs, 15);
        warned |= sim.lights().rad_threat;
    }
    assert!(warned, "pilot 0 sees it coming");
}

#[test]
fn a_pilots_cannon_hits_another_pilot() {
    let (bursts, hits) = shot_at_pilot(true, false, true, false, 6);
    assert!(bursts > 0 && hits > 0, "{bursts} {hits}");
    // At most every quarter second: not every round that hits.
    assert!(hits < bursts, "{bursts} {hits}");
    let (bursts, hits) = shot_at_pilot(false, false, true, false, 6);
    assert!(bursts > 0 && hits == 0, "{bursts} {hits}");
}

#[test]
fn a_client_reports_his_rounds_hitting_another_pilot() {
    let (bursts, hits) = shot_at_pilot(true, false, true, true, 6);
    assert!(bursts > 0 && hits > 0, "{bursts} {hits}");
    let (_, hits) = shot_at_pilot(false, false, true, true, 6);
    assert_eq!(hits, 0);
}

#[test]
fn a_forged_report_on_a_pilot_is_turned_down() {
    let (mut sim, base) = two_pilots();
    sim.opts.friendly_fire = true;
    let b = sim.objects()[base];
    let (mx, my) = (b.x + 0x1000, b.y - 0x800);
    sim.set_client_rounds(1, true);
    let mut inputs = vec![(0, flying(mx, my - 6, 5000, 0, 300)), (1, flying(mx, my, 5000, 0, 300))];
    inputs[1].1.cmd.gun = true;
    for _ in 0..10 {
        sim.step_pilots(&inputs, 15);
    }
    let v = sim.pilot_view(0).unwrap();
    let step = sim.step_count();
    let at = |off: i32| (v.wx + off, 0x100000 - v.wy, v.view_z);
    sim.claim_hit(1, claims::GunHit::Pilot { id: 0, step, at: at(2000) });
    let hit =
        |ev: Vec<PilotEvent>| ev.iter().any(|e| matches!(e.event, SimEvent::PlayerHit { .. }));
    assert!(!hit(sim.step_pilots(&inputs, 15)), "far from where he was");
    sim.claim_hit(1, claims::GunHit::Pilot { id: 0, step: sim.step_count(), at: at(0) });
    assert!(hit(sim.step_pilots(&inputs, 15)), "where he is");
}
