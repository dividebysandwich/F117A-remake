//! The campaign host without the renderer, on the shipped game data.

use stealth_campaign::{Aircraft, CampaignSetup, PlayerId};
use stealth_host::{Host, HostEvent, Rules, theater};
use stealth_sim::PlayerState;

fn setup(code: &str) -> CampaignSetup {
    CampaignSetup {
        theater: theater::theater_index(code).unwrap(),
        theater_code: code.into(),
        roe: 1,
        opponents: 1,
        seed: 1234,
        sky_ratio: 24.0,
        start_hour: 8.0,
        dynamic_weather: true,
    }
}

/// Two players fly sorties for a while; the war goes on.
fn two_players(code: &str) {
    let s = setup(code);
    let data = theater::game_data().unwrap();
    let sim_data = theater::sim_data(s.theater, code).unwrap();
    let rules = Rules { continuous_time: true, ..Default::default() };
    let mut host = Host::new(s, &data, rules).unwrap();
    let mut sim = host.new_sim(&sim_data);
    host.begin(&mut sim);
    host.tick(&mut sim, 0.1, &[]);
    let me = PlayerId(0);
    let wing = host.join(&mut sim, "Wingman", |_| true).unwrap();
    assert_eq!(wing, PlayerId(1));
    assert_eq!(
        host.join(&mut sim, "wingman", |_| true).unwrap(),
        wing,
        "the same name is the same player"
    );
    let flown: Vec<u32> =
        host.state.board.iter().filter(|o| !o.kind.ai_flown()).map(|o| o.id).collect();
    assert!(flown.len() >= 2, "{:?}", host.state.board);
    let mut inputs: Vec<(u8, PlayerState)> = Vec::new();
    for (p, &offer, aircraft) in [(me, &flown[0], Aircraft::F117), (wing, &flown[1], Aircraft::F16)]
    {
        let ev = host.take_mission(&mut sim, p, offer, None, aircraft, None).unwrap();
        let start = ev
            .iter()
            .find_map(|e| match e {
                HostEvent::SortieStarted { player, start, .. } if *player == p => {
                    Some(start.clone())
                }
                _ => None,
            })
            .expect("sortie started");
        inputs.push((p.0, start));
    }
    assert!(!sim.pilot_absent(0) && !sim.pilot_absent(1));
    for _ in 0..15 * 60 {
        let ev = sim.step_pilots(&inputs, 15);
        let mut ev2 = host.take_sim_events(&mut sim);
        ev2.extend(ev);
        host.tick(&mut sim, 1.0 / 15.0, &ev2);
    }
    assert!(host.state.clock > 59.0);
    let ev = host.end_sortie(&mut sim, wing, 7);
    assert!(
        ev.iter().any(|e| matches!(e, HostEvent::SortieFinished { player, .. } if *player == wing)),
        "{ev:?}"
    );
    assert!(sim.pilot_absent(1) && !sim.pilot_absent(0));
}

#[test]
fn two_players_in_central_europe() {
    two_players("ce");
}

#[test]
fn a_remake_theater_without_the_renderer() {
    two_players("ua");
}

#[test]
fn the_server_sees_by_the_moon() {
    // Noon is day, midnight dark (the light of a moon at most).
    let lat = stealth_host::sky::latitude("ce");
    assert_eq!(stealth_host::sky::visual_light(7000.0, 12.0, lat), 255);
    assert!(stealth_host::sky::visual_light(7000.0, 0.0, lat) <= 85);
}

/// The friendly side's air defences stand from the start: Patriots in the rear, Gepards at the
/// airfields.
#[test]
fn friendly_air_defences_stand_from_the_start() {
    use stealth_sim::aaa::AaaKind;
    for code in ["ce", "ua"] {
        let s = setup(code);
        let data = theater::game_data().unwrap();
        let sim_data = theater::sim_data(s.theater, code).unwrap();
        let mut host = Host::new(s, &data, Rules::default()).unwrap();
        let mut sim = host.new_sim(&sim_data);
        host.begin(&mut sim);
        let blue = |sim: &stealth_sim::Sim, kind| {
            (0..sim.aaa_units().len())
                .filter(|&k| sim.aaa_units()[k].kind == kind && sim.aaa_blue(k))
                .count()
        };
        let batteries = host.state.patriot_sites().len();
        assert!(batteries > 0, "{code}");
        assert_eq!(blue(&sim, AaaKind::PatriotRadar), batteries, "{code}");
        assert_eq!(blue(&sim, AaaKind::PatriotLauncher), 3 * batteries, "{code}");
        assert!(blue(&sim, AaaKind::Gepard) > 0, "{code}");
        assert_eq!(blue(&sim, AaaKind::Zsu), 0, "{code}");
        // No two batteries in one place, on a second begin neither.
        host.begin(&mut sim);
        assert_eq!(blue(&sim, AaaKind::PatriotRadar), batteries, "{code}");
    }
}

