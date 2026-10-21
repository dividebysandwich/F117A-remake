//! Realtime dynamic campaign: the campaign model (crate stealth-campaign) driving one long
//! flight session with the simulation in campaign mode (`stealth_sim::campaign`).
//!
//! * The 3D world stays loaded for the whole campaign. Between sorties the player is absent
//!   from the simulation and the Ops screen (mission board, war map, debrief, log; `ops.rs`) is
//!   up over the cockpit, parked at a friendly base; the war goes on meanwhile.
//! * A sortie respawns the aircraft on the chosen base's runway with the mission's stores and
//!   route. Crashing, ejecting (once the chute is down) or ending the sortie from the in-flight
//!   menu brings up the debrief and the board again; landing at a friendly base offers to rearm.
//! * Every frame the simulation's events become campaign reports, and the campaign's events
//!   (helicopters to launch, places that change hands, repairs, intelligence, enemy patrols)
//!   are applied to the simulation. The campaign clock is real time; the pause menu does not
//!   stop it. The sky runs `sky_ratio` times faster.
//! * The campaign is saved (`campaigns/<name>.json` in the data directory) every minute and on
//!   leaving. Only the campaign is saved, not the simulation: on loading, what was in the air
//!   starts again.
//!
//! The campaign is driven only through commands and reports with the player's id, the shape a
//! networked coop host would need.

mod menu;
mod ops;
mod picture;
mod save;
pub mod unit_view;

pub use menu::CampaignWindow;
pub use unit_view::{UnitCamera, UnitView};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;
use stealth_campaign::{
    AaaView, CampaignEvent, CampaignSetup, CampaignState, Command, FlightView, MissionKind,
    Objective, PlayerId, PlayerPos, Report, Side, Sortie, SortieEnd, SortieRecord, WorldView,
};
use stealth_mission::{Mission, MissionKind as GenKind, MissionSettings};
use stealth_sim::{Sim, SimEvent, fflags, oflags};

use crate::combat::{Combat, CombatSystems};
use crate::flight::{Aircraft, FlightConstants, FlightEnd, Player, bam};
use crate::hud::{HudMessages, Waypoint, Waypoints};
use crate::mission::{ActiveMission, MissionData, PlannedRoute};
use crate::settings::AppState;
use crate::world::{TileOverrides, WorldPos, wu_to_bevy};

/// The local player (coop players would get the next ids).
pub const ME: PlayerId = PlayerId(0);
/// Seconds the end of a sortie stays in view before the debrief.
const END_SECONDS: f32 = 4.0;
/// Seconds between autosaves.
const SAVE_EVERY: f32 = 60.0;

pub struct CampaignPlugin;

impl Plugin for CampaignPlugin {
    fn build(&self, app: &mut App) {
        let running =
            |c: Option<Res<ActiveCampaign>>, k: Option<Res<Combat>>| c.is_some() && k.is_some();
        app.add_systems(
            Update,
            (begin, tick, ops::block_input)
                .chain()
                .after(CombatSystems)
                .run_if(in_state(AppState::Flight))
                .run_if(running),
        )
        .add_systems(Update, ops::overlay.run_if(in_state(AppState::Flight)))
        .add_systems(
            Update,
            crate::planner::prepare_map
                .run_if(in_state(AppState::Flight))
                .run_if(|c: Option<Res<ActiveCampaign>>| c.is_some_and(|c| c.screen_open())),
        )
        .add_systems(
            EguiPrimaryContextPass,
            ops::ops_ui.run_if(in_state(AppState::Flight)).run_if(running),
        )
        .init_resource::<CampaignWindow>()
        .init_resource::<UnitView>()
        .add_systems(
            Update,
            unit_view::follow
                .after(crate::sim_render::SimRenderSystems)
                .run_if(in_state(AppState::Flight)),
        )
        .add_systems(
            EguiPrimaryContextPass,
            unit_view::window
                .after(ops::ops_ui)
                .run_if(in_state(AppState::Flight))
                .run_if(|c: Option<Res<ActiveCampaign>>| c.is_some_and(|c| c.screen_open())),
        )
        .add_systems(EguiPrimaryContextPass, menu::campaign_window.run_if(in_state(AppState::Menu)))
        .add_systems(OnExit(AppState::Flight), leave);
        #[cfg(feature = "xr")]
        app.add_systems(
            Update,
            unit_view::vr_panel.after(unit_view::follow).run_if(in_state(AppState::Flight)),
        );
    }
}

