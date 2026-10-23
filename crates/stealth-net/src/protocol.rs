//! The coop messages and their encoding (CBOR, LZ4-compressed when large).

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use stealth_campaign::{Aircraft, CampaignState, PlaceId, PlayerId, RoutePoint, Sortie};
use stealth_host::{HostEvent, Rules, Stores};
use stealth_sim::claims::GunHit;
use stealth_sim::net::{PilotSnap, PilotTick, WorldFast, WorldSlow};
use stealth_sim::{Commands, PlayerState, SimEvent};

/// Changes with every change of the messages: client and server must agree.
pub const PROTOCOL: u16 = 15;
pub const DEFAULT_PORT: u16 = 28989;
/// Players a server takes at most.
pub const MAX_PLAYERS: usize = stealth_sim::MAX_PILOTS;
/// A player who dropped out keeps his aircraft this long for his return (seconds).
pub const GRACE: f32 = 30.0;
/// Characters a chat message has at most.
pub const MAX_CHAT: usize = 120;

/// A player's message to the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToServer {
    Hello {
        protocol: u16,
        name: String,
    },
    /// The flight model's report of one frame (numbered), with the commands of the last few
    /// frames again (a lost packet loses no trigger press).
    Input {
        seq: u32,
        state: PlayerState,
        recent: Vec<(u32, Commands)>,
    },
    TakeMission {
        offer: u32,
        base: Option<u16>,
        aircraft: Aircraft,
        stores: Option<Stores>,
    },
    OrderInsertion {
        place: PlaceId,
    },
    /// Call in AI SEAD for an offer's place.
    RequestSupport {
        offer: u32,
    },
    /// Plan a mission of the board with the others on it, from a base (None: plan none).
    Select {
        offer: Option<u32>,
        base: u16,
    },
    /// Change the base and route of the mission he plans.
    Plan {
        offer: u32,
        base: u16,
        route: Vec<RoutePoint>,
    },
    /// End the sortie (the simulation's outcome codes: 0 landed, 1 crashed, 7 aborted).
    EndSortie {
        outcome: u8,
    },
    /// Landed at a friendly base: rearm and refuel.
    Rearm {
        stores: Stores,
    },
    /// One of his own gun rounds hit (he flies them himself; the server checks).
    Hit(GunHit),
    /// A chat message to the others.
    Chat {
        text: String,
    },
    Bye,
}

/// Where a returning player's aircraft is: his sortie and the simulation's view of him.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resume {
    pub sortie: Sortie,
    pub state: PlayerState,
}

/// A player as the others see him.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
    pub status: PlayerStatus,
    /// Round trip (ms); 0 for the host's own player.
    pub ping: u16,
    /// The host's own player (listen server).
    pub host: bool,
    /// What the server sends him (bytes a second).
    #[serde(default)]
    pub rate: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlayerStatus {
    /// On the Ops screen.
    Ops,
    Flying {
        title: String,
        aircraft: Aircraft,
    },
    /// Dropped out: his aircraft waits this many seconds more.
    Reconnecting {
        seconds: u16,
    },
}

/// The server's message to a player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToClient {
    Welcome {
        id: PlayerId,
        server: String,
        rules: Rules,
        state: CampaignState,
        /// He comes back within the grace period: his sortie goes on.
        resume: Option<Resume>,
    },
    Reject {
        reason: String,
    },
    /// What moves and the player's own state, a few times a second (unreliable). `step`: the
    /// server's frame; `pilot` only when it changed (and now and then).
    Fast {
        step: u32,
        world: WorldFast,
        tick: PilotTick,
        pilot: Option<PilotSnap>,
    },
    /// What changed now and then.
    Slow(WorldSlow),
    /// The simulation's events for this player.
    Events(Vec<SimEvent>),
    /// The campaign's news, and the player's sorties.
    Host(HostEvent),
    /// The whole campaign (now and then: the board, the places, the log).
    Campaign(CampaignState),
    /// A command failed.
    Error(String),
    Roster(Vec<PlayerInfo>),
    /// Another player's chat message.
    Chat {
        from: String,
        text: String,
    },
    Shutdown(String),
}

