//! The host's side of a coop game: the players' connections around the campaign [`Host`] and
//! its simulation. A listen server plays along as player 0 (its game handles player 0's
//! events itself); a dedicated server has no player of its own.
//!
//! Each frame: [`Server::pump`] takes in the players' messages (joins, commands, inputs),
//! [`Server::inputs`] are their flight models' reports for the step, [`Server::after_step`]
//! hands each player his events of the step, [`Server::route`] the host's events, and
//! [`Server::send_frame`] sends the state.

use std::collections::HashMap;
use std::time::Instant;

use stealth_campaign::PlayerId;
use stealth_host::{Host, HostEvent};
use stealth_sim::net::SlowBase;
use stealth_sim::{Commands, PilotEvent, PlayerState, Sim, SimEvent};

use crate::protocol::*;
use crate::transport::{Channel, ConnId, Endpoint, NetEvent};

/// Seconds between full campaign updates (the board, the places, the log) and rosters.
const SYNC_EVERY: f32 = 5.0;
const ROSTER_EVERY: f32 = 1.0;

/// A connection, and once it has joined its player.
struct Remote {
    conn: ConnId,
    player: Option<PlayerId>,
    base: SlowBase,
    /// The latest report of his flight model, and the commands since the last step.
    state: Option<PlayerState>,
    fresh: bool,
    pending: Commands,
    last_seq: Option<u32>,
    events: Vec<SimEvent>,
}

pub struct Server {
    net: Endpoint,
    pub name: String,
    remotes: HashMap<ConnId, Remote>,
    /// Players who dropped out, and when their aircraft goes.
    grace: HashMap<PlayerId, Instant>,
    /// The listen server's own player.
    local: Option<PlayerId>,
    step: u32,
    sync_timer: f32,
    roster_timer: f32,
    campaign_dirty: bool,
    last_pump: Option<Instant>,
    /// What happened, for a dedicated server's log.
    pub log: Vec<String>,
}

impl Server {
    /// Listens on `port` (all interfaces). `local`: the listen server's own player.
    pub fn new(
        name: &str,
        port: u16,
        max_players: usize,
        local: Option<PlayerId>,
    ) -> std::io::Result<Self> {
        let others = max_players.min(MAX_PLAYERS).saturating_sub(local.is_some() as usize);
        let net = Endpoint::server(&format!("0.0.0.0:{port}"), others)?;
        Ok(Self {
            net,
            name: name.to_string(),
            remotes: HashMap::new(),
            grace: HashMap::new(),
            local,
            step: 0,
            sync_timer: 0.0,
            roster_timer: 0.0,
            campaign_dirty: false,
            last_pump: None,
            log: Vec::new(),
        })
    }

    pub fn port(&self) -> Option<u16> {
        self.net.local_addr().map(|a| a.port())
    }

    fn send(&mut self, conn: ConnId, channel: Channel, msg: &ToClient) {
        self.net.send(conn, channel, &encode(msg));
    }

    fn conn_of(&self, player: PlayerId) -> Option<ConnId> {
        self.remotes.values().find(|r| r.player == Some(player)).map(|r| r.conn)
    }

    /// Joined players connected now.
    pub fn connected(&self) -> Vec<PlayerId> {
        self.remotes.values().filter_map(|r| r.player).collect()
    }

    /// The players' messages since the last call; what they caused in the campaign (for the
    /// listen server's own player too).
    pub fn pump(&mut self, host: &mut Host, sim: &mut Sim, now: Instant) -> Vec<HostEvent> {
        let dt = self.last_pump.map_or(0.0, |t| now.duration_since(t).as_secs_f32());
        self.last_pump = Some(now);
        let mut out = Vec::new();
        for e in self.net.poll(now) {
            match e {
                NetEvent::Connected(conn) => {
                    self.remotes.insert(
                        conn,
                        Remote {
                            conn,
                            player: None,
                            base: SlowBase::default(),
                            state: None,
                            fresh: false,
                            pending: Commands::default(),
                            last_seq: None,
                            events: Vec::new(),
                        },
                    );
                }
                NetEvent::Disconnected(conn, why) => {
                    if let Some(r) = self.remotes.remove(&conn)
                        && let Some(p) = r.player
                    {
                        let name = host.state.player_name(p).unwrap_or("?").to_string();
                        self.log.push(format!("{name} left ({why})"));
                        if host.state.sortie(p).is_some() {
                            self.grace.insert(p, now + std::time::Duration::from_secs_f32(GRACE));
                        }
                    }
                }
                NetEvent::Message(conn, _, bytes) => {
                    let Some(msg) = decode::<ToServer>(&bytes) else { continue };
                    out.extend(self.message(conn, msg, host, sim, now));
                }
            }
        }
        // Dropped out for too long: the sortie is over.
        let expired: Vec<PlayerId> =
            self.grace.iter().filter(|(_, t)| now >= **t).map(|(p, _)| *p).collect();
        for p in expired {
            self.grace.remove(&p);
            let ev = host.end_sortie(sim, p, 7);
            self.route(&ev);
            out.extend(ev);
        }
        self.sync_timer += dt;
        self.roster_timer += dt;
        out
    }

