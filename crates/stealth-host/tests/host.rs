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
