//! A player's side of a coop game: the connection to the server, his flight model's reports
//! out, the server's state and events in.

use std::collections::VecDeque;
use web_time::Instant;

use stealth_campaign::{CampaignState, PlayerId};
use stealth_host::{HostEvent, Rules};
use stealth_sim::net::{PilotSnap, PilotTick, WorldFast, WorldSlow};
use stealth_sim::{Commands, PlayerState, SimEvent};

use crate::protocol::*;
use crate::transport::{Channel, Endpoint, NetEvent};

/// Frames whose commands each input repeats.
const REPEAT: usize = 4;

/// What came from the server.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// Connected; the server takes the player in (or not).
    Connected,
    Welcome {
        id: PlayerId,
        server: String,
        rules: Rules,
        state: Box<CampaignState>,
        resume: Option<Resume>,
    },
    /// Not taken in, or gone: why.
    Closed(String),
    Fast {
        step: u32,
        world: Box<WorldFast>,
        tick: PilotTick,
        pilot: Option<Box<PilotSnap>>,
    },
    Slow(Box<WorldSlow>),
    Events(Vec<SimEvent>),
    Host(HostEvent),
    Campaign(Box<CampaignState>),
    Error(String),
    Roster(Vec<PlayerInfo>),
    /// Another player's chat message.
    Chat {
        from: String,
        text: String,
    },
}

pub struct Client {
    net: Endpoint,
    name: String,
    pub id: Option<PlayerId>,
    seq: u32,
    recent: VecDeque<(u32, Commands)>,
    last_step: Option<u32>,
    closed: bool,
}

impl Client {
    /// Starts connecting to `addr` ("host" or "host:port") as `name`.
    pub fn connect(addr: &str, name: &str) -> std::io::Result<Self> {
        Ok(Self::over(Endpoint::client(&with_default_port(addr))?, name))
    }

    /// Starts connecting through the master server's relay at `url`
    /// (`master::relay_url`): the browser build's way to a server.
    #[cfg(target_arch = "wasm32")]
    pub fn relayed(url: &str, name: &str) -> std::io::Result<Self> {
        Ok(Self::over(Endpoint::relay(url)?, name))
    }

    /// Starts connecting through `net` (`Endpoint::client_over`).
    pub fn over(net: Endpoint, name: &str) -> Self {
        Self {
            net,
            name: name.to_string(),
            id: None,
            seq: 0,
            recent: VecDeque::new(),
            last_step: None,
            closed: false,
        }
    }

    pub fn send(&mut self, msg: &ToServer) {
        self.net.send(0, Channel::Reliable, &encode(msg));
    }

    /// The flight model's report of one frame.
    pub fn send_input(&mut self, state: &PlayerState) {
        if self.id.is_none() {
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        let msg = ToServer::Input {
            seq: self.seq,
            state: state.clone(),
            recent: self.recent.iter().cloned().collect(),
        };
        self.net.send(0, Channel::Unreliable, &encode(&msg));
        if state.cmd != Commands::default() {
            self.recent.push_back((self.seq, state.cmd.clone()));
        }
        let seq = self.seq;
        self.recent.retain(|(s, _)| seq.wrapping_sub(*s) < REPEAT as u32);
    }

    /// A chat message to the others.
    pub fn chat(&mut self, text: &str) {
        let text = chat_text(text);
        if self.id.is_some() && !text.is_empty() {
            self.send(&ToServer::Chat { text });
        }
    }

    /// Reports a hit of one of his own gun rounds.
    pub fn send_hit(&mut self, hit: stealth_sim::claims::GunHit) {
        if self.id.is_some() {
            self.send(&ToServer::Hit(hit));
        }
    }

    /// Round trip time (seconds).
    pub fn rtt(&self) -> f32 {
        self.net.rtt(0)
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// What came in since the last call.
    pub fn poll(&mut self, now: Instant) -> Vec<ClientEvent> {
        let mut out = Vec::new();
        for e in self.net.poll(now) {
            match e {
                NetEvent::Connected(_) => {
                    let hello = ToServer::Hello { protocol: PROTOCOL, name: self.name.clone() };
                    self.send(&hello);
                    out.push(ClientEvent::Connected);
                }
                NetEvent::Disconnected(_, why) => {
                    self.closed = true;
                    out.push(ClientEvent::Closed(why));
                }
                NetEvent::Message(_, _, bytes) => {
                    let Some(msg) = decode::<ToClient>(&bytes) else { continue };
                    out.extend(self.message(msg));
                }
            }
        }
        out
    }

    fn message(&mut self, msg: ToClient) -> Option<ClientEvent> {
        Some(match msg {
            ToClient::Welcome { id, server, rules, state, resume } => {
                self.id = Some(id);
                ClientEvent::Welcome { id, server, rules, state: Box::new(state), resume }
            }
            ToClient::Reject { reason } => {
                self.closed = true;
                ClientEvent::Closed(reason)
            }
            ToClient::Fast { step, world, tick, pilot } => {
                // An older frame than the last one is of no use.
                if self.last_step.is_some_and(|l| step.wrapping_sub(l) as i32 <= 0) {
                    return None;
                }
                self.last_step = Some(step);
                ClientEvent::Fast { step, world: Box::new(world), tick, pilot: pilot.map(Box::new) }
            }
            ToClient::Slow(s) => ClientEvent::Slow(Box::new(s)),
            ToClient::Events(e) => ClientEvent::Events(e),
            ToClient::Host(e) => ClientEvent::Host(e),
            ToClient::Campaign(s) => ClientEvent::Campaign(Box::new(s)),
            ToClient::Error(e) => ClientEvent::Error(e),
            ToClient::Roster(r) => ClientEvent::Roster(r),
            ToClient::Chat { from, text } => ClientEvent::Chat { from, text },
            ToClient::Shutdown(why) => {
                self.closed = true;
                ClientEvent::Closed(why)
            }
        })
    }

    /// Sends what is queued.
    pub fn flush(&mut self, now: Instant) {
        self.net.flush(now);
    }

    /// Leaves the game (the server ends the sortie).
    pub fn leave(&mut self) {
        self.send(&ToServer::Bye);
        let now = Instant::now();
        self.net.flush(now);
        self.net.close(0, "Left");
        self.net.flush(now);
        self.closed = true;
    }
}

/// "host" or "host:port" with the port: the default one if none.
fn with_default_port(addr: &str) -> String {
    let addr = addr.trim();
    if addr.rsplit_once(':').is_some_and(|(h, p)| {
        !h.is_empty() && p.parse::<u16>().is_ok() && (!h.contains(':') || h.ends_with(']'))
    }) {
        addr.to_string()
    } else if addr.contains(':') && !addr.starts_with('[') {
        // A bare IPv6 address.
        format!("[{addr}]:{DEFAULT_PORT}")
    } else {
        format!("{addr}:{DEFAULT_PORT}")
    }
}