/// A message's bytes: a flag (1 compressed) and CBOR.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut cbor = Vec::new();
    if ciborium::into_writer(msg, &mut cbor).is_err() {
        return Vec::new();
    }
    if cbor.len() > 200 {
        let mut out = vec![1];
        out.extend(lz4_flex::compress_prepend_size(&cbor));
        out
    } else {
        let mut out = vec![0];
        out.extend(cbor);
        out
    }
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    let (&flag, body) = bytes.split_first()?;
    match flag {
        0 => ciborium::from_reader(body).ok(),
        1 => {
            let raw = lz4_flex::decompress_size_prepended(body).ok()?;
            ciborium::from_reader(raw.as_slice()).ok()
        }
        _ => None,
    }
}

/// A chat message as it goes out: on one line, without control characters, `MAX_CHAT`
/// characters at most (empty: nothing to send).
pub fn chat_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MAX_CHAT).collect::<String>().trim().to_string()
}

/// Adds frame `b`'s one-shot commands to `a`'s (keys pressed in either).
pub fn merge_commands(a: &mut Commands, b: &Commands) {
    a.fire |= b.fire;
    a.gun |= b.gun;
    a.trigger |= b.trigger;
    a.flare |= b.flare;
    a.chaff |= b.chaff;
    a.decoy |= b.decoy;
    a.toggle_irj ^= b.toggle_irj;
    a.toggle_ecm ^= b.toggle_ecm;
    a.toggle_bay ^= b.toggle_bay;
    a.next_bay |= b.next_bay;
    a.designate |= b.designate;
    a.next_target |= b.next_target;
    a.nearest_target |= b.nearest_target;
    a.clear_target |= b.clear_target;
    a.camera_dir = b.camera_dir.or(a.camera_dir);
    a.eject |= b.eject;
    a.lock_object = b.lock_object.or(a.lock_object);
    a.lock_unit = b.lock_unit.or(a.lock_unit);
    a.lock_pilot = b.lock_pilot.or(a.lock_pilot);
    a.designate_at = b.designate_at.or(a.designate_at);
    // Not in a coop game: invulnerability and the training resupply.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_survive_encoding() {
        let m = ToServer::Input {
            seq: 7,
            state: PlayerState { x: 123_456, altitude: 3000, ..Default::default() },
            recent: vec![(6, Commands { fire: true, ..Default::default() })],
        };
        assert_eq!(decode::<ToServer>(&encode(&m)), Some(m));
        let plan = ToServer::Plan {
            offer: 3,
            base: 40,
            route: vec![RoutePoint { x: 1, y: -2, kind: stealth_campaign::RouteKind::Land }],
        };
        assert_eq!(decode::<ToServer>(&encode(&plan)), Some(plan));
        let chat = ToServer::Chat { text: "Wheels up".into() };
        assert_eq!(decode::<ToServer>(&encode(&chat)), Some(chat));
        let big = ToClient::Error("x".repeat(5000));
        let bytes = encode(&big);
        assert!(bytes.len() < 500 && bytes[0] == 1);
        assert_eq!(format!("{:?}", decode::<ToClient>(&bytes)), format!("{:?}", Some(big)));
    }

    #[test]
    fn chat_text_is_one_short_line() {
        assert_eq!(chat_text("  Hi\nthere\t "), "Hithere");
        assert_eq!(chat_text(&"x".repeat(500)).len(), MAX_CHAT);
        assert_eq!(chat_text(" \r "), "");
    }

    #[test]
    fn other_pilots_keep_their_missile_slots_smoke_and_turret() {
        use stealth_sim::Projectile;
        use stealth_sim::net::RemotePilot;
        let p = RemotePilot {
            id: 2,
            view: Default::default(),
            gear_up: true,
            on_ground: false,
            door_anim: 2,
            lockheed: false,
            external_stores: true,
            helicopter: true,
            gunship: true,
            gun_firing: true,
            gun_aim: Some((0x4000, -0x800)),
            burning: false,
            smoke: 3,
            chute: None,
            missiles: vec![(3, Projectile { ttl: 40, spec: 5, ..Default::default() })],
            cms: Default::default(),
        };
        let w = WorldFast { pilots: vec![p.clone()], ..Default::default() };
        let back = decode::<WorldFast>(&encode(&w)).unwrap();
        assert_eq!(back.pilots, vec![p]);
    }
}
