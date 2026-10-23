//! The host's side of a coop game: the players' connections around the campaign [`Host`] and
//! its simulation. A listen server plays along as player 0 (its game handles player 0's
//! events itself); a dedicated server has no player of its own.
//!
//! Each frame: [`Server::pump`] takes in the players' messages (joins, commands, inputs),
//! [`Server::inputs`] are their flight models' reports for the step, [`Server::after_step`]
//! hands each player his events of the step, [`Server::route`] the host's events, and
//! [`Server::send_frame`] sends the state.

use std::collections::HashMap;
use web_time::Instant;

use stealth_campaign::{CampaignEvent, PlayerId};
use stealth_host::{Host, HostEvent};
use stealth_sim::net::{RemotePilot, SlowBase, WorldFast};
use stealth_sim::{Commands, PilotEvent, PlayerState, Sim, SimEvent};

use crate::protocol::*;
use crate::transport::{Channel, ConnId, Endpoint, NetEvent};

/// Seconds between full campaign updates (the board, the places, the log) and rosters.
const SYNC_EVERY: f32 = 5.0;
const ROSTER_EVERY: f32 = 1.0;
/// Simulation frames between the moving world's messages by default (7.5 a second).
pub const SEND_EVERY: u32 = 2;
/// A player's own state goes again after a change this many times (the channel loses some),
/// and at least every this many messages.
const SNAP_REPEAT: u32 = 3;
const SNAP_REFRESH: u32 = 8;
/// Pilots farther than this from a player (world units, 60 km) are in every `FAR_EVERY`th
/// message only.
const FAR: i32 = 122_880;
const FAR_EVERY: u32 = 4;

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
    /// His own state as last sent (a hash), how often it is still to go, messages since.
    snap_hash: u64,
    snap_repeat: u32,
    snap_age: u32,
    /// Bytes sent him this second, and a second's worth.
    sent: usize,
    rate: u32,
}

