//! The realtime dynamic campaign (not in the original game): two sides hold the cities and
//! airfields of a theater; the friendly side takes the enemy's places by landing troops with
//! helicopters. A mission board offers sorties made from the live situation (recon, SEAD, CAP,
//! CAS, anti-ship, strike, escort), and insertions the AI flies. Pilots never die or get
//! captured: a sortie just ends and the next one starts from the board.
//!
//! The crate knows nothing of the flight or the renderer. One authority (the game, or a coop
//! host) owns a [`CampaignState`] and drives it with:
//!
//! * [`CampaignState::apply`]: a player's [`Command`] (take a mission, order an insertion, end
//!   a sortie), with the player's id.
//! * [`CampaignState::tick`]: the campaign clock (real seconds), a [`WorldView`] of what the
//!   simulation sees now and the [`Report`]s of what happened since the last tick.
//!
//! Both return [`CampaignEvent`]s the authority applies to its simulation (helicopters to
//! launch, places that change hands, repairs, intelligence) and shows to the players. The
//! state is plain serialisable data with its own random stream: the same seed, commands and
//! reports give the same campaign, and a snapshot is all a joining client needs.

mod director;
mod missions;
pub mod rng;
mod state;
pub mod tuning;

use serde::{Deserialize, Serialize};

pub use state::*;

/// What the simulation sees now (sent with every tick).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldView {
    /// The low-level air defences, by their index in the simulation.
    pub aaa: Vec<AaaView>,
    /// Aircraft in the air (alive).
    pub flights: Vec<FlightView>,
    /// The players' aircraft (absent while on the board).
    pub players: Vec<PlayerPos>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AaaView {
    pub x: i32,
    pub y: i32,
    pub side: Side,
    pub alive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FlightView {
    pub x: i32,
    pub y: i32,
    pub side: Side,
    /// Campaign tag (0: not launched by the campaign).
    pub tag: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerPos {
    pub id: PlayerId,
    pub x: i32,
    pub y: i32,
    pub airborne: bool,
}

/// Something that happened in the simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Report {
    /// A world object (SAM site, airfield, ship, city building) was destroyed.
    ObjectDestroyed {
        object: ObjectId,
        by: Option<PlayerId>,
    },
    /// A gun or MANPADS team (AAA unit index) was destroyed.
    AaaDestroyed {
        index: u32,
        by: Option<PlayerId>,
    },
    FlightDestroyed {
        tag: u32,
        side: Side,
        x: i32,
        y: i32,
        by: Option<PlayerId>,
    },
    /// A campaign aircraft landed (helicopters at their goal, patrols back at base).
    FlightLanded {
        tag: u32,
        x: i32,
        y: i32,
    },
    /// A player photographed a world object (recon).
    PhotoTaken {
        player: PlayerId,
        object: ObjectId,
    },
}

/// A player's request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Fly a mission from the board, from a friendly base (None: the offer's base).
    TakeMission { offer: u32, base: Option<ObjectId> },
    /// Send helicopters with troops against an enemy place.
    OrderInsertion { place: PlaceId },
    /// The player's sortie is over (landed and done, crashed, ejected, aborted).
    EndSortie { end: SortieEnd },
}