/// What the player is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// On the Ops screen (absent from the simulation).
    Ops,
    /// Flying a sortie.
    Sortie,
}

/// The last sortie's debrief.
#[derive(Debug, Clone)]
pub struct SortieDebrief {
    pub record: SortieRecord,
    /// How the flight ended.
    pub title: String,
    /// What happened in the campaign meanwhile.
    pub news: Vec<String>,
}

/// The campaign being played (the flight session is its world).
#[derive(Resource)]
pub struct ActiveCampaign {
    pub name: String,
    pub state: CampaignState,
    pub phase: Phase,
    pub ui: ops::OpsUi,
    pub debrief: Option<SortieDebrief>,
    /// Landed and stopped at this friendly base: rearm, or end the sortie.
    pub landed: Option<usize>,
    /// Base the player is parked at between sorties.
    pub parked: Option<usize>,
    /// The campaign's own mission (its world is the theater's), the base of the sorties'.
    pub mission: Mission,
    started: bool,
    save_timer: f32,
    /// Seconds since the sortie ended (the debrief comes up after `END_SECONDS`).
    end_timer: Option<f32>,
    /// Seconds the flight model has ended the flight without the simulation following.
    crash_timer: f32,
    /// Leave the campaign (back to the menu) at the next frame.
    pub exit: bool,
    /// The stores chosen in the briefing for the sortie being taken.
    pub take_loadout: Option<crate::loadout::PlannedLoadout>,
}

impl ActiveCampaign {
    /// A new campaign on a theater; `mission` is any generated mission of it (the theater's
    /// world replaces the generator's).
    pub fn new(name: String, setup: CampaignSetup, data: &MissionData) -> anyhow::Result<Self> {
        let mission = base_mission(&setup, data)?;
        let state = CampaignState::new(setup, &mission.world);
        Ok(Self::with_state(name, state, mission))
    }

    /// A saved campaign.
    pub fn resume(name: String, state: CampaignState, data: &MissionData) -> anyhow::Result<Self> {
        let mission = base_mission(&state.setup, data)?;
        Ok(Self::with_state(name, state, mission))
    }

    fn with_state(name: String, state: CampaignState, mission: Mission) -> Self {
        Self {
            name,
            state,
            phase: Phase::Ops,
            ui: ops::OpsUi::default(),
            debrief: None,
            landed: None,
            parked: None,
            mission,
            started: false,
            save_timer: 0.0,
            end_timer: None,
            crash_timer: 0.0,
            exit: false,
            take_loadout: None,
        }
    }

    /// The Ops screen (or the landing prompt) is up.
    pub fn screen_open(&self) -> bool {
        (self.phase == Phase::Ops && self.end_timer.is_none()) || self.landed.is_some()
    }
}

/// The campaign's mission: a strike mission of the theater (for its settings, briefing date,
/// time and weather) with the theater's own world and no aircraft of its own.
fn base_mission(setup: &CampaignSetup, data: &MissionData) -> anyhow::Result<Mission> {
    let settings = MissionSettings {
        theater: setup.theater,
        roe: setup.roe,
        opponents: setup.opponents,
        kind: GenKind::Strike,
        ..default()
    };
    let mut m = crate::mission::generate(data, &settings, (setup.seed & 0xFFFF) as u16)?;
    let td = data.0.theaters.get(setup.theater).ok_or_else(|| anyhow::anyhow!("no theater"))?;
    m.world = td.world.clone();
    m.drones = None;
    m.time.deadline_ticks = 0;
    m.time.has_deadline = false;
    Ok(m)
}

/// Starts a campaign flight session (from the menu or the command line).
pub fn start(
    commands: &mut Commands,
    campaign: ActiveCampaign,
    setup: &mut crate::settings::MissionSetup,
    start_dir: &[i16],
) {
    let m = campaign.mission.clone();
    *setup = crate::mission::setup_from_mission(&m, start_dir);
    commands.insert_resource(ActiveMission(m.clone()));
    commands.insert_resource(PlannedRoute::of(&m));
    commands.remove_resource::<crate::career::CareerFlight>();
    commands.remove_resource::<crate::loadout::PlannedLoadout>();
    commands.insert_resource(campaign);
}

