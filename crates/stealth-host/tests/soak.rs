//! A day of war without players, in the simulation, for the campaign's balance: who holds
//! what each hour, each side's materiel and aircraft, where the enemy attacks.
//! `cargo test --release -p stealth-host --test soak -- --ignored --nocapture --test-threads=1`

use stealth_campaign::{CampaignSetup, Side};
use stealth_host::{Host, Rules, theater};

fn soak(code: &str, opponents: u8, hours: usize) {
    let s = CampaignSetup {
        theater: theater::theater_index(code).unwrap(),
        theater_code: code.into(),
        roe: 1,
        opponents,
        seed: 777,
        sky_ratio: 24.0,
        start_hour: 8.0,
        dynamic_weather: true,
    };
    let data = theater::game_data().unwrap();
    let sim_data = theater::sim_data(s.theater, code).unwrap();
    let mut host = Host::new(s, &data, Rules::default()).unwrap();
    let mut sim = host.new_sim(&sim_data);
    host.begin(&mut sim);
    let mut seen = 0.0;
    let mut targets: Vec<String> = Vec::new();
    for h in 1..=hours {
        for _ in 0..15 * 3600 {
            let e = sim.step_pilots(&[], 15);
            let mut e2 = host.take_sim_events(&mut sim);
            e2.extend(e);
            host.tick(&mut sim, 1.0 / 15.0, &e2);
        }
        let st = &host.state;
        let new: Vec<&str> =
            st.log.iter().filter(|l| l.time > seen).map(|l| l.text.as_str()).collect();
        seen = st.clock;
        let count = |k: &str| new.iter().filter(|t| t.contains(k)).count();
        targets.extend(
            new.iter()
                .filter_map(|t| t.split_once("enemy helicopters lift off from "))
                .filter_map(|(_, r)| r.split_once(" for ").map(|(_, p)| p.split(" in ").next()))
                .flatten()
                .map(str::to_string),
        );
        let (b, r) = (&st.blue, &st.red);
        println!(
            "[{code} skill {opponents}] h{h:2}: held B{:2} R{:2} | B {:3.0} mat {:2} ftr {:2} helo | R {:3.0} mat {:2} ftr {:2} helo | CAPs {} raids {} insertions {} lost {} taken {}",
            st.held(Side::Blue),
            st.held(Side::Red),
            b.materiel,
            b.fighters,
            b.helicopters,
            r.materiel,
            r.fighters,
            r.helicopters,
            count("CAP scrambled"),
            count("strike aircraft to raid"),
            count("enemy helicopters lift off"),
            count("lost to the enemy"),
            count(" captured"),
        );
        if st.outcome.is_some() {
            println!("over: {:?}", st.outcome);
            break;
        }
    }
    targets.sort();
    let mut by: Vec<(usize, String)> = Vec::new();
    for t in targets {
        match by.iter_mut().find(|b| b.1 == t) {
            Some(b) => b.0 += 1,
            None => by.push((1, t)),
        }
    }
    by.sort_by_key(|b| std::cmp::Reverse(b.0));
    println!("[{code} skill {opponents}] enemy insertion targets: {by:?}");
}

#[test]
#[ignore]
fn soak_central_europe() {
    for skill in [1, 3] {
        soak("ce", skill, 24);
    }
}

#[test]
#[ignore]
fn soak_ukraine() {
    soak("ua", 2, 24);
}

#[test]
#[ignore]
fn soak_libya() {
    soak("lb", 1, 12);
}
