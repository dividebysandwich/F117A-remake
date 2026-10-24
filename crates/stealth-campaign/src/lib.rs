//! The realtime dynamic campaign: two sides hold the cities and
//! airfields of a theater and take each other's places by landing troops with helicopters;
//! the enemy also raids the friendly airfields. A mission board offers sorties made from the
//! live situation (recon, SEAD, CAP, CAS, anti-ship, strike, escort, intercept, airfield
//! defence), and insertions the AI flies. Pilots never die or get captured: a sortie just
//! ends and the next one starts from the board. The war is won when the enemy holds no
//! airfield, lost when the friendly side holds none (nor a carrier). Each side's AI runs on
//! the war materiel its places bring in and the aircraft it has left; the players never run
//! short of either.
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
mod events;
mod missions;
mod record;
pub mod rng;
mod state;
mod supply;
pub mod tuning;
pub mod weather;

use serde::{Deserialize, Serialize};

pub use record::PilotRecord;
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
    /// A helicopter stopped on the ground (anywhere).
    #[serde(default)]
    pub landed: bool,
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
    /// A friendly aircraft detected an enemy site (its radar on the RWR, or it fired).
    SiteSpotted {
        object: ObjectId,
    },
}

/// A player's request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Fly a mission from the board, from a friendly base (None: the offer's base).
    TakeMission {
        offer: u32,
        base: Option<ObjectId>,
        #[serde(default)]
        aircraft: Aircraft,
    },
    /// Send helicopters with troops against an enemy place.
    OrderInsertion { place: PlaceId },
    /// The player's sortie is over (landed and done, crashed, ejected, aborted).
    EndSortie { end: SortieEnd },
    /// Join the plan of an offer from a base (the plan's base unless nobody is on it), or
    /// leave the plans (None).
    Select { offer: Option<u32>, base: ObjectId },
    /// Change the plan the player is on: its base (resets the route) and its route.
    Plan { offer: u32, base: ObjectId, route: Vec<RoutePoint> },
    /// Call in AI SEAD against the SAM sites of an offer's place.
    RequestSupport { offer: u32 },
}

/// What the authority has to do or show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CampaignEvent {
    /// The mission board changed.
    BoardChanged,
    /// The players' plans changed (all of them).
    PlansChanged(Vec<MissionPlan>),
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
    /// Launch helicopters (tags) of a side from a base to land at a world object.
    InsertionLaunched {
        id: u32,
        #[serde(default = "blue")]
        side: Side,
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
    /// One of a place's destroyed guns or MANPADS teams is replaced.
    GunRepaired {
        place: PlaceId,
    },
    /// Strike aircraft (tags) of a side take off from a base to bomb a world object, or
    /// attack drones to dive into it.
    RaidLaunched {
        id: u32,
        side: Side,
        from: ObjectId,
        target: ObjectId,
        tags: Vec<u32>,
        #[serde(default)]
        drones: bool,
    },
    /// An AWACS of a side takes off from a base to orbit a point.
    LaunchAwacs {
        tag: u32,
        side: Side,
        from: ObjectId,
        x: i32,
        y: i32,
    },
    /// A campaign aircraft is called home (it lands at its base).
    Recall {
        tag: u32,
    },
    /// The war is over.
    Ended {
        winner: Side,
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
        #[serde(default)]
        task: FighterTask,
    },
    /// The enemy pushes a battery of guns forward to (x, y).
    BatteryDeployed {
        id: u32,
        x: i32,
        y: i32,
    },
    /// News for the players (shown on the HUD and in the campaign log).
    Message(String),
}

fn blue() -> Side {
    Side::Blue
}

impl CampaignState {
    /// Carry out a player's command.
    pub fn apply(&mut self, player: PlayerId, cmd: Command) -> Result<Vec<CampaignEvent>, String> {
        match cmd {
            Command::TakeMission { offer, base, aircraft } => {
                self.take_mission(player, offer, base, aircraft)
            }
            Command::OrderInsertion { place } => self.order_insertion(Some(player), place),
            Command::EndSortie { end } => Ok(self.end_sortie(player, end)),
            Command::Select { offer, base } => self.select(player, offer, base),
            Command::Plan { offer, base, route } => self.edit_plan(player, offer, base, route),
            Command::RequestSupport { offer } => self.request_support(player, offer),
        }
    }

    /// Advance the campaign clock by `dt` real seconds with what happened meanwhile.
    pub fn tick(&mut self, dt: f64, view: &WorldView, reports: &[Report]) -> Vec<CampaignEvent> {
        let mut ev = Vec::new();
        self.clock += dt.max(0.0);
        for r in reports {
            self.report(r, view, &mut ev);
        }
        self.track_sorties(dt, view, &mut ev);
        // Over: the sorties in the air may still finish, nothing else happens.
        if self.outcome.is_some() {
            return ev;
        }
        self.supply();
        self.run_downed(&mut ev);
        self.run_insertions(view, &mut ev);
        self.run_raids(&mut ev);
        self.repairs(&mut ev);
        self.expire_intel(&mut ev);
        self.enemy_air(view, &mut ev);
        self.friendly_cap(view, &mut ev);
        self.auto_insertion(view, &mut ev);
        self.enemy_offensive(view, &mut ev);
        self.run_awacs(&mut ev);
        self.drone_waves(&mut ev);
        self.forward_batteries(view, &mut ev);
        if self.check_outcome(&mut ev) {
            return ev;
        }
        if self.clock - self.board_at >= tuning::BOARD_REFRESH
            || ev.iter().any(|e| matches!(e, CampaignEvent::PlaceCaptured { .. }))
        {
            self.board_at = self.clock;
            if self.rebuild_board(view) {
                ev.push(CampaignEvent::BoardChanged);
            }
            if self.prune_plans() {
                ev.push(self.plans_changed());
            }
        }
        ev
    }

    /// After loading a save: the simulation starts afresh, so whatever was in the air is gone.
    /// Insertions in flight start over, sorties end as aborted.
    pub fn after_load(&mut self) -> Vec<CampaignEvent> {
        self.ground_everything();
        let lead = self.clock + tuning::INSERTION_LEAD;
        for ins in &mut self.insertions {
            if ins.state == InsertionState::Enroute {
                ins.state = InsertionState::Pending { launch_at: lead };
                ins.helicopters.clear();
                ins.alive.clear();
                ins.scrambled = false;
                ins.enroute_at = None;
            }
        }
        let lead = self.clock + tuning::RAID_LEAD;
        for r in &mut self.raids {
            if r.state == RaidState::Enroute {
                r.state = RaidState::Pending { launch_at: lead };
                r.aircraft.clear();
                r.alive.clear();
                r.escorted = false;
                r.enroute_at = None;
            }
        }
        self.patrols.clear();
        self.caps.clear();
        // The AWACS went down with the simulation: another one goes up soon.
        self.awacs = None;
        // The batteries' guns are placed again.
        for b in &mut self.batteries {
            b.seen = false;
        }
        self.board_at = f64::MIN;
        for p in &mut self.plans {
            p.crew.clear();
        }
        let players: Vec<PlayerId> =
            self.players.iter().filter(|p| p.sortie.is_some()).map(|p| p.id).collect();
        players.into_iter().flat_map(|p| self.end_sortie(p, SortieEnd::Aborted)).collect()
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