/// `--campaign <name>`: straight into a saved campaign (or a new one under that name, in
/// `--theater <code>` with `--roe`, `--opponents`, `--sky-ratio` and `--start-hour`).
pub fn from_args(app: &mut App) {
    let args: Vec<String> = std::env::args().collect();
    let get = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let Some(name) = get("--campaign") else { return };
    let data = MissionData::load().expect("mission data");
    let tables = crate::flight::SimTables::load();
    let c = match save::load(&name) {
        Some(state) => ActiveCampaign::resume(name, state, &data),
        None => {
            let code = get("--theater").unwrap_or_else(|| "ce".into()).to_ascii_lowercase();
            let setup = CampaignSetup {
                theater: crate::mission::theater_index(&code).unwrap_or(3),
                theater_code: code,
                roe: get("--roe").and_then(|s| s.parse().ok()).unwrap_or(1),
                opponents: get("--opponents").and_then(|s| s.parse().ok()).unwrap_or(1),
                seed: get("--seed").and_then(|s| s.parse().ok()).unwrap_or_else(rand::random),
                sky_ratio: get("--sky-ratio").and_then(|s| s.parse().ok()).unwrap_or(24.0),
                start_hour: get("--start-hour").and_then(|s| s.parse().ok()).unwrap_or(8.0),
            };
            ActiveCampaign::new(name, setup, &data)
        }
    };
    let c = match c {
        Ok(c) => c,
        Err(e) => {
            warn!("campaign: {e:#}");
            return;
        }
    };
    let m = c.mission.clone();
    let mut setup = crate::mission::setup_from_mission(&m, &tables.start_dir);
    setup.freecam = false;
    app.insert_resource(setup)
        .insert_resource(PlannedRoute::of(&m))
        .insert_resource(ActiveMission(m))
        .insert_resource(data)
        .insert_resource(c)
        .insert_state(AppState::Flight);
}

/// The simulation events the campaign follows (collected by the combat step).
pub fn follows(e: &SimEvent) -> bool {
    matches!(
        e,
        SimEvent::GroundDestroyed { .. }
            | SimEvent::CampaignFlightDestroyed { .. }
            | SimEvent::CampaignFlightLanded { .. }
            | SimEvent::ObjectiveDone { .. }
            | SimEvent::SortieEnded(_)
            | SimEvent::LandedAtBase { .. }
    )
}

fn side_of(blue: bool) -> Side {
    if blue { Side::Blue } else { Side::Red }
}

/// What the simulation shows the campaign.
fn world_view(sim: &Sim) -> WorldView {
    let aaa = sim
        .aaa_units()
        .iter()
        .enumerate()
        .map(|(k, u)| AaaView {
            x: u.x,
            y: u.y,
            side: side_of(sim.aaa_blue(k)),
            alive: !u.destroyed,
        })
        .collect();
    let flights = sim
        .flights()
        .iter()
        .enumerate()
        .filter(|(_, u)| {
            u.flags & (fflags::EXISTS | fflags::AIRBORNE) == fflags::EXISTS | fflags::AIRBORNE
                && u.flags & fflags::DESTROYED == 0
                && u.speed > 0
        })
        .filter_map(|(i, u)| {
            let tag = sim.campaign_flight(i).map_or(0, |f| f.tag);
            let blue =
                sim.campaign_flight(i).map_or_else(|| sim.friendly_aircraft(u.spec), |f| f.blue);
            // The base traffic is no part of the war.
            (tag != 0 || !blue).then_some(FlightView { x: u.x, y: u.y, side: side_of(blue), tag })
        })
        .collect();
    let players = if sim.player_absent() {
        Vec::new()
    } else {
        let p = sim.player();
        vec![PlayerPos { id: ME, x: p.mx, y: p.my, airborne: p.alt > 50 }]
    };
    WorldView { aaa, flights, players }
}

