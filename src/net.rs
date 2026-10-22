//! Coop multiplayer in the game (crate stealth-net): the link to the other players, a
//! listen server's or a client's.
//!
//! * A listen server ([`NetLink::Host`]) runs the war as alone and steps the simulation with
//!   the joined players' reports too (`combat.rs`); the campaign hands them their share
//!   (`campaign/mod.rs`).
//! * A client ([`NetLink::Client`]) keeps a mirror of the server's simulation: the server's
//!   frames and events land in the [`NetInbox`] here and the combat step applies them instead
//!   of stepping; the campaign's news go to the campaign.

use web_time::Instant;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use stealth_campaign::PlayerId;
use stealth_net::client::{Client, ClientEvent};
use stealth_net::protocol::{PlayerInfo, PlayerStatus};
use stealth_net::server::Server;
use stealth_sim::SimEvent;
use stealth_sim::net::{PilotSnap, PilotTick, WorldFast, WorldSlow};

/// At most this many of the server's frames wait to be applied (the newest kept).
const MAX_FRAMES: usize = 30;
/// Seconds a player's join or leave stays up, and how many are up at most.
const NOTICE_SECONDS: f32 = 6.0;
const MAX_NOTICES: usize = 4;

#[derive(Resource)]
pub enum NetLink {
    Host(Server),
    Client(Client),
}

impl NetLink {
    pub fn is_client(&self) -> bool {
        matches!(self, NetLink::Client(_))
    }
}

/// What the server sent for the simulation, in the order it came.
pub enum SimMsg {
    /// The server's frame, the world, the pilot's ticking state and (when it changed) the
    /// rest of it.
    Fast(u32, Box<WorldFast>, PilotTick, Option<Box<PilotSnap>>),
    Slow(Box<WorldSlow>),
    Events(Vec<SimEvent>),
}

/// A client's messages from the server, for the combat step and the campaign.
#[derive(Resource, Default)]
pub struct NetInbox {
    pub sim: Vec<SimMsg>,
    pub campaign: Vec<ClientEvent>,
}

/// Something the menu tells the player when he is back (why a coop game ended).
#[derive(Resource, Default)]
pub struct Notice(pub Option<String>);

/// Players joining and leaving the coop game, each with the seconds it stays up (oldest
/// first): a bubble at the screen's top left and a line on the HUD.
#[derive(Resource, Default)]
pub struct PlayerNotices(pub Vec<(String, f32)>);

impl PlayerNotices {
    pub fn push(&mut self, text: String) {
        self.0.push((text, NOTICE_SECONDS));
        let extra = self.0.len().saturating_sub(MAX_NOTICES);
        self.0.drain(..extra);
    }

    /// The newest one.
    pub fn latest(&self) -> Option<&str> {
        self.0.last().map(|(s, _)| s.as_str())
    }

    /// Who joined and who left between two rosters (not this player himself; nothing for
    /// the first roster, which is everybody already there).
    pub fn roster_changed(&mut self, old: &[PlayerInfo], new: &[PlayerInfo], me: PlayerId) {
        if old.is_empty() {
            return;
        }
        let here = |r: &[PlayerInfo], id| {
            r.iter().any(|p| p.id == id && !matches!(p.status, PlayerStatus::Reconnecting { .. }))
        };
        for p in new.iter().filter(|p| p.id != me) {
            if here(new, p.id) && !here(old, p.id) {
                self.push(format!("{} joined", p.name));
            }
        }
        for p in old.iter().filter(|p| p.id != me) {
            if here(old, p.id) && !here(new, p.id) {
                let dropped = new.iter().any(|q| q.id == p.id);
                self.push(format!(
                    "{} {}",
                    p.name,
                    if dropped { "lost connection" } else { "left" }
                ));
            }
        }
    }
}

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetInbox>()
            .init_resource::<Notice>()
            .init_resource::<PlayerNotices>()
            .add_systems(Update, age_notices)
            .add_systems(EguiPrimaryContextPass, notice_bubble)
            .add_systems(
                Update,
                pump_client.before(crate::combat::CombatSystems).run_if(resource_exists::<NetLink>),
            )
            .add_systems(Last, goodbye.run_if(resource_exists::<NetLink>));
    }
}