    fn message(
        &mut self,
        conn: ConnId,
        msg: ToServer,
        host: &mut Host,
        sim: &mut Sim,
        now: Instant,
    ) -> Vec<HostEvent> {
        let player = self.remotes.get(&conn).and_then(|r| r.player);
        let result = match (msg, player) {
            (ToServer::Hello { protocol, name }, None) => {
                self.hello(conn, protocol, &name, host, sim, now);
                return Vec::new();
            }
            (ToServer::Input { seq, state, recent }, Some(_)) => {
                let Some(r) = self.remotes.get_mut(&conn) else { return Vec::new() };
                if r.last_seq.is_some_and(|l| seq.wrapping_sub(l) as i32 <= 0) {
                    return Vec::new();
                }
                for (s, c) in recent.iter().chain(std::iter::once(&(seq, state.cmd.clone()))) {
                    if r.last_seq.is_none_or(|l| s.wrapping_sub(l) as i32 > 0) {
                        merge_commands(&mut r.pending, c);
                    }
                }
                r.last_seq = Some(seq);
                r.state = Some(state);
                r.fresh = true;
                return Vec::new();
            }
            (ToServer::TakeMission { offer, base, aircraft, stores }, Some(p)) => {
                host.take_mission(sim, p, offer, base, aircraft, stores)
            }
            (ToServer::OrderInsertion { place }, Some(p)) => host.order_insertion(sim, p, place),
            (ToServer::EndSortie { outcome }, Some(p)) => Ok(host.end_sortie(sim, p, outcome)),
            (ToServer::Rearm { stores }, Some(p)) => {
                host.rearm(sim, p, &stores);
                Ok(Vec::new())
            }
            (ToServer::Bye, _) => {
                self.net.close(conn, "Left");
                if let Some(r) = self.remotes.remove(&conn)
                    && let Some(p) = r.player
                {
                    // Leaving on purpose: the sortie ends now.
                    let name = host.state.player_name(p).unwrap_or("?").to_string();
                    self.log.push(format!("{name} left"));
                    let ev = host.end_sortie(sim, p, 7);
                    self.route(&ev);
                    return ev;
                }
                return Vec::new();
            }
            _ => return Vec::new(),
        };
        match result {
            Ok(ev) => {
                self.route(&ev);
                ev
            }
            Err(e) => {
                self.send(conn, Channel::Reliable, &ToClient::Error(e));
                Vec::new()
            }
        }
    }