/// How a sortie ended, from the simulation's end record.
fn sortie_end(e: &stealth_sim::MissionEnd) -> (SortieEnd, &'static str) {
    match (e.outcome, e.landing_type) {
        (0, 2) => (SortieEnd::Ejected, "Ejected - picked up by friendly forces"),
        (0, _) => (SortieEnd::Landed, "Sortie ended on the ground"),
        (6, _) => (SortieEnd::Crashed, "Ejection failed - the pilot is recovered"),
        (7, _) => (SortieEnd::Aborted, "Sortie aborted"),
        (5, _) => (SortieEnd::Crashed, "Bad landing"),
        _ => (SortieEnd::Crashed, "Shot down / crashed - the pilot is recovered"),
    }
}

/// The flight session's parts a sortie start or end touches.
#[derive(SystemParam)]
pub struct Session<'w, 's> {
    commands: Commands<'w, 's>,
    combat: ResMut<'w, Combat>,
    player: Query<'w, 's, (&'static mut Aircraft, &'static mut WorldPos), With<Player>>,
    constants: Res<'w, FlightConstants>,
    settings: Res<'w, crate::settings::Settings>,
    wps: ResMut<'w, Waypoints>,
    orders: ResMut<'w, crate::mfd::MissionOrders>,
    kneeboard: ResMut<'w, crate::kneeboard::Kneeboard>,
    msgs: ResMut<'w, HudMessages>,
    tiles: ResMut<'w, TileOverrides>,
    theater: Option<ResMut<'w, crate::world::Theater>>,
    view: ResMut<'w, crate::camera::ViewState>,
    /// The campaign clock: virtual time (never paused in a campaign; `--time-scale` speeds the
    /// whole war up for tests).
    time: Res<'w, Time>,
    menu: ResMut<'w, crate::flight_menu::FlightMenu>,
}

/// First frame of the session: the simulation is made to match the campaign.
fn begin(mut c: ResMut<ActiveCampaign>, mut s: Session) {
    if c.started {
        return;
    }
    c.started = true;
    let c = &mut *c;
    let sim = &mut s.combat.sim;
    let world = c.state.world_state();
    for &(o, side) in &world.owners {
        sim.set_side(o as usize, side == Side::Blue, side == Side::Neutral);
    }
    for &o in &world.destroyed {
        sim.remove_object(o as usize);
    }
    for &o in &world.intel {
        sim.set_intel(o as usize, true);
    }
    for p in &c.state.places {
        if p.owner != Side::Neutral {
            sim.place_defences(p.x, p.y, p.airfield.is_some(), p.owner == Side::Blue);
        }
    }
    if let Some(t) = s.theater.as_deref_mut() {
        mark_friendly(t, &c.state);
    }
    let events = c.state.after_load();
    apply(c, &mut s, events);
    drain_sim(&mut s);
    let base = default_base(&c.state);
    park(c, &mut s, base);
    c.ui.tab = ops::test_tab().unwrap_or(ops::Tab::Board);
    // Test aid: `--campaign-insert` orders an insertion against the nearest enemy place.
    if std::env::args().any(|a| a == "--campaign-insert")
        && let Some(&place) = c.state.front(Side::Blue).first()
    {
        match c.state.apply(ME, Command::OrderInsertion { place }) {
            Ok(ev) => apply(c, &mut s, ev),
            Err(e) => warn!("--campaign-insert: {e}"),
        }
    }
}

/// The friendly side's base nearest the front (where the player waits between sorties).
fn default_base(state: &CampaignState) -> Option<usize> {
    let front = state.front(Side::Blue);
    let (x, y) =
        front.first().and_then(|&p| state.place(p)).map_or((0x4000, 0x4000), |p| (p.x, p.y));
    state.nearest_base(Side::Blue, x, y).map(|b| b as usize)
}

/// The flight model's landing corridors follow the campaign's owners (friendly bases are the
/// ones it lands on).
fn mark_friendly(t: &mut crate::world::Theater, state: &CampaignState) {
    for s in &state.sites {
        let Some(o) = t.world.objects.get_mut(s.object as usize) else { continue };
        if (o.x as i32, o.y as i32) != (s.x, s.y) {
            continue;
        }
        let mut f = o.target_flags as u16 & !0x500;
        match state.owner(s.object) {
            Some(Side::Blue) => f |= 0x400,
            Some(Side::Neutral) => f |= 0x100,
            _ => {}
        }
        o.target_flags = f as i16;
    }
}

