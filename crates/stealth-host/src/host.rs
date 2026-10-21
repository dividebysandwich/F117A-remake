//! The campaign's authority: the campaign model driving one simulation for all its players
//! (the single-player game, a listen server or a dedicated server).
//!
//! The host does not own the simulation (the game keeps it with its renderer): every call
//! takes it. Every frame the simulation's events become campaign reports, and the campaign's
//! events are applied to the simulation. What the players have to know comes back as
//! [`HostEvent`]s; what the host's calls raised in the simulation itself (tiles destroyed and
//! repaired, rearming) is in [`Host::take_sim_events`].

use serde::{Deserialize, Serialize};
use stealth_campaign::{
    Aircraft, CampaignEvent, CampaignSetup, CampaignState, Command, Objective, PlaceId, PlayerId,
    Report, Side, Sortie, SortieRecord,
};
use stealth_mission::{GameData, Mission};
use stealth_sim::{PilotEvent, PlayerState, Sim, SimEvent};

use crate::glue::{self, side_of};
use crate::rules::Rules;

/// Seconds between the friendly aircraft's looks for enemy radars.
const SPOT_EVERY: f64 = 1.0;

/// A finished sortie for its pilot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Debrief {
    pub record: SortieRecord,
    /// How the flight ended.
    pub title: String,
    /// What happened in the campaign meanwhile.
    pub news: Vec<String>,
}

/// What the players have to know.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HostEvent {
    /// The campaign's news and changes (for everyone; `SortieStarted` and `ObjectiveAchieved`
    /// are about one player).
    Campaign(CampaignEvent),
    /// A player's sortie started: the start state for his flight model.
    SortieStarted {
        player: PlayerId,
        sortie: Sortie,
        start: PlayerState,
    },
    SortieFinished {
        player: PlayerId,
        debrief: Debrief,
    },
    /// A player came to a stop on a friendly runway (rearm, or end the sortie).
    Landed {
        player: PlayerId,
        base: usize,
    },
}

/// The stores a player chose for a sortie: stations (weapon index, count; -1 empty) and the
/// one selected.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Stores {
    pub stations: Vec<(i32, i32)>,
    pub select: usize,
}

pub struct Host {
    pub state: CampaignState,
    /// The campaign's own mission (its world is the theater's), the base of the sorties'.
    pub mission: Mission,
    pub rules: Rules,
    spot_timer: f64,
    started: bool,
    /// The sky: days since 1970 of the mission date, the theater's latitude.
    days: f64,
    lat: f64,
    /// Events the host's own calls took from the simulation and did not use.
    leftover: Vec<PilotEvent>,
}

impl Host {
    /// A new campaign.
    pub fn new(setup: CampaignSetup, data: &GameData, rules: Rules) -> anyhow::Result<Self> {
        let mission = glue::base_mission(&setup, data)?;
        let state = CampaignState::new(setup, &mission.world);
        Ok(Self::with_state(state, mission, rules))
    }

    /// A saved campaign.
    pub fn resume(state: CampaignState, data: &GameData, rules: Rules) -> anyhow::Result<Self> {
        let mission = glue::base_mission(&state.setup, data)?;
        Ok(Self::with_state(state, mission, rules))
    }

    fn with_state(state: CampaignState, mission: Mission, rules: Rules) -> Self {
        let (y, m, d) = crate::sky::briefing_date(&mission.briefing.date);
        let days = crate::sky::days_from_civil(y, m, d) as f64;
        let lat = crate::sky::latitude(&state.setup.theater_code.to_ascii_lowercase());
        Self {
            state,
            mission,
            rules,
            spot_timer: 0.0,
            started: false,
            days,
            lat,
            leftover: Vec::new(),
        }
    }

    /// The simulation for this campaign (the host's; a client's mirror is built the same way).
    pub fn new_sim(&self, data: &stealth_sim::SimData) -> Sim {
        Sim::with_options(data, &self.mission, self.rules.sim_options())
    }

    /// The simulation is made to match the campaign (once, on the first frame).
    pub fn begin(&mut self, sim: &mut Sim) -> Vec<HostEvent> {
        if std::mem::replace(&mut self.started, true) {
            return Vec::new();
        }
        let world = self.state.world_state();
        for &(o, side) in &world.owners {
            sim.set_side(o as usize, side == Side::Blue, side == Side::Neutral);
        }
        for &o in &world.destroyed {
            sim.remove_object(o as usize);
        }
        for &o in &world.intel {
            sim.set_intel(o as usize, true);
        }
        for p in &self.state.places {
            if p.owner != Side::Neutral {
                sim.place_defences(p.x, p.y, p.airfield.is_some(), p.owner == Side::Blue);
            }
        }
        sim.record_shells(true);
        let events = self.state.after_load();
        self.apply(sim, events)
    }

