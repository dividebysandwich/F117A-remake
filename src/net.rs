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
use stealth_net::client::{Client, ClientEvent};
use stealth_net::server::Server;
use stealth_sim::SimEvent;
use stealth_sim::net::{PilotSnap, PilotTick, WorldFast, WorldSlow};

/// At most this many of the server's frames wait to be applied (the newest kept).
const MAX_FRAMES: usize = 30;

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
    /// Chat messages (sender, text).
    pub chat: Vec<(String, String)>,
}

/// Something the menu tells the player when he is back (why a coop game ended).
#[derive(Resource, Default)]
pub struct Notice(pub Option<String>);

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetInbox>()
            .init_resource::<Notice>()
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
            ClientEvent::Chat { from, text } => inbox.chat.push((from, text)),
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