/// Events the campaign's calls raised in the simulation (outside its steps): tiles destroyed
/// and repaired.
fn drain_sim(s: &mut Session) {
    for e in s.combat.sim.take_events() {
        match e {
            SimEvent::TileDestroyed(t) => {
                s.tiles.set(t.level as usize, t.col, t.row, t.index, t.shape)
            }
            SimEvent::TileRestored(t) => s.tiles.clear(t.level as usize, t.col, t.row, t.index),
            _ => {}
        }
    }
}

/// The player waits at a base (absent from the simulation; the aircraft stands on the runway).
fn park(c: &mut ActiveCampaign, s: &mut Session, base: Option<usize>) {
    c.phase = Phase::Ops;
    c.landed = None;
    c.end_timer = None;
    c.crash_timer = 0.0;
    let base = base.or(c.parked);
    c.parked = base;
    s.combat.sim.end_sortie(7);
    s.combat.campaign_events.clear();
    if let Some(b) = base {
        let st = s.combat.sim.start_state_at(b);
        place_player(s, &st, b);
    }
    s.view.view = crate::camera::View::Cockpit;
}

/// Puts the aircraft at a start state (fresh, on the ground, engines idle).
fn place_player(s: &mut Session, st: &stealth_sim::PlayerState, base: usize) {
    let carrier = s.combat.sim.objects().get(base).is_some_and(|o| o.flags & oflags::CARRIER != 0);
    let fuel = s.constants.fuel;
    let Ok((mut a, mut pos)) = s.player.single_mut() else { return };
    let p = wu_to_bevy(st.x as f64, st.y as f64, if carrier { 128.0 } else { 0.0 });
    *a = Aircraft::new_on_runway(fuel);
    a.altitude = (p.y / crate::world::WU) as f32;
    a.orientation = Aircraft::from_euler(bam(st.heading as f32), 0.0, 0.0);
    pos.0 = p;
}

