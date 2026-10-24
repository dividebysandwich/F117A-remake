//! The campaign's authority: the campaign model driving one simulation for all its players
//! (the single-player game, a listen server or a dedicated server).
//!
//! The host does not own the simulation (the game keeps it with its renderer): every call
//! takes it. Every frame the simulation's events become campaign reports, and the campaign's
//! events are applied to the simulation. What the players have to know comes back as
//! [`HostEvent`]s; what the host's calls raised in the simulation itself (tiles destroyed and
//! repaired, rearming) is in [`Host::take_sim_events`].

use serde::{Deserialize, Serialize};
use stealth_campaign::weather::Forecast;
use stealth_campaign::{
    Aircraft, CampaignEvent, CampaignSetup, CampaignState, Command, FighterTask, Objective,
    PlaceId, PlayerId, Report, RoutePoint, Side, Sortie, SortieRecord,
};
use stealth_mission::{GameData, Mission};
use stealth_sim::{PilotEvent, PlayerState, Sim, SimEvent, oflags};

use crate::glue::{self, side_of};
use crate::rules::Rules;

/// Seconds between the friendly aircraft's looks for enemy radars.
const SPOT_EVERY: f64 = 1.0;
/// The take-off formation of coop players on a runway (world units, ~0.49 m): the step back
/// between the rows and the columns' offset left and right of the centre line.
const STEP_BACK: i32 = 0x80;
const STEP_SIDE: i32 = 20;
/// Another aircraft this close to a place takes it.
const PLACE_TAKEN: i32 = 30;

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
    /// The changing weather (none under a clear sky).
    forecast: Option<Forecast>,
    /// Events the host's own calls took from the simulation and did not use.
    leftover: Vec<PilotEvent>,
    /// Places with a Patriot battery placed in the simulation.
    patriots: Vec<PlaceId>,
}

impl Host {
    /// A new campaign under `rules` (kept with it).
    pub fn new(setup: CampaignSetup, data: &GameData, rules: Rules) -> anyhow::Result<Self> {
        let mission = glue::base_mission(&setup, data)?;
        let mut state = CampaignState::new(setup, &mission.world);
        state.rules = Some(rules.into());
        state.terrain = glue::terrain(&state.setup, data);
        Ok(Self::with_state(state, mission, rules))
    }

    /// A saved campaign, under the rules it was started with (an older save without them
    /// takes `rules` from now on).
    pub fn resume(mut state: CampaignState, data: &GameData, rules: Rules) -> anyhow::Result<Self> {
        let mission = glue::base_mission(&state.setup, data)?;
        let rules = state.rules.map_or(rules, Rules::from);
        state.rules = Some(rules.into());
        state.terrain = glue::terrain(&state.setup, data);
        Ok(Self::with_state(state, mission, rules))
    }