    pub fn started(&self) -> bool {
        self.started
    }

    /// A player joins (a slot and record by name): his id, a pilot of the simulation.
    pub fn join(&mut self, sim: &mut Sim, name: &str) -> PlayerId {
        let id = self.state.join(name);
        sim.add_pilot(id.0);
        id
    }

    /// A player's sortie: a mission of the board from a friendly base (None: the offer's) in
    /// an aircraft with stores (None: the mission's).
    pub fn take_mission(
        &mut self,
        sim: &mut Sim,
        player: PlayerId,
        offer: u32,
        base: Option<u16>,
        aircraft: Aircraft,
        stores: Option<Stores>,
    ) -> Result<Vec<HostEvent>, String> {
        let events = self.state.apply(player, Command::TakeMission { offer, base, aircraft })?;
        let out = self.apply(sim, events);
        for e in &out {
            if let HostEvent::SortieStarted { player, sortie, .. } = e {
                let st = match &stores {
                    Some(s) => s.clone(),
                    None => {
                        let m = glue::sortie_mission(&self.mission, &self.state, sortie);
                        Stores {
                            stations: m
                                .loadout
                                .bays
                                .iter()
                                .take(m.loadout.usable_bays as usize)
                                .map(|b| (b.weapon as i32, b.count as i32))
                                .collect(),
                            select: 0,
                        }
                    }
                };
                sim.set_pilot_loadout(player.0, &st.stations, st.select);
            }
        }
        Ok(out)
    }

    /// A player orders an insertion against an enemy place.
    pub fn order_insertion(
        &mut self,
        sim: &mut Sim,
        player: PlayerId,
        place: PlaceId,
    ) -> Result<Vec<HostEvent>, String> {
        let events = self.state.apply(player, Command::OrderInsertion { place })?;
        Ok(self.apply(sim, events))
    }

    /// A player's sortie ends from outside (aborted, or his flight model's crash), with the
    /// simulation's outcome codes.
    pub fn end_sortie(&mut self, sim: &mut Sim, player: PlayerId, outcome: u8) -> Vec<HostEvent> {
        sim.end_sortie_pilot(player.0, outcome);
        let events = sim.take_pilot_events();
        self.tick(sim, 0.0, &events)
    }

    /// A player landed at a friendly base rearms and refuels.
    pub fn rearm(&mut self, sim: &mut Sim, player: PlayerId, stores: &Stores) {
        sim.rearm_pilot(player.0, &stores.stations);
        sim.set_pilot_loadout(player.0, &stores.stations, stores.select);
        let ev = sim.take_pilot_events();
        self.leftover.extend(ev);
    }