/// One frame of the campaign.
fn tick(mut c: ResMut<ActiveCampaign>, mut s: Session) {
    let c = &mut *c;
    let dt = s.time.delta_secs();
    if c.exit {
        return;
    }
    // END SORTIE from the in-flight menu.
    if std::mem::take(&mut s.combat.quit) && c.phase == Phase::Sortie {
        s.combat.sim.end_sortie(7);
        for e in s.combat.sim.take_events() {
            s.combat.campaign_events.push(e);
        }
    }
    // The flight model ended the flight (its ground model differs in detail) and the
    // simulation did not follow: the sortie ends anyway.
    if c.phase == Phase::Sortie && c.end_timer.is_none() && !s.combat.sim.player_absent() {
        let ended = s.player.single().ok().and_then(|(a, _)| a.ended);
        match ended {
            Some(FlightEnd::Crash | FlightEnd::BadLanding) if s.combat.sim.chute().is_none() => {
                c.crash_timer += dt;
                if c.crash_timer > 3.0 {
                    let outcome = if ended == Some(FlightEnd::BadLanding) { 5 } else { 1 };
                    s.combat.sim.end_sortie(outcome);
                    let ev = s.combat.sim.take_events();
                    s.combat.campaign_events.extend(ev);
                }
            }
            // Stopped on a runway the simulation does not count as a landing: the prompt.
            Some(FlightEnd::Landed) if c.landed.is_none() => {
                c.crash_timer += dt;
                if c.crash_timer > 3.0 {
                    let p = s.combat.sim.player();
                    c.landed = c.state.nearest_base(Side::Blue, p.mx, p.my).map(|b| b as usize);
                }
            }
            _ => c.crash_timer = 0.0,
        }
    }
    // Test aid: take a mission of the board at once.
    if c.phase == Phase::Ops
        && c.state.clock > 2.0
        && let Some(n) = ops::test_take()
        && !std::mem::replace(&mut c.ui.test_taken, true)
    {
        let flown: Vec<u32> =
            c.state.board.iter().filter(|o| !o.kind.ai_flown()).map(|o| o.id).collect();
        if let Some(&offer) = flown.get(n.min(flown.len().saturating_sub(1))) {
            match c.state.apply(ME, Command::TakeMission { offer, base: None }) {
                Ok(ev) => apply(c, &mut s, ev),
                Err(e) => warn!("--campaign-take: {e}"),
            }
        }
    }
    // The simulation's events as reports.
    let sortie = c.state.sortie(ME).cloned();
    let mut reports = Vec::new();
    let mut ended = None;
    for e in std::mem::take(&mut s.combat.campaign_events) {
        match e {
            SimEvent::GroundDestroyed { object, .. } if object > 2 => {
                let by = (c.phase == Phase::Sortie).then_some(ME);
                reports.push(Report::ObjectDestroyed { object: object as u16, by });
            }
            SimEvent::CampaignFlightDestroyed { unit, tag, by_player } => {
                let u = s.combat.sim.flights().get(unit).copied().unwrap_or_default();
                let blue = s.combat.sim.campaign_flight(unit).is_some_and(|f| f.blue);
                reports.push(Report::FlightDestroyed {
                    tag,
                    side: side_of(blue),
                    x: u.x,
                    y: u.y,
                    by: by_player.then_some(ME),
                });
            }
            SimEvent::CampaignFlightLanded { tag, x, y, .. } => {
                reports.push(Report::FlightLanded { tag, x, y })
            }
            SimEvent::ObjectiveDone { primary: true } => {
                if let Some(Objective::Recon { object, .. }) =
                    sortie.as_ref().map(|s| &s.offer.objective)
                {
                    reports.push(Report::PhotoTaken { player: ME, object: *object });
                }
            }
            SimEvent::SortieEnded(e) => ended = Some(e),
            SimEvent::LandedAtBase { object } if c.phase == Phase::Sortie => {
                c.landed = Some(object);
                s.msgs.show("Landed - rearm or end the sortie");
            }
            _ => {}
        }
    }
    for k in std::mem::take(&mut s.combat.aaa_killed) {
        let by = (c.phase == Phase::Sortie).then_some(ME);
        reports.push(Report::AaaDestroyed { index: k as u32, by });
    }
    let view = world_view(&s.combat.sim);
    let events = c.state.tick(dt as f64, &view, &reports);
    apply(c, &mut s, events);
    if let Some(e) = ended
        && c.phase == Phase::Sortie
    {
        let (end, title) = sortie_end(&e);
        let started = c.state.sortie(ME).map_or(c.state.clock, |s| s.started);
        if let Ok(events) = c.state.apply(ME, Command::EndSortie { end }) {
            for ev in events {
                if let CampaignEvent::SortieFinished { record, .. } = ev {
                    let news = c
                        .state
                        .log
                        .iter()
                        .filter(|l| l.time >= started)
                        .map(|l| l.text.clone())
                        .collect();
                    c.debrief = Some(SortieDebrief { record, title: title.into(), news });
                }
            }
        }
        c.landed = None;
        c.end_timer = Some(0.0);
        s.msgs.show(title);
    }
    drain_sim(&mut s);
    // The debrief comes up a few seconds after the end (the chute is down by then).
    if let Some(t) = c.end_timer.as_mut() {
        *t += dt;
        if *t > END_SECONDS {
            let base = c.parked;
            park(c, &mut s, base);
            c.ui.tab = ops::Tab::Debrief;
        }
    }
    // The sky's clock.
    let day = 86_400.0 * 30.0;
    s.combat.sky_seconds = Some(((c.state.sky_hours() * 3600.0) % day) as f32);
    // Saved now and then.
    c.save_timer += dt;
    if c.save_timer > SAVE_EVERY {
        c.save_timer = 0.0;
        save::save(&c.name, &c.state);
    }
}

