//! A coop session over loopback: a dedicated-style server and a client with a mirror.

use std::time::{Duration, Instant};

use stealth_campaign::{Aircraft, CampaignSetup, PlayerId};
use stealth_host::{Host, HostEvent, Rules, theater};
use stealth_net::client::{Client, ClientEvent};
use stealth_net::protocol::{PlayerStatus, ToClient, ToServer, encode};
use stealth_net::server::Server;
use stealth_sim::{PlayerState, Sim};

struct World {
    host: Host,
    sim: Sim,
    server: Server,
    mirror: Sim,
}

fn world() -> World {
    let setup = CampaignSetup {
        theater: 3,
        theater_code: "ce".into(),
        roe: 1,
        opponents: 1,
        seed: 99,
        sky_ratio: 24.0,
        start_hour: 8.0,
    };
    let data = theater::game_data().unwrap();
    let sim_data = theater::sim_data(3, "ce").unwrap();
    let mut host = Host::new(setup, &data, Rules::default()).unwrap();
    let mut sim = host.new_sim(&sim_data);
    host.begin(&mut sim);
    let mirror = host.new_sim(&sim_data);
    let mut server = Server::new("Test", 0, 8, None).unwrap();
    // Every frame: the mirror is compared with the server's simulation as of the same frame.
    server.send_every = 1;
    World { host, sim, server, mirror }
}

/// One server frame at `now` and the client's side of it.
fn frame(
    w: &mut World,
    c: &mut Client,
    input: Option<&PlayerState>,
    now: Instant,
) -> Vec<ClientEvent> {
    let ev = w.server.pump(&mut w.host, &mut w.sim, now);
    let _ = ev;
    let inputs = w.server.inputs();
    let mut events = w.sim.step_pilots(&inputs, 15);

    events.extend(w.host.take_sim_events(&mut w.sim));
    w.server.after_step(&events);
    let hev = w.host.tick(&mut w.sim, 1.0 / 15.0, &events);
    w.server.route(&hev);
    w.server.send_frame(&w.host, &mut w.sim, now);
    std::thread::sleep(Duration::from_millis(1));
    if let Some(ps) = input {
        c.send_input(ps);
    }
    c.flush(now);
    let got = c.poll(now);
    for e in &got {
        match e {
            ClientEvent::Welcome { id, .. } => w.mirror.make_mirror(id.0),
            ClientEvent::Slow(s) => w.mirror.apply_world_slow(s),
            ClientEvent::Fast { world, tick, pilot, .. } => {
                w.mirror.apply_world_fast(world);
                if let Some(pilot) = pilot {
                    w.mirror.apply_pilot_snap(pilot);
                }
                w.mirror.apply_pilot_tick(tick);
            }
            _ => {}
        }
    }
    w.mirror.mirror_frame();
    got
}