pub struct Server {
    net: Endpoint,
    pub name: String,
    /// Players at most (the listen server's own among them).
    pub max_players: usize,
    remotes: HashMap<ConnId, Remote>,
    /// Players who dropped out, and when their aircraft goes.
    grace: HashMap<PlayerId, Instant>,
    /// The listen server's own player.
    local: Option<PlayerId>,
    /// The simulation's step, and the one last sent.
    frame: u32,
    sent_frame: Option<u32>,
    /// Frames between the moving world's messages.
    pub send_every: u32,
    rate_timer: f32,
    sync_timer: f32,
    roster_timer: f32,
    campaign_dirty: bool,
    last_pump: Option<Instant>,
    /// What happened, for a dedicated server's log.
    pub log: Vec<String>,
    /// The players' chat messages (sender, text) for the listen server's own player.
    chat: Vec<(String, String)>,
    /// On the public server list: its reporter, and the time since the last update.
    master: Option<crate::master::Reporter>,
    master_timer: f32,
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
            max_players: max_players.min(MAX_PLAYERS),
            remotes: HashMap::new(),
            grace: HashMap::new(),
            local,
            frame: 0,
            sent_frame: None,
            send_every: SEND_EVERY,
            rate_timer: 0.0,
            sync_timer: 0.0,
            roster_timer: 0.0,
            campaign_dirty: false,
            last_pump: None,
            log: Vec::new(),
            chat: Vec::new(),
            master: None,
            master_timer: f32::MAX,
        })
    }

    /// Puts the server on the public list at `master` (reported to every half minute).
    pub fn list_on(&mut self, master: &str) {
        self.master = Some(crate::master::Reporter::new(master));
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
                            snap_hash: 0,
                            snap_repeat: 0,
                            snap_age: 0,
                            sent: 0,
                            rate: 0,
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
                        out.extend(self.unselect(p, host, sim));
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
        self.master_timer += dt;
        self.rate_timer += dt;
        if self.rate_timer >= 1.0 {
            for r in self.remotes.values_mut() {
                r.rate = (r.sent as f32 / self.rate_timer) as u32;
                r.sent = 0;
            }
            self.rate_timer = 0.0;
        }
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
            (ToServer::Input { seq, state, recent }, Some(p)) => {
                // His client flies his gun rounds.
                sim.set_client_rounds(p.0, true);
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
            (ToServer::RequestSupport { offer }, Some(p)) => host.request_support(sim, p, offer),
            (ToServer::Select { offer, base }, Some(p)) => host.select(sim, p, offer, base),
            (ToServer::Plan { offer, base, route }, Some(p)) => {
                host.edit_plan(sim, p, offer, base, route)
            }
            (ToServer::EndSortie { outcome }, Some(p)) => Ok(host.end_sortie(sim, p, outcome)),
            (ToServer::Hit(hit), Some(p)) => {
                sim.claim_hit(p.0, hit);
                return Vec::new();
            }
            (ToServer::Chat { text }, Some(p)) => {
                let name = host.state.player_name(p).unwrap_or("?").to_string();
                self.relay_chat(Some(conn), &name, &text);
                return Vec::new();
            }
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
                    let mut ev = host.end_sortie(sim, p, 7);
                    self.route(&ev);
                    ev.extend(self.unselect(p, host, sim));
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

    /// A chat message to every player but its sender (`from`: his connection; None for the
    /// listen server's own player), and the log.
    fn relay_chat(&mut self, from: Option<ConnId>, name: &str, text: &str) {
        let text = chat_text(text);
        if text.is_empty() {
            return;
        }
        // A listen server's log is not read.
        if self.local.is_none() {
            self.log.push(format!("{name}: {text}"));
        }
        let msg = encode(&ToClient::Chat { from: name.to_string(), text: text.clone() });
        for r in self.remotes.values() {
            if r.player.is_some() && Some(r.conn) != from {
                self.net.send(r.conn, Channel::Reliable, &msg);
            }
        }
        if from.is_some() && self.local.is_some() {
            self.chat.push((name.to_string(), text));
        }
    }

    /// The listen server's own player says something.
    pub fn say(&mut self, name: &str, text: &str) {
        self.relay_chat(None, name, text);
    }

    /// The players' chat messages since the last call (sender, text): a listen server's.
    pub fn take_chat(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.chat)
    }

    /// A player gone: off the plans he was on.
    fn unselect(&mut self, p: PlayerId, host: &mut Host, sim: &mut Sim) -> Vec<HostEvent> {
        if !host.state.plans.iter().any(|x| x.crew.contains(&p)) {
            return Vec::new();
        }
        let ev = host.select(sim, p, None, 0).unwrap_or_default();
        self.route(&ev);
        ev
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
            // The plans come whole with the event (and change often while dragging).
            if !matches!(e, HostEvent::Campaign(CampaignEvent::PlansChanged(_))) {
                self.campaign_dirty = true;
            }
            let msg = encode(&ToClient::Host(e.clone()));
            for r in self.remotes.values() {
                if r.player.is_some() && to.is_none_or(|p| r.player == Some(p)) {
                    self.net.send(r.conn, Channel::Reliable, &msg);
                }
            }
        }
    }

    /// Sends the state to every player: what changed now and then and the events at once, the
    /// moving world every `send_every` frames.
    pub fn send_frame(&mut self, host: &Host, sim: &mut Sim, now: Instant) {
        self.frame = sim.step_count();
        let due = self.sent_frame.is_none_or(|f| self.frame.wrapping_sub(f) >= self.send_every);
        if due {
            self.sent_frame = Some(self.frame);
        }
        let world = due.then(|| sim.world_fast());
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
        let round = self.frame / self.send_every.max(1);
        let conns: Vec<ConnId> = self.remotes.keys().copied().collect();
        for conn in conns {
            let Some(r) = self.remotes.get_mut(&conn) else { continue };
            let Some(p) = r.player else { continue };
            let mut out: Vec<(Channel, Vec<u8>)> = Vec::new();
            let mut slow = sim.world_slow(&mut r.base);
            slow.shells.extend_from_slice(&shells);
            slow.ai_shells.extend_from_slice(&ai_shells);
            let events = std::mem::take(&mut r.events);
            if slow != Default::default() {
                out.push((Channel::Reliable, encode(&ToClient::Slow(slow))));
            }
            if !events.is_empty() {
                out.push((Channel::Reliable, encode(&ToClient::Events(events))));
            }
            if let Some(world) = &world {
                // His own state when it changed (a few times over), else now and then.
                let snap = sim.pilot_snap(p.0);
                let hash = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    encode(&snap).hash(&mut h);
                    h.finish()
                };
                if hash != r.snap_hash {
                    r.snap_hash = hash;
                    r.snap_repeat = SNAP_REPEAT;
                }
                r.snap_age += 1;
                let pilot = (r.snap_repeat > 0 || r.snap_age >= SNAP_REFRESH).then(|| {
                    r.snap_repeat = r.snap_repeat.saturating_sub(1);
                    r.snap_age = 0;
                    snap
                });
                let fast = ToClient::Fast {
                    step: self.frame,
                    world: Self::seen_by(world, sim, p.0, round),
                    tick: sim.pilot_tick(p.0),
                    pilot,
                };
                out.push((Channel::Unreliable, encode(&fast)));
            }
            if let Some(m) = &campaign {
                out.push((Channel::Reliable, m.clone()));
            }
            if let Some(m) = &roster {
                out.push((Channel::Reliable, m.clone()));
            }
            r.sent += out.iter().map(|(_, m)| m.len()).sum::<usize>();
            for (channel, m) in out {
                self.net.send(conn, channel, &m);
            }
        }
        self.net.flush(now);
        // The public list's view of the server, now and then.
        if self.master.is_some() && self.master_timer >= 1.0 {
            self.master_timer = 0.0;
            let report = crate::master::report(self, host, sim, self.local.is_none());
            if let Some(m) = &self.master {
                m.update(report);
                self.log.extend(m.take_log());
            }
        }
    }

    /// The moving world for pilot `me`: the pilots far from him only in some of the messages
    /// (each in every `FAR_EVERY`th, in turns).
    fn seen_by(world: &WorldFast, sim: &Sim, me: u8, round: u32) -> WorldFast {
        let Some(at) = sim.pilot_view(me).filter(|_| !sim.pilot_absent(me)) else {
            return WorldFast { present: Vec::new(), ..world.clone() };
        };
        let near = |p: &RemotePilot| {
            p.id == me
                || (p.view.wx - at.wx).abs().max((p.view.wy - at.wy).abs()) < FAR
                || (round + p.id as u32) % FAR_EVERY == 0
        };
        WorldFast {
            present: world.pilots.iter().map(|p| p.id).collect(),
            pilots: world.pilots.iter().filter(|p| near(p)).cloned().collect(),
            ..world.clone()
        }
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
                    rate: self.conn_of(id).and_then(|c| self.remotes.get(&c)).map_or(0, |r| r.rate),
                }
            })
            .collect()
    }

    /// The server goes down: everybody is told.
    pub fn shutdown(&mut self, reason: &str) {
        if let Some(mut m) = self.master.take() {
            m.stop();
        }
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