/// Applies the campaign's events to the simulation and the cockpit.
fn apply(c: &mut ActiveCampaign, s: &mut Session, events: Vec<CampaignEvent>) {
    for e in events {
        let sim = &mut s.combat.sim;
        match e {
            CampaignEvent::InsertionLaunched { from, target, tags, .. } => {
                for (n, tag) in tags.into_iter().enumerate() {
                    sim.spawn_insertion(from as usize, target as usize, tag, n);
                }
            }
            CampaignEvent::PlaceCaptured { place, side, members, removed } => {
                let blue = side == Side::Blue;
                for &m in &members {
                    sim.restore_object(m as usize);
                    sim.set_side(m as usize, blue, false);
                }
                for &r in &removed {
                    sim.remove_object(r as usize);
                }
                if let Some(p) = c.state.place(place) {
                    sim.place_defences(p.x, p.y, p.airfield.is_some(), blue);
                }
                if let Some(t) = s.theater.as_deref_mut() {
                    mark_friendly(t, &c.state);
                }
            }
            CampaignEvent::Repaired { object } => sim.restore_object(object as usize),
            CampaignEvent::IntelGained { objects, .. } => {
                for o in objects {
                    sim.set_intel(o as usize, true);
                }
            }
            CampaignEvent::IntelExpired { objects, .. } => {
                for o in objects {
                    sim.set_intel(o as usize, false);
                }
            }
            CampaignEvent::LaunchFighters { tag, side, from, x, y } => {
                sim.spawn_fighter(from as usize, x, y, tag, side == Side::Blue);
            }
            CampaignEvent::Message(text) => {
                info!("campaign: {text}");
                s.msgs.show(text)
            }
            CampaignEvent::ObjectiveAchieved { player, .. } if player == ME => {
                s.msgs.show("Objective achieved");
            }
            CampaignEvent::SortieStarted { player, sortie } if player == ME => {
                begin_sortie(c, s, &sortie);
            }
            _ => {}
        }
    }
}

/// A sortie starts: the aircraft on the base's runway with the mission's stores and route.
fn begin_sortie(c: &mut ActiveCampaign, s: &mut Session, sortie: &Sortie) {
    let objective = match sortie.offer.objective {
        Objective::Recon { object, .. } => Some((1, object as usize)),
        Objective::Strike { airfield, .. } => Some((2, airfield as usize)),
        Objective::AntiShip { ship } => Some((2, ship as usize)),
        Objective::Sead { ref sites, .. } => sites.first().map(|&o| (2, o as usize)),
        _ => None,
    };
    let base = sortie.base as usize;
    let st = s.combat.sim.respawn_player(base, objective);
    let m = sortie_mission(&c.mission, &c.state, sortie);
    let aircraft = s.settings.aircraft;
    let loadout = c
        .take_loadout
        .take()
        .filter(|l| l.fits(&m, aircraft))
        .unwrap_or_else(|| crate::loadout::PlannedLoadout::of(&m, aircraft));
    s.combat.sim.set_loadout(&loadout.stations());
    s.combat.sim.select_bay(loadout.first_choice(&m));
    let route = PlannedRoute::of(&m);
    *s.wps = Waypoints {
        points: route
            .points
            .iter()
            .map(|p| Waypoint { east: p.x as f64 * 32.0, north: (0x8000 - p.y) as f64 * 32.0 })
            .collect(),
        current: 0,
        mission_current: 0,
        anchors: route.anchors(),
    };
    s.orders.lines =
        m.orders.lines.iter().map(|l| (l.color, crate::mission::clean_text(&l.text))).collect();
    s.commands.insert_resource(route);
    s.commands.insert_resource(loadout);
    s.commands.insert_resource(ActiveMission(m));
    s.kneeboard.new_mission();
    s.combat.record = default();
    s.combat.campaign_events.clear();
    place_player(s, &st, base);
    s.view.view = crate::camera::View::Cockpit;
    s.menu.open = false;
    c.phase = Phase::Sortie;
    c.parked = Some(base);
    c.landed = None;
    c.end_timer = None;
    c.crash_timer = 0.0;
    s.msgs.show(sortie.offer.title.clone());
}

/// Stores for a kind of mission (weapon table indices, the generator's bays).
fn stores(kind: MissionKind) -> [i16; 4] {
    match kind {
        MissionKind::Recon => [16, 0, 2, 0],
        MissionKind::Sead => [2, 2, 5, 0],
        MissionKind::Cas => [5, 7, 5, 0],
        MissionKind::Strike => [6, 8, 6, 0],
        MissionKind::AntiShip => [4, 3, 5, 0],
        MissionKind::Cap | MissionKind::Escort | MissionKind::Insertion => [1, 0, 1, 0],
    }
}