#[test]
fn a_player_joins_flies_and_returns() {
    let mut w = world();
    let port = w.server.port().unwrap();
    let mut c = Client::connect(&format!("127.0.0.1:{port}"), "Maverick").unwrap();
    let mut now = Instant::now();
    let mut me = None;
    let mut sizes = (0usize, 0usize);
    for _ in 0..300 {
        now += Duration::from_millis(66);
        for e in frame(&mut w, &mut c, None, now) {
            if let ClientEvent::Welcome { id, state, .. } = e {
                assert_eq!(state.player_name(id), Some("Maverick"));
                me = Some(id);
            }
        }
        if me.is_some() {
            break;
        }
    }
    let me = me.expect("welcome");
    assert_eq!(me, PlayerId(1), "player 0 is the host's");
    // Wait for the board, then fly its first mission in an F-16.
    let mut start = None;
    for _ in 0..300 {
        now += Duration::from_millis(66);
        if start.is_none()
            && let Some(o) = w.host.state.board.iter().find(|o| !o.kind.ai_flown())
        {
            c.send(&ToServer::TakeMission {
                offer: o.id,
                base: None,
                aircraft: Aircraft::F16,
                stores: None,
            });
        }
        for e in frame(&mut w, &mut c, None, now) {
            if let ClientEvent::Host(HostEvent::SortieStarted { player, start: s, .. }) = e {
                assert_eq!(player, me);
                start = Some(s);
            }
        }
        if start.is_some() {
            break;
        }
    }
    let mut ps = start.expect("sortie started");
    ps.on_ground = false;
    ps.altitude = 2000;
    ps.velocity = 300 * 27;
    ps.gear_up = true;
    let mut roster = None;
    for _ in 0..60 {
        now += Duration::from_millis(66);
        ps.y += 300;
        for e in frame(&mut w, &mut c, Some(&ps), now) {
            if let ClientEvent::Roster(r) = e {
                roster = Some(r);
            }
        }
    }
    assert!(!w.sim.pilot_absent(me.0));
    assert_eq!(w.sim.pilot_view(me.0).map(|p| p.alt), Some(2000), "the server flies his reports");
    assert!(!w.mirror.player_absent(), "the mirror knows he flies");
    let live =
        |s: &Sim| -> Vec<_> { s.flights().iter().filter(|u| u.flags != 0).copied().collect() };
    assert_eq!(live(&w.mirror), live(&w.sim));
    let r = roster.expect("roster");
    assert!(matches!(r[0].status, PlayerStatus::Flying { aircraft: Aircraft::F16, .. }), "{r:?}");
    let fast = encode(&ToClient::Fast {
        step: 0,
        world: w.sim.world_fast(),
        tick: w.sim.pilot_tick(me.0),
        pilot: Some(w.sim.pilot_snap(me.0)),
    });
    sizes.0 = fast.len();
    let slow = w.sim.world_slow(&mut Default::default());
    sizes.1 = encode(&ToClient::Slow(slow)).len();
    eprintln!("frame {} bytes, join {} bytes", sizes.0, sizes.1);
    assert!(sizes.0 < 16_000);
    // He drops out (no more packets): the aircraft waits, then a new connection under his name
    // takes it back.
    drop(c);
    for _ in 0..10 {
        now += Duration::from_secs(2);
        w.server.pump(&mut w.host, &mut w.sim, now);
    }
    assert!(!w.sim.pilot_absent(me.0), "the aircraft waits for him");
    let mut c = Client::connect(&format!("127.0.0.1:{port}"), "maverick").unwrap();
    let mut resumed = None;
    for _ in 0..300 {
        now += Duration::from_millis(66);
        for e in frame(&mut w, &mut c, None, now) {
            if let ClientEvent::Welcome { id, resume, .. } = e {
                assert_eq!(id, me);
                resumed = Some(resume.is_some());
            }
        }
        if resumed.is_some() {
            break;
        }
    }
    assert_eq!(resumed, Some(true));
    // Gone for good: after the grace period the sortie ends.
    drop(c);
    for _ in 0..30 {
        now += Duration::from_secs(2);
        w.server.pump(&mut w.host, &mut w.sim, now);
    }
    assert!(w.sim.pilot_absent(me.0));
    assert!(w.host.state.sortie(me).is_none());
}

#[test]
fn a_full_campaign_is_saved_and_resumed() {
    let w = world();
    let dir = std::env::temp_dir().join(format!("sm89-net-{}", std::process::id()));
    let path = dir.join("dedicated.json");
    stealth_host::save::save_to(&path, &w.host.state).unwrap();
    let back = stealth_host::save::load_from(&path).unwrap().unwrap();
    assert_eq!(back, w.host.state);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sixty_four_pilots_fit_in_a_frame() {
    let mut w = world();
    let sim = &mut w.sim;
    let base = (3..sim.objects().len())
        .find(|&i| sim.objects()[i].flags & 0xF0F == 0x401)
        .expect("friendly base");
    let b = sim.objects()[base];
    let mut inputs = Vec::new();
    for id in 0..64u8 {
        sim.respawn_pilot(id, base, None, false, id % 2 == 0);
        // Half of them together near the base, the others spread out far away.
        let (dx, dy) = if id < 32 { (id as i32 * 8, 0) } else { (id as i32 * 0x300, 0x2000) };
        let (x, y) = Sim::map_to_world(b.x + dx, b.y - 0x400 - dy);
        let ps = PlayerState {
            x,
            y,
            altitude: 3000 + id as u16 * 50,
            heading: id as u16 * 1000,
            velocity: 400 * 27,
            gear_up: true,
            fuel: 10000,
            ..Default::default()
        };
        inputs.push((id, ps));
    }
    for _ in 0..4 {
        for (_, ps) in inputs.iter_mut() {
            ps.y += 200;
        }
        sim.step_pilots(&inputs, 15);
    }
    let world = sim.world_fast();
    assert_eq!(world.pilots.len(), 64);
    let size = |pilots: usize| {
        let mut world = world.clone();
        world.pilots.truncate(pilots);
        encode(&ToClient::Fast { step: 0, world, tick: sim.pilot_tick(1), pilot: None }).len()
    };
    let (none, all) = (size(0), size(64));
    eprintln!("frame without pilots {none} bytes, with 64 {all} bytes");
    assert!((all - none) / 64 < 60, "a pilot costs {} bytes", (all - none) / 64);
    assert!(all < 6_000);
}