/// A client's messages from the server into the inbox, and what it has to send out.
fn pump_client(mut link: ResMut<NetLink>, mut inbox: ResMut<NetInbox>) {
    let NetLink::Client(client) = &mut *link else { return };
    let now = Instant::now();
    for e in client.poll(now) {
        match e {
            ClientEvent::Fast { step, world, tick, pilot } => {
                inbox.sim.push(SimMsg::Fast(step, world, tick, pilot))
            }
            ClientEvent::Slow(s) => inbox.sim.push(SimMsg::Slow(s)),
            ClientEvent::Events(e) => inbox.sim.push(SimMsg::Events(e)),
            e => inbox.campaign.push(e),
        }
    }
    // Frames piled up (the flight was still loading): the latest are enough; the slow
    // changes and events must all be kept.
    let fast = inbox.sim.iter().filter(|m| matches!(m, SimMsg::Fast(..))).count();
    if fast > MAX_FRAMES {
        let mut skip = fast - MAX_FRAMES;
        inbox.sim.retain(|m| {
            if skip > 0 && matches!(m, SimMsg::Fast(..)) {
                skip -= 1;
                return false;
            }
            true
        });
    }
    client.flush(now);
}

fn age_notices(time: Res<Time<Real>>, mut notices: ResMut<PlayerNotices>) {
    if notices.0.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    notices.0.retain_mut(|(_, t)| {
        *t -= dt;
        *t > 0.0
    });
}

/// The joins and leaves in a small bubble at the top left, over everything.
fn notice_bubble(mut contexts: EguiContexts, notices: Res<PlayerNotices>) {
    if notices.0.is_empty() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    egui::Area::new(egui::Id::new("player_notices"))
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(12.0, 12.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(190))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    for (text, t) in &notices.0 {
                        // Fades out over the last second.
                        let a = (t.min(1.0) * 255.0) as u8;
                        let col = egui::Color32::from_rgba_unmultiplied(255, 220, 90, a);
                        ui.label(egui::RichText::new(text).color(col).size(16.0));
                    }
                });
        });
}

/// The game quits: the server learns at once that the player left (a listen server tells its
/// players), rather than after the timeout.
fn goodbye(mut exit: MessageReader<AppExit>, mut link: ResMut<NetLink>) {
    if exit.read().next().is_none() {
        return;
    }
    match &mut *link {
        NetLink::Client(client) => client.leave(),
        NetLink::Host(server) => server.shutdown("The host left the game"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: u8, name: &str, status: PlayerStatus) -> PlayerInfo {
        PlayerInfo { id: PlayerId(id), name: name.into(), status, ping: 0, host: false, rate: 0 }
    }

    #[test]
    fn roster_changes_become_notices() {
        let me = player(0, "ME", PlayerStatus::Ops);
        let ann = player(1, "ANN", PlayerStatus::Ops);
        let bob = player(2, "BOB", PlayerStatus::Ops);
        let gone = PlayerStatus::Reconnecting { seconds: 30 };
        let mut n = PlayerNotices::default();
        // The first roster is who is already there.
        n.roster_changed(&[], &[me.clone(), ann.clone()], me.id);
        assert!(n.0.is_empty());
        n.roster_changed(
            &[me.clone(), ann.clone()],
            &[me.clone(), ann.clone(), bob.clone()],
            me.id,
        );
        n.roster_changed(
            &[me.clone(), ann.clone(), bob.clone()],
            &[me.clone(), bob.clone()],
            me.id,
        );
        let dropped = player(2, "BOB", gone);
        n.roster_changed(&[me.clone(), bob.clone()], &[me.clone(), dropped.clone()], me.id);
        n.roster_changed(&[me.clone(), dropped], &[me.clone(), bob], me.id);
        let texts: Vec<&str> = n.0.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(texts, ["BOB joined", "ANN left", "BOB lost connection", "BOB joined"]);
        assert_eq!(n.latest(), Some("BOB joined"));
    }
}
