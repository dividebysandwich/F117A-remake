//! The public server list (`stealth-master-proto`): a server's reports to the master server
//! on a thread of their own ([`Reporter`]), and a game's look at the list ([`fetch_list`]).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use stealth_campaign::Side;
use stealth_host::Host;
use stealth_master_proto::{
    LiveMap, MapPlace, MapPos, REPORT_EVERY, Registered, ReportPlayer, ServerEntry, ServerReport,
    SideHolding,
};
use stealth_sim::Sim;
use ureq::config::IpFamily;

use crate::PROTOCOL;
use crate::protocol::PlayerStatus;
use crate::server::Server;

pub use stealth_master_proto::DEFAULT_MASTER;

/// Seconds a request to the master server may take (off the simulation's thread: a slow
/// network costs nothing but the wait), and its name lookup and connection each.
const TIMEOUT: u64 = 20;
const PHASE_TIMEOUT: u64 = 5;
/// After a failed report, the next try (doubling with each failure, up to `REPORT_EVERY`): one
/// lost request must not keep the server away longer than the master's `EXPIRE_AFTER`.
const RETRY: u64 = 5;
/// Seconds at least between reports when the players change (or move: the live map).
const SOON: u64 = 10;

const SITUATIONS: [&str; 3] = ["Cold War", "Limited War", "Conventional War"];
const OPPONENTS: [&str; 4] = ["Green", "Regular", "Veteran", "Elite"];

enum Msg {
    Report(Box<ServerReport>),
    Stop,
}

/// Reports a server to the master server every half minute (and at once the first time),
/// and takes it off the list when it stops.
pub struct Reporter {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
    /// What happened (for a dedicated server's log).
    log: Arc<Mutex<Vec<String>>>,
}

impl Reporter {
    /// Reports to `master` ("https://host").
    pub fn new(master: &str) -> Self {
        let (tx, rx) = mpsc::channel();
        let log = Arc::new(Mutex::new(Vec::new()));
        let (url, l) = (master.trim_end_matches('/').to_string(), log.clone());
        let thread = std::thread::Builder::new()
            .name("master-server".into())
            .spawn(move || run(&url, &rx, &l))
            .ok();
        Self { tx, thread, log }
    }

    /// The server as it is now (sent with the next report).
    pub fn update(&self, report: ServerReport) {
        let _ = self.tx.send(Msg::Report(Box::new(report)));
    }

    pub fn take_log(&self) -> Vec<String> {
        self.log.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
    }