    fn hello(
        &mut self,
        conn: ConnId,
        protocol: u16,
        name: &str,
        host: &mut Host,
        sim: &mut Sim,
        now: Instant,
    ) {
        let name: String = name.trim().chars().filter(|c| !c.is_control()).take(24).collect();
        let reject = if protocol != PROTOCOL {
            Some(format!("Different game version (protocol {protocol}, server {PROTOCOL})"))
        } else if name.is_empty() {
            Some("No name".to_string())
        } else {
            let taken = host
                .state
                .players
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(&name))
                .is_some_and(|p| Some(p.id) == self.local || self.conn_of(p.id).is_some());
            taken.then(|| format!("{name} is already playing"))
        };
        if let Some(reason) = reject {
            self.send(conn, Channel::Reliable, &ToClient::Reject { reason: reason.clone() });
            self.net.flush(now);
            self.net.close(conn, &reason);
            return;
        }
        let busy = |id: PlayerId| {
            Some(id) == self.local || self.conn_of(id).is_some() || self.grace.contains_key(&id)
        };
        let Some(id) = host.join(sim, &name, busy) else {
            let reason = "The campaign has no room for more pilots".to_string();
            self.send(conn, Channel::Reliable, &ToClient::Reject { reason: reason.clone() });
            self.net.flush(now);
            self.net.close(conn, &reason);
            return;
        };
        // Back within the grace period: the aircraft is still there.
        let resume = if self.grace.remove(&id).is_some() && !sim.pilot_absent(id.0) {
            host.state
                .sortie(id)
                .cloned()
                .map(|sortie| Resume { sortie, state: sim.pilot_input(id.0) })
        } else {
            if host.state.sortie(id).is_some() {
                // A sortie without an aircraft (the server restarted): it is over.
                let ev = host.end_sortie(sim, id, 7);
                self.route(&ev);
            }
            None
        };
        if let Some(r) = self.remotes.get_mut(&conn) {
            r.player = Some(id);
        }
        let welcome = ToClient::Welcome {
            id,
            server: self.name.clone(),
            rules: host.rules,
            state: host.state.clone(),
            resume,
        };
        self.send(conn, Channel::Reliable, &welcome);
        self.log.push(format!("{name} joined as player {}", id.0 + 1));
        self.roster_timer = ROSTER_EVERY;
    }

    /// The players' reports for this step (those with a new one).
    pub fn inputs(&mut self) -> Vec<(u8, PlayerState)> {
        let mut out = Vec::new();
        for r in self.remotes.values_mut() {
            let (Some(p), Some(s)) = (r.player, r.state.as_ref()) else { continue };
            if !r.fresh && r.pending == Commands::default() {
                continue;
            }
            r.fresh = false;
            let mut ps = s.clone();
            ps.cmd = std::mem::take(&mut r.pending);
            out.push((p.0, ps));
        }
        out
    }

    /// A step's events, to the players they are for.
    pub fn after_step(&mut self, events: &[PilotEvent]) {
        for r in self.remotes.values_mut() {
            let Some(p) = r.player else { continue };
            r.events.extend(
                events
                    .iter()
                    .filter(|e| !e.event.is_private() || e.pilot == p.0)
                    .map(|e| e.event.clone()),
            );
        }
    }

    /// The host's events to the players they are for (the campaign's news to everyone).
    pub fn route(&mut self, events: &[HostEvent]) {
        for e in events {
            let to = match e {
                HostEvent::SortieStarted { player, .. }
                | HostEvent::SortieFinished { player, .. }
                | HostEvent::Landed { player, .. } => Some(*player),
                HostEvent::Campaign(_) => None,
            };
            self.campaign_dirty = true;
            let msg = encode(&ToClient::Host(e.clone()));
            for r in self.remotes.values() {
                if r.player.is_some() && to.is_none_or(|p| r.player == Some(p)) {
                    self.net.send(r.conn, Channel::Reliable, &msg);
                }
            }
        }
    }

    /// Sends the frame's state to every player.
    pub fn send_frame(&mut self, host: &Host, sim: &mut Sim, now: Instant) {
        self.step = self.step.wrapping_add(1);
        let world = sim.world_fast();
        let (shells, ai_shells) = sim.take_new_shells();
        let sync = self.sync_timer >= SYNC_EVERY || (self.campaign_dirty && self.sync_timer >= 1.0);
        let campaign = sync.then(|| encode(&ToClient::Campaign(host.state.clone())));
        if sync {
            self.sync_timer = 0.0;
            self.campaign_dirty = false;
        }
        let roster = (self.roster_timer >= ROSTER_EVERY).then(|| {
            self.roster_timer = 0.0;
            encode(&ToClient::Roster(self.roster(host, sim)))
        });
        let conns: Vec<ConnId> = self.remotes.keys().copied().collect();
        for conn in conns {
            let Some(r) = self.remotes.get_mut(&conn) else { continue };
            let Some(p) = r.player else { continue };
            let mut slow = sim.world_slow(&mut r.base);
            slow.shells.extend_from_slice(&shells);
            slow.ai_shells.extend_from_slice(&ai_shells);
            let events = std::mem::take(&mut r.events);
            if slow != Default::default() {
                let m = encode(&ToClient::Slow(slow));
                self.net.send(conn, Channel::Reliable, &m);
            }
            if !events.is_empty() {
                let m = encode(&ToClient::Events(events));
                self.net.send(conn, Channel::Reliable, &m);
            }
            let fast = ToClient::Fast {
                step: self.step,
                world: world.clone(),
                pilot: sim.pilot_snap(p.0),
            };
            let m = encode(&fast);
            self.net.send(conn, Channel::Unreliable, &m);
            if let Some(m) = &campaign {
                self.net.send(conn, Channel::Reliable, m);
            }
            if let Some(m) = &roster {
                self.net.send(conn, Channel::Reliable, m);
            }
        }
        self.net.flush(now);
    }

    /// Everybody in the campaign now: the host's player, the connected ones, those dropped out.
    pub fn roster(&self, host: &Host, sim: &Sim) -> Vec<PlayerInfo> {
        let now = Instant::now();
        let mut ids: Vec<PlayerId> = self.connected();
        ids.extend(self.local);
        ids.extend(self.grace.keys().copied());
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .map(|id| {
                let status = if let Some(t) = self.grace.get(&id) {
                    PlayerStatus::Reconnecting {
                        seconds: t.saturating_duration_since(now).as_secs() as u16,
                    }
                } else if let Some(s) = host.state.sortie(id).filter(|_| !sim.pilot_absent(id.0)) {
                    PlayerStatus::Flying { title: s.offer.title.clone(), aircraft: s.aircraft }
                } else {
                    PlayerStatus::Ops
                };
                let ping = self.conn_of(id).map_or(0, |c| (self.net.rtt(c) * 1000.0) as u16);
                PlayerInfo {
                    id,
                    name: host.state.player_name(id).unwrap_or("?").to_string(),
                    status,
                    ping,
                    host: Some(id) == self.local,
                }
            })
            .collect()
    }

    /// The server goes down: everybody is told.
    pub fn shutdown(&mut self, reason: &str) {
        let m = encode(&ToClient::Shutdown(reason.to_string()));
        let conns: Vec<ConnId> = self.remotes.keys().copied().collect();
        for c in &conns {
            self.net.send(*c, Channel::Reliable, &m);
        }
        let now = Instant::now();
        self.net.flush(now);
        for c in conns {
            self.net.close(c, reason);
        }
        self.net.flush(now);
    }
}