/// The enemy's offensive played out in the simulation: its raid and its insertion end one way
/// or the other (the strike aircraft bomb or are stopped, the Hinds land or are shot down).
#[test]
fn the_enemy_offensive_plays_out() {
    use stealth_campaign::{InsertionState, RaidState, Side, tuning};
    let s = CampaignSetup { opponents: 3, ..setup("ce") };
    let data = theater::game_data().unwrap();
    let sim_data = theater::sim_data(s.theater, "ce").unwrap();
    let mut host = Host::new(s, &data, Rules::default()).unwrap();
    let mut sim = host.new_sim(&sim_data);
    host.begin(&mut sim);
    // Both due at once.
    host.state.raid_at = -tuning::RAID_EVERY[3];
    host.state.enemy_insert_at = -tuning::ENEMY_INSERT_EVERY[3];
    for _ in 0..15 * 60 * 60 {
        let e = sim.step_pilots(&[], 15);
        let mut e2 = host.take_sim_events(&mut sim);
        e2.extend(e);
        host.tick(&mut sim, 1.0 / 15.0, &e2);
        let raid = host.state.raids.first().map(|r| r.state);
        let ins = host.state.insertions.iter().find(|i| i.side == Side::Red).map(|i| i.state);
        let done = |s: Option<bool>| s == Some(true);
        if done(raid.map(|r| matches!(r, RaidState::Hit | RaidState::Repelled)))
            && done(ins.map(|i| matches!(i, InsertionState::Captured | InsertionState::Failed)))
        {
            return;
        }
    }
    panic!(
        "not over in an hour: raids {:?}, insertions {:?}",
        host.state.raids,
        host.state.insertions.iter().map(|i| (i.side, i.state)).collect::<Vec<_>>()
    );
}

/// A campaign keeps the rules it started with, whoever serves it later.
#[test]
fn a_campaign_keeps_its_rules() {
    let data = theater::game_data().unwrap();
    let hard = Rules { realistic_damage: true, friendly_fire: true, ..Default::default() };
    let host = Host::new(setup("ce"), &data, hard).unwrap();
    let saved: stealth_campaign::CampaignState =
        serde_json::from_str(&serde_json::to_string(&host.state).unwrap()).unwrap();
    let again = Host::resume(saved, &data, Rules::default()).unwrap();
    assert_eq!(again.rules, hard);
    // An older save takes the server's.
    let mut old = host.state.clone();
    old.rules = None;
    let easy = Rules { no_crashes: true, ..Default::default() };
    assert_eq!(Host::resume(old, &data, easy).unwrap().rules, easy);
}

/// A drone wave over Ukraine played out in the simulation: the drones dive into their target
/// or are shot down.
#[test]
fn a_drone_wave_plays_out() {
    use stealth_campaign::{RaidState, tuning};
    let s = setup("ua");
    let data = theater::game_data().unwrap();
    let sim_data = theater::sim_data(s.theater, "ua").unwrap();
    let mut host = Host::new(s, &data, Rules::default()).unwrap();
    let mut sim = host.new_sim(&sim_data);
    host.begin(&mut sim);
    host.state.drones_at = -tuning::DRONES_EVERY[1];
    for _ in 0..15 * 60 * 120 {
        let e = sim.step_pilots(&[], 15);
        let mut e2 = host.take_sim_events(&mut sim);
        e2.extend(e);
        host.tick(&mut sim, 1.0 / 15.0, &e2);
        if host
            .state
            .raids
            .iter()
            .any(|r| r.drones && matches!(r.state, RaidState::Hit | RaidState::Repelled))
        {
            return;
        }
    }
    panic!("{:?}", host.state.raids);
}

/// The kneeboard's targets are the campaign's: an air mission has none to photograph, a strike
/// its airfield.
#[test]
fn a_sortie_briefs_its_own_target() {
    use stealth_campaign::{MissionKind, Objective, Progress, Sortie, WorldView};
    let data = theater::game_data().unwrap();
    let mut host = Host::new(setup("ce"), &data, Rules::default()).unwrap();
    host.state.tick(0.1, &WorldView::default(), &[]);
    let sortie = |kind| {
        let offer = host.state.board.iter().find(|o| o.kind == kind).cloned().unwrap();
        Sortie {
            id: 1,
            base: offer.base,
            offer,
            aircraft: Default::default(),
            started: 0.0,
            progress: Progress::default(),
        }
    };
    let cap =
        stealth_host::glue::sortie_mission(&host.mission, &host.state, &sortie(MissionKind::Cap));
    assert_eq!(cap.primary.mission_type, 8, "airborne: no photo");
    assert!(cap.primary.shape_name.is_empty());
    assert_eq!((cap.secondary.position.x, cap.secondary.position.y), (0, 0));
    let s = sortie(MissionKind::Strike);
    let Objective::Strike { airfield, .. } = s.offer.objective else { panic!() };
    let strike = stealth_host::glue::sortie_mission(&host.mission, &host.state, &s);
    assert_eq!(strike.primary.mission_type, 2);
    assert_eq!(strike.primary.object_index, airfield as i16);
    assert_eq!(strike.primary.shape_name, "AIRFIELD");
}