    /// Off the list (waits for the master server a moment).
    pub fn stop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A reporting server's agent speaks IPv4 to the master server: the address it sees is the
/// one listed, and game servers listen on IPv4 only.
/// A fresh connection each time: at a report every half minute a kept one saves nothing, and
/// one a NAT box forgot would only hang the next report.
fn new_agent(timeout: u64, ip: IpFamily) -> ureq::Agent {
    let phase = Some(Duration::from_secs(PHASE_TIMEOUT.min(timeout)));
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .timeout_resolve(phase)
        .timeout_connect(phase)
        .max_idle_connections(0)
        .ip_family(ip)
        .user_agent(concat!("stealth-mission-89/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn run(url: &str, rx: &Receiver<Msg>, log: &Mutex<Vec<String>>) {
    let say = |s: String| {
        if let Ok(mut l) = log.lock() {
            l.push(s);
        }
    };
    let agent = new_agent(TIMEOUT, IpFamily::Ipv4Only);
    let mut latest: Option<ServerReport> = None;
    let mut next = Instant::now();
    let mut every = REPORT_EVERY;
    let mut listed = false;
    let mut fails = 0u32;
    // The players as last reported, and when.
    let mut sent_players = Vec::new();
    let mut sent_at = Instant::now();
    loop {
        let wait = next.saturating_duration_since(Instant::now());
        match rx.recv_timeout(if latest.is_some() { wait } else { Duration::from_secs(3600) }) {
            Ok(Msg::Report(r)) => {
                // Somebody came, went or flew on: the list learns soon, not at the next report.
                let changed = listed
                    && r.players != sent_players
                    && sent_at.elapsed() >= Duration::from_secs(SOON);
                latest = Some(*r);
                if Instant::now() < next && !changed {
                    continue;
                }
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let Some(r) = &latest else { continue };
        sent_at = Instant::now();
        sent_players = r.players.clone();
        match agent.post(format!("{url}/api/servers")).send_json(r) {
            Ok(mut resp) => {
                fails = 0;
                if let Ok(reg) = resp.body_mut().read_json::<Registered>() {
                    every = reg.every.clamp(10, 300);
                }
                if !listed {
                    say(format!("listed on {url}"));
                    listed = true;
                }
                next = Instant::now() + Duration::from_secs(every);
            }
            Err(e) => {
                let retry = (RETRY << fails.min(3)).min(REPORT_EVERY);
                fails += 1;
                say(format!("the server list ({url}): {e} (again in {retry} s)"));
                listed = false;
                next = Instant::now() + Duration::from_secs(retry);
            }
        }
    }
    if let Some(r) = latest.filter(|_| listed) {
        let _ =
            new_agent(2, IpFamily::Ipv4Only).delete(format!("{url}/api/servers/{}", r.port)).call();
    }
}

/// The servers on the list at `master`.
pub fn fetch_list(master: &str) -> Result<Vec<ServerEntry>, String> {
    let url = format!("{}/api/servers", master.trim_end_matches('/'));
    new_agent(TIMEOUT, IpFamily::Any)
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json::<Vec<ServerEntry>>()
        .map_err(|e| e.to_string())
}

/// The campaign's clock: "day 3 14:05".
pub fn clock_text(host: &Host) -> String {
    let h = host.state.sky_hours();
    format!(
        "day {} {:02}:{:02}",
        (h / 24.0).floor() as i64 + 1,
        h.rem_euclid(24.0) as i64,
        (h.rem_euclid(1.0) * 60.0) as i64
    )
}

/// The rules on (empty: classic).
pub fn rules_list(r: &stealth_host::Rules) -> Vec<String> {
    [
        (r.realistic_damage, "realistic damage"),
        (r.realistic_guidance, "realistic guidance"),
        (r.no_crashes, "no crashes"),
        (r.continuous_time, "continuous time"),
        (r.realistic_weapons, "realistic weapons"),
        (r.friendly_fire, "friendly fire"),
    ]
    .iter()
    .filter(|x| x.0)
    .map(|x| x.1.to_string())
    .collect()
}

/// Places (and airfields among them) a side holds.
pub fn held(host: &Host, side: Side) -> (usize, usize) {
    let p = host.state.places.iter().filter(|p| p.owner == side);
    (p.clone().count(), p.filter(|p| p.airfield.is_some()).count())
}

/// The server's report for the list.
pub fn report(server: &Server, host: &Host, sim: &Sim, dedicated: bool) -> ServerReport {
    let setup = &host.state.setup;
    const SIDES: [(Side, &str); 3] =
        [(Side::Blue, "Friendly"), (Side::Red, "Enemy"), (Side::Neutral, "Neutral")];
    let sides = SIDES
        .into_iter()
        .map(|(side, name)| {
            let (places, airfields) = held(host, side);
            SideHolding { side: name.into(), places: places as u16, airfields: airfields as u16 }
        })
        .collect();
    let players = server
        .roster(host, sim)
        .into_iter()
        .map(|p| ReportPlayer {
            pos: matches!(p.status, PlayerStatus::Flying { .. })
                .then(|| sim.pilot_view(p.id.0))
                .flatten()
                .map(|v| MapPos {
                    x: v.mx,
                    y: v.my,
                    heading: ((v.heading as u16 as u32 * 360 + 32768) / 65536 % 360) as u16,
                    alt: v.alt,
                }),
            status: match p.status {
                PlayerStatus::Ops => "On the mission board".into(),
                PlayerStatus::Flying { title, aircraft } => {
                    format!("Flying {title} ({})", aircraft.name())
                }
                PlayerStatus::Reconnecting { .. } => "Reconnecting".into(),
            },
            name: p.name,
            host: p.host,
        })
        .collect();
    ServerReport {
        protocol: PROTOCOL,
        version: env!("CARGO_PKG_VERSION").into(),
        name: server.name.clone(),
        port: server.port().unwrap_or(crate::DEFAULT_PORT),
        dedicated,
        max_players: server.max_players as u16,
        theater: stealth_host::theater::theater_name(&setup.theater_code),
        clock: clock_text(host),
        situation: SITUATIONS.get(setup.roe as usize).unwrap_or(&"").to_string(),
        opponents: OPPONENTS.get(setup.opponents as usize).unwrap_or(&"").to_string(),
        rules: rules_list(&host.rules),
        sides,
        players,
        map: Some(LiveMap {
            theater: setup.theater_code.to_ascii_lowercase(),
            places: host
                .state
                .places
                .iter()
                .map(|p| MapPlace {
                    name: p.name.clone(),
                    x: p.x,
                    y: p.y,
                    side: SIDES.iter().find(|s| s.0 == p.owner).map_or("Neutral", |s| s.1).into(),
                    airfield: p.airfield.is_some(),
                })
                .collect(),
        }),
    }
}