/// The sortie as a mission (briefing, kneeboard, route, stores): from the base to the target
/// and back.
fn sortie_mission(base: &Mission, state: &CampaignState, sortie: &Sortie) -> Mission {
    use stealth_mission::{Base, Bay, MapPoint, OrdersLine};
    let mut m = base.clone();
    let o = &sortie.offer;
    let b = state.site(sortie.base);
    let (bx, by) = b.map_or((0x4000, 0x4000), |s| (s.x, s.y));
    let home = Base {
        object_index: sortie.base as i16,
        name: b.map(|s| s.name.clone()).unwrap_or_default(),
        position: MapPoint { x: bx, y: by },
        carrier: b.is_some_and(|s| s.kind == stealth_campaign::SiteKind::Carrier),
        neutral: false,
        distance: 0,
    };
    m.takeoff = home.clone();
    m.landing = home;
    let target = MapPoint { x: o.x, y: o.y };
    let mid = MapPoint { x: (bx + o.x) / 2, y: (by + o.y) / 2 };
    m.waypoints = [mid, target, target, MapPoint { x: bx, y: by }];
    m.primary.place_name = o.title.clone();
    m.primary.position = target;
    m.primary.mission_type = if o.kind == MissionKind::Recon { 1 } else { 2 };
    m.primary.type_name = o.kind.name().into();
    m.secondary.position = target;
    m.secondary.mission_type = 0;
    let what = describe(o.kind);
    m.briefing.title = o.title.clone();
    m.briefing.primary_objective = what.to_string();
    m.briefing.secondary_objective = String::new();
    m.orders.primary = o.title.to_ascii_uppercase();
    m.orders.secondary = String::new();
    m.orders.return_to = format!("RETURN TO {}", m.landing.name.to_ascii_uppercase());
    m.orders.lines = vec![
        OrdersLine { text: o.title.clone(), color: 15 },
        OrdersLine { text: what.into(), color: 11 },
        OrdersLine { text: m.orders.return_to.clone(), color: 11 },
    ];
    m.loadout.bays =
        stores(o.kind).iter().map(|&w| Bay { weapon: w, name: String::new(), count: 1 }).collect();
    m.loadout.usable_bays =
        if m.settings.plane == stealth_mission::Plane::Lockheed { 2 } else { 4 };
    m
}

/// What a kind of mission asks the pilot for.
pub fn describe(kind: MissionKind) -> &'static str {
    match kind {
        MissionKind::Recon => {
            "Photograph the target: its defences are then known (spotted from twice the range, shorter radar range) for an hour"
        }
        MissionKind::Sead => "Destroy the SAM sites and radars defending the place",
        MissionKind::Cap => {
            "Hold the air over the point for 10 minutes or shoot down two enemy aircraft"
        }
        MissionKind::Cas => "Destroy the guns and MANPADS teams around the place",
        MissionKind::AntiShip => "Sink the enemy ship",
        MissionKind::Strike => {
            "Destroy the airfield: no enemy aircraft take off from it until it is repaired"
        }
        MissionKind::Escort => "Cover the insertion helicopters until they land",
        MissionKind::Insertion => {
            "Helicopters fly troops in from the nearest friendly base; one landing takes the place"
        }
    }
}

/// Leaving the flight session: the campaign is saved and closed, and the Ops screen's egui
/// context goes with it (the menu brings its own).
fn leave(
    mut commands: Commands,
    c: Option<Res<ActiveCampaign>>,
    cams: Query<Entity, With<ops::OpsCamera>>,
    mut view: ResMut<UnitView>,
    mut sites: ResMut<crate::world::StreamSites>,
) {
    for e in &cams {
        commands.entity(e).try_despawn();
    }
    view.close();
    unit_view::follow_cleanup(&mut commands, &mut view);
    sites.follow = None;
    #[cfg(feature = "xr")]
    commands.remove_resource::<crate::vr_menu::WorldPanel>();
    if let Some(c) = c {
        save::save(&c.name, &c.state);
        commands.remove_resource::<ActiveCampaign>();
    }
}