    /// One frame: `dt` campaign seconds and the events of the simulation's steps since the
    /// last one.
    pub fn tick(&mut self, sim: &mut Sim, dt: f64, events: &[PilotEvent]) -> Vec<HostEvent> {
        let mut out = Vec::new();
        let mut reports = Vec::new();
        let mut ended = Vec::new();
        for e in events {
            let player = PlayerId(e.pilot);
            match e.event {
                SimEvent::GroundDestroyed { object, by, .. } if object > 2 => {
                    reports.push(Report::ObjectDestroyed {
                        object: object as u16,
                        by: by.map(PlayerId),
                    });
                }
                SimEvent::CampaignFlightDestroyed { unit, tag, by, .. } => {
                    let u = sim.flights().get(unit).copied().unwrap_or_default();
                    let blue = sim.campaign_flight(unit).is_some_and(|f| f.blue);
                    reports.push(Report::FlightDestroyed {
                        tag,
                        side: side_of(blue),
                        x: u.x,
                        y: u.y,
                        by: by.map(PlayerId),
                    });
                }
                SimEvent::CampaignFlightLanded { tag, x, y, .. } => {
                    reports.push(Report::FlightLanded { tag, x, y })
                }
                SimEvent::ObjectiveDone { primary: true } => {
                    if let Some(Objective::Recon { object, .. }) =
                        self.state.sortie(player).map(|s| &s.offer.objective)
                    {
                        reports.push(Report::PhotoTaken { player, object: *object });
                    }
                }
                SimEvent::AaaDestroyed { unit, by } => {
                    reports.push(Report::AaaDestroyed { index: unit as u32, by: by.map(PlayerId) })
                }
                SimEvent::SortieEnded(end) => ended.push((player, end)),
                SimEvent::LandedAtBase { object } if self.state.sortie(player).is_some() => {
                    out.push(HostEvent::Landed { player, base: object })
                }
                _ => {}
            }
        }
        // Enemy radars friendly aircraft pick up, once a second.
        self.spot_timer += dt;
        if self.spot_timer > SPOT_EVERY {
            self.spot_timer = 0.0;
            reports.extend(glue::spotted(&self.state, sim));
        }
        let view = glue::world_view(sim);
        let events = self.state.tick(dt, &view, &reports);
        out.extend(self.apply(sim, events));
        for (player, e) in ended {
            let (end, title) = glue::sortie_end(&e);
            let started = self.state.sortie(player).map_or(self.state.clock, |s| s.started);
            if let Ok(events) = self.state.apply(player, Command::EndSortie { end }) {
                for ev in events {
                    if let CampaignEvent::SortieFinished { record, .. } = ev {
                        let news = self
                            .state
                            .log
                            .iter()
                            .filter(|l| l.time >= started)
                            .map(|l| l.text.clone())
                            .collect();
                        out.push(HostEvent::SortieFinished {
                            player,
                            debrief: Debrief { record, title: title.into(), news },
                        });
                    }
                }
            }
        }
        if self.rules.continuous_time {
            let hours = self.state.sky_hours();
            sim.set_visual_light(Some(crate::sky::visual_light(
                self.days + (hours / 24.0).floor(),
                hours.rem_euclid(24.0),
                self.lat,
            )));
        } else {
            sim.set_visual_light(None);
        }
        out
    }

    /// The events the host's own calls raised in the simulation (tiles destroyed and
    /// repaired, rearming): the caller hands them on like a step's.
    pub fn take_sim_events(&mut self, sim: &mut Sim) -> Vec<PilotEvent> {
        let mut ev = std::mem::take(&mut self.leftover);
        ev.extend(sim.take_pilot_events());
        ev
    }

    /// Applies the campaign's events to the simulation.
    fn apply(&mut self, sim: &mut Sim, events: Vec<CampaignEvent>) -> Vec<HostEvent> {
        let mut out = Vec::new();
        for e in events {
            match &e {
                CampaignEvent::InsertionLaunched { from, target, tags, .. } => {
                    for (n, &tag) in tags.iter().enumerate() {
                        sim.spawn_insertion(*from as usize, *target as usize, tag, n);
                    }
                }
                CampaignEvent::PlaceCaptured { place, side, members, removed } => {
                    let blue = *side == Side::Blue;
                    for &m in members {
                        sim.restore_object(m as usize);
                        sim.set_side(m as usize, blue, false);
                    }
                    for &r in removed {
                        sim.remove_object(r as usize);
                    }
                    if let Some(p) = self.state.place(*place) {
                        sim.place_defences(p.x, p.y, p.airfield.is_some(), blue);
                    }
                }
                CampaignEvent::Repaired { object } => sim.restore_object(*object as usize),
                CampaignEvent::IntelGained { objects, .. } => {
                    for &o in objects {
                        sim.set_intel(o as usize, true);
                    }
                }
                CampaignEvent::IntelExpired { objects, .. } => {
                    for &o in objects {
                        sim.set_intel(o as usize, false);
                    }
                }
                CampaignEvent::LaunchFighters { tag, side, from, x, y } => {
                    sim.spawn_fighter(*from as usize, *x, *y, *tag, *side == Side::Blue);
                }
                CampaignEvent::SortieStarted { player, sortie } => {
                    let base = sortie.base as usize;
                    let (lockheed, external) = self.rules.airframe(sortie.aircraft);
                    let start = sim.respawn_pilot(
                        player.0,
                        base,
                        glue::sim_objective(sortie),
                        lockheed,
                        external,
                    );
                    out.push(HostEvent::SortieStarted {
                        player: *player,
                        sortie: sortie.clone(),
                        start,
                    });
                    continue;
                }
                _ => {}
            }
            out.push(HostEvent::Campaign(e));
        }
        let ev = sim.take_pilot_events();
        self.leftover.extend(ev);
        out
    }
}