    fn with_state(state: CampaignState, mission: Mission, rules: Rules) -> Self {
        let (y, m, d) = crate::sky::briefing_date(&mission.briefing.date);
        let days = crate::sky::days_from_civil(y, m, d) as f64;
        let lat = crate::sky::latitude(&state.setup.theater_code.to_ascii_lowercase());
        let forecast = Forecast::new(&state.setup);
        Self {
            state,
            mission,
            rules,
            spot_timer: 0.0,
            started: false,
            days,
            lat,
            forecast,
            leftover: Vec::new(),
            patriots: Vec::new(),
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
                let blue = p.owner == Side::Blue;
                sim.place_defences(p.x, p.y, p.airfield.is_some(), blue);
                // The guns it lost stay lost until the campaign replaces them.
                sim.wreck_defences(p.x, p.y, blue, p.guns_lost as usize);
            }
        }
        self.place_patriots(sim);
        for b in &self.state.batteries {
            sim.place_battery(b.x, b.y);
        }
        sim.record_shells(true);
        let events = self.state.after_load();
        self.apply(sim, events)
    }

    /// Patriot batteries at the friendly airfields in the rear that have none yet (batteries
    /// stay where they are when the front moves on).
    fn place_patriots(&mut self, sim: &mut Sim) {
        for (place, x, y) in self.state.patriot_sites() {
            if !self.patriots.contains(&place) {
                self.patriots.push(place);
                sim.place_patriot(x, y);
            }
        }
    }

    pub fn started(&self) -> bool {
        self.started
    }

    /// A player joins (a slot and record by name): his id, a pilot of the simulation. None:
    /// every slot is taken by someone `busy` or flying.
    pub fn join(
        &mut self,
        sim: &mut Sim,
        name: &str,
        busy: impl Fn(PlayerId) -> bool,
    ) -> Option<PlayerId> {
        let id = self.state.join(name, stealth_sim::MAX_PILOTS, busy)?;
        sim.add_pilot(id.0);
        Some(id)
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

    /// A player calls in AI SEAD for an offer's place.
    pub fn request_support(
        &mut self,
        sim: &mut Sim,
        player: PlayerId,
        offer: u32,
    ) -> Result<Vec<HostEvent>, String> {
        let events = self.state.apply(player, Command::RequestSupport { offer })?;
        Ok(self.apply(sim, events))
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

    /// A player joins the plan of an offer from a base, or leaves the plans (None).
    pub fn select(
        &mut self,
        sim: &mut Sim,
        player: PlayerId,
        offer: Option<u32>,
        base: u16,
    ) -> Result<Vec<HostEvent>, String> {
        let events = self.state.apply(player, Command::Select { offer, base })?;
        Ok(self.apply(sim, events))
    }

    /// A player changes the base and route of the plan he is on.
    pub fn edit_plan(
        &mut self,
        sim: &mut Sim,
        player: PlayerId,
        offer: u32,
        base: u16,
        route: Vec<RoutePoint>,
    ) -> Result<Vec<HostEvent>, String> {
        let events = self.state.apply(player, Command::Plan { offer, base, route })?;
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
                    if let CampaignEvent::PlansChanged(_) = ev {
                        out.push(HostEvent::Campaign(ev));
                    } else if let CampaignEvent::SortieFinished { record, .. } = ev {
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
            sim.set_visual_light(Some(crate::sky::visual_light(self.days, hours, self.lat)));
        } else {
            sim.set_visual_light(None);
        }
        if let Some(f) = &mut self.forecast {
            sim.set_weather(f.at(self.state.clock).weather);
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
                CampaignEvent::InsertionLaunched { from, target, tags, side, .. } => {
                    let blue = *side == Side::Blue;
                    for (n, &tag) in tags.iter().enumerate() {
                        sim.spawn_insertion_for(*from as usize, *target as usize, tag, n, blue);
                    }
                }
                CampaignEvent::RaidLaunched { from, target, tags, side, drones, .. } => {
                    let (from, target) = (*from as usize, *target as usize);
                    let blue = *side == Side::Blue;
                    for &tag in tags {
                        if *drones {
                            sim.spawn_drone(from, target, tag, blue);
                        } else {
                            sim.spawn_striker(from, target, tag, blue);
                        }
                    }
                }
                CampaignEvent::LaunchAwacs { tag, side, from, x, y } => {
                    sim.spawn_awacs(*from as usize, *x, *y, *tag, *side == Side::Blue);
                }
                CampaignEvent::Recall { tag } => sim.recall_flight(*tag),
                CampaignEvent::BatteryDeployed { x, y, .. } => sim.place_battery(*x, *y),
                CampaignEvent::GunRepaired { place } => {
                    if let Some(p) = self.state.place(*place) {
                        sim.restore_defence(p.x, p.y, p.owner == Side::Blue);
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
                    // A battery at a place the enemy took is gone with its other defences.
                    if !blue {
                        self.patriots.retain(|p| p != place);
                    }
                    // The front moved: airfields now in the rear may get a battery.
                    self.place_patriots(sim);
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
                CampaignEvent::LaunchFighters { tag, side, from, x, y, task } => {
                    let task = match task {
                        FighterTask::Patrol => stealth_sim::ai_loadout::FighterTask::Patrol,
                        FighterTask::Cap => stealth_sim::ai_loadout::FighterTask::Cap,
                    };
                    sim.spawn_fighter(*from as usize, *x, *y, *tag, *side == Side::Blue, task);
                }
                CampaignEvent::SortieStarted { player, sortie } => {
                    let base = sortie.base as usize;
                    let airframe = self.rules.airframe(sortie.aircraft);
                    let mut start =
                        sim.respawn_pilot(player.0, base, glue::sim_objective(sortie), airframe);
                    // Coop players taking off from the same base: a staggered formation on
                    // the runway, the first free place.
                    let carrier =
                        sim.objects().get(base).is_some_and(|o| o.flags & oflags::CARRIER != 0);
                    if !carrier {
                        start = formation_place(sim, player.0, &start);
                        sim.set_pilot_input(player.0, &start);
                    }
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

/// Places of the take-off formation on the runway.
const FORMATION: usize = 8;

/// The places of the take-off formation, relative to the runway start (along the runway,
/// to its right): the lead in the middle, far enough down the runway for the others; behind
/// him rows of two, left and right of the centre line, the right one a little behind the
/// left one, each row a step further back.
pub fn formation(k: usize) -> (i32, i32) {
    let lead = 3 * STEP_BACK;
    if k == 0 {
        return (lead, 0);
    }
    let row = (k as i32 + 1) / 2;
    let right = k % 2 == 0;
    let along = lead - row * STEP_BACK - if right { STEP_BACK / 3 } else { 0 };
    (along, if right { STEP_SIDE } else { -STEP_SIDE })
}

/// Where pilot `id` starts: the first place of the formation nobody stands on.
fn formation_place(sim: &Sim, id: u8, start: &PlayerState) -> PlayerState {
    // Heading north (0) or south (0x8000): along the runway and to its right.
    let dir = if start.heading == 0 { 1 } else { -1 };
    let at = |(along, right): (i32, i32)| (start.x + dir * right, start.y + dir * along);
    let others: Vec<(i32, i32)> = (0..sim.pilot_count() as u8)
        .filter(|&k| k != id)
        .filter_map(|k| sim.pilot_view(k))
        .map(|v| (v.wx, v.wy))
        .collect();
    let room = |(x, y): (i32, i32)| {
        others.iter().map(|&(ox, oy)| (ox - x).abs().max((oy - y).abs())).min().unwrap_or(i32::MAX)
    };
    let places = || (0..FORMATION).map(formation).map(at);
    // All places taken (a crowd at one base): the one with the most room.
    let (x, y) = places()
        .find(|&p| room(p) > PLACE_TAKEN)
        .or_else(|| places().max_by_key(|&p| room(p)))
        .expect("the formation has places");
    PlayerState { x, y, ..start.clone() }
}

#[cfg(test)]
mod tests {
    use super::formation;

    #[test]
    fn the_formation_is_staggered() {
        assert_eq!(formation(0).1, 0, "the lead in the middle");
        // Rows of two, left then right a little behind it, each row further back.
        for row in 0..3 {
            let (l, r) = (formation(1 + 2 * row), formation(2 + 2 * row));
            assert!(l.1 < 0 && r.1 > 0 && r.0 < l.0, "{l:?} {r:?}");
            assert!(formation(2 * row + 3).0 < r.0);
        }
        assert!(formation(0).0 > formation(1).0);
        // Never wider than the two columns.
        assert!((0..8).all(|k| formation(k).1.abs() <= formation(1).1.abs()));
        let all: Vec<_> = (0..8).map(formation).collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!((a.0 - b.0).abs().max((a.1 - b.1).abs()) > 20, "{a:?} {b:?}");
            }
        }
    }
}
