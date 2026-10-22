//! A player joins through the master server's relay, as the browser build does: a WebSocket to
//! the relay, which passes each message on as a datagram to the listed game server.

use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use stealth_campaign::CampaignSetup;
use stealth_host::{Host, Rules, theater};
use stealth_master::List;
use stealth_master_proto::{ServerReport, server_id};
use stealth_net::client::{Client, ClientEvent};
use stealth_net::server::Server;
use stealth_net::transport::{Datagrams, Endpoint};
use tungstenite::{Message, WebSocket};

/// The browser's WebSocket, natively.
struct Ws(WebSocket<TcpStream>);

impl Datagrams for Ws {
    fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let _ = self.0.flush();
        match self.0.read() {
            Ok(Message::Binary(b)) => {
                buf[..b.len()].copy_from_slice(&b);
                Ok((b.len(), "0.0.0.0:0".parse().unwrap()))
            }
            Ok(_) => Err(std::io::ErrorKind::WouldBlock.into()),
            Err(tungstenite::Error::Io(e)) => Err(e),
            Err(e) => Err(std::io::Error::other(e)),
        }
    }

    fn send_to(&mut self, p: &[u8], _to: SocketAddr) {
        let _ = self.0.send(Message::Binary(p.to_vec().into()));
    }
}

#[test]
fn a_browser_player_joins_through_the_relay() {
    let setup = CampaignSetup {
        theater: 3,
        theater_code: "ce".into(),
        roe: 1,
        opponents: 1,
        seed: 7,
        sky_ratio: 24.0,
        start_hour: 8.0,
        dynamic_weather: true,
    };
    let data = theater::game_data().unwrap();
    let mut host = Host::new(setup, &data, Rules::default()).unwrap();
    let mut sim = host.new_sim(&theater::sim_data(3, "ce").unwrap());
    host.begin(&mut sim);
    let mut server = Server::new("Relayed", 0, 8, None).unwrap();
    server.send_every = 1;

    // The server on the list, and the relay (to this machine: a test).
    let mut list = List::default();
    let report =
        ServerReport { name: "Relayed".into(), port: server.port().unwrap(), ..Default::default() };
    let local: IpAddr = "127.0.0.1".parse().unwrap();
    let body = serde_json::to_vec(&report).unwrap();
    assert_eq!(list.handle("POST", "/api/servers", &body, local, Instant::now()).status, 200);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let relay = listener.local_addr().unwrap();
    let list = Arc::new(Mutex::new(list));
    std::thread::spawn(move || stealth_master::relay::serve(listener, list, true));

    let id = server_id("Relayed", "127.0.0.1");
    let stream = TcpStream::connect(relay).unwrap();
    let (ws, _) = tungstenite::client(format!("ws://{relay}/relay/{id}"), stream).unwrap();
    ws.get_ref().set_nonblocking(true).unwrap();
    let net = Endpoint::client_over(Box::new(Ws(ws)), "0.0.0.0:0".parse().unwrap());
    let mut c = Client::over(net, "Goose");

    let (mut welcomed, mut frames) = (false, 0);
    let mut now = Instant::now();
    for _ in 0..300 {
        now += Duration::from_millis(66);
        server.pump(&mut host, &mut sim, now);
        let mut events = sim.step_pilots(&server.inputs(), 15);
        events.extend(host.take_sim_events(&mut sim));
        server.after_step(&events);
        let hev = host.tick(&mut sim, 1.0 / 15.0, &events);
        server.route(&hev);
        server.send_frame(&host, &mut sim, now);
        c.flush(now);
        std::thread::sleep(Duration::from_millis(3));
        for e in c.poll(now) {
            match e {
                ClientEvent::Welcome { id, state, .. } => {
                    assert_eq!(state.player_name(id), Some("Goose"));
                    welcomed = true;
                }
                ClientEvent::Fast { .. } => frames += 1,
                ClientEvent::Closed(why) => panic!("closed: {why}"),
                _ => {}
            }
        }
        if welcomed && frames >= 5 {
            return;
        }
    }
    panic!("welcomed: {welcomed}, frames: {frames}");
}