/// What the authority has to do or show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CampaignEvent {
    /// The mission board changed.
    BoardChanged,
    SortieStarted {
        player: PlayerId,
        sortie: Sortie,
    },
    /// The player's objective is achieved (once per sortie).
    ObjectiveAchieved {
        player: PlayerId,
        title: String,
    },
    SortieFinished {
        player: PlayerId,
        record: SortieRecord,
    },
    /// Launch helicopters (tags) from a base to land at a world object.
    InsertionLaunched {
        id: u32,
        from: ObjectId,
        target: ObjectId,
        tags: Vec<u32>,
    },
    InsertionFailed {
        id: u32,
        place: PlaceId,
    },
    /// A place changed hands: its cities and airfields (repaired) and the defences removed with
    /// it.
    PlaceCaptured {
        place: PlaceId,
        side: Side,
        members: Vec<ObjectId>,
        removed: Vec<ObjectId>,
    },
    /// A destroyed site or airfield is back in service.
    Repaired {
        object: ObjectId,
    },
    /// Recon: the defences of these objects see less far and the players spot them from
    /// further away, until the intelligence expires.
    IntelGained {
        place: PlaceId,
        objects: Vec<ObjectId>,
    },
    IntelExpired {
        place: PlaceId,
        objects: Vec<ObjectId>,
    },
    /// Fighters of a side take off from a base towards a point.
    LaunchFighters {
        tag: u32,
        side: Side,
        from: ObjectId,
        x: i32,
        y: i32,
    },
    /// News for the players (shown on the HUD and in the campaign log).
    Message(String),
}

impl CampaignState {
    /// Carry out a player's command.
    pub fn apply(&mut self, player: PlayerId, cmd: Command) -> Result<Vec<CampaignEvent>, String> {
        match cmd {
            Command::TakeMission { offer, base } => self.take_mission(player, offer, base),
            Command::OrderInsertion { place } => self.order_insertion(Some(player), place),
            Command::EndSortie { end } => Ok(self.end_sortie(player, end).into_iter().collect()),
        }
    }

    /// Advance the campaign clock by `dt` real seconds with what happened meanwhile.
    pub fn tick(&mut self, dt: f64, view: &WorldView, reports: &[Report]) -> Vec<CampaignEvent> {
        let mut ev = Vec::new();
        self.clock += dt.max(0.0);
        for r in reports {
            self.report(r, &mut ev);
        }
        self.track_sorties(dt, view, &mut ev);
        self.run_insertions(view, &mut ev);
        self.repairs(&mut ev);
        self.expire_intel(&mut ev);
        self.enemy_air(view, &mut ev);
        self.auto_insertion(view, &mut ev);
        if self.clock - self.board_at >= tuning::BOARD_REFRESH
            || ev.iter().any(|e| matches!(e, CampaignEvent::PlaceCaptured { .. }))
        {
            self.board_at = self.clock;
            if self.rebuild_board(view) {
                ev.push(CampaignEvent::BoardChanged);
            }
        }
        ev
    }

    /// After loading a save: the simulation starts afresh, so whatever was in the air is gone.
    /// Insertions in flight start over, sorties end as aborted.
    pub fn after_load(&mut self) -> Vec<CampaignEvent> {
        let lead = self.clock + tuning::INSERTION_LEAD;
        for ins in &mut self.insertions {
            if ins.state == InsertionState::Enroute {
                ins.state = InsertionState::Pending { launch_at: lead };
                ins.helicopters.clear();
                ins.alive.clear();
                ins.scrambled = false;
            }
        }
        self.patrols.clear();
        self.board_at = f64::NEG_INFINITY;
        let players: Vec<PlayerId> =
            self.players.iter().filter(|p| p.sortie.is_some()).map(|p| p.id).collect();
        players.into_iter().filter_map(|p| self.end_sortie(p, SortieEnd::Aborted)).collect()
    }

    /// Everything the simulation needs to match the campaign when it is built: destroyed and
    /// removed objects, intelligence, and the owner of every tracked object.
    pub fn world_state(&self) -> WorldState {
        WorldState {
            owners: self
                .sites
                .iter()
                .filter_map(|s| Some((s.object, self.owner(s.object)?)))
                .collect(),
            destroyed: self.sites.iter().filter(|s| !s.alive()).map(|s| s.object).collect(),
            intel: self
                .sites
                .iter()
                .filter(|s| self.has_intel(s.object))
                .map(|s| s.object)
                .collect(),
        }
    }
}

/// The campaign's world for building a simulation (see [`CampaignState::world_state`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    pub owners: Vec<(ObjectId, Side)>,
    pub destroyed: Vec<ObjectId>,
    pub intel: Vec<ObjectId>,
}

#[cfg(test)]
mod tests;
