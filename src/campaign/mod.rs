//! Realtime dynamic campaign: the campaign model (crate stealth-campaign) driving one long
//! flight session with the simulation in campaign mode (`stealth_sim::campaign`), through the
//! campaign host (crate stealth-host).
//!
//! * The 3D world stays loaded for the whole campaign. Between sorties the player is absent
//!   from the simulation and the Ops screen (mission board, war map, players, debrief, log;
//!   `ops.rs`) is up over the cockpit, parked at a friendly base; the war goes on meanwhile.
//! * A sortie respawns the aircraft on the chosen base's runway with the mission's stores and
//!   route, in the aircraft chosen in the briefing. Crashing, ejecting (once the chute is down)
//!   or ending the sortie from the in-flight menu brings up the debrief and the board again;
//!   landing at a friendly base offers to rearm.
//! * The campaign clock is real time; the pause menu does not stop it. The sky runs
//!   `sky_ratio` times faster.
//!
//! Who runs the war ([`Role`]):
//! * Alone (`Local`) and as a listen server (`Host`): the game holds the [`Host`]; it is saved
//!   (`campaigns/<name>.json` in the data directory) every minute and on leaving. Only the
//!   campaign is saved, not the simulation: on loading, what was in the air starts again. A
//!   listen server also serves the players who joined (`crate::net`).
//! * A coop client (`Client`): the server runs the war; the game shows its copy of the
//!   campaign, sends the player's commands and flies his aircraft.
//!
//! The Ops screen asks for things through [`Action`]s, carried out here for every role.

mod menu;
mod ops;
mod picture;
pub mod unit_view;

pub use menu::CampaignWindow;
pub use unit_view::{UnitCamera, UnitView};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;
use stealth_campaign::{
    Aircraft as Airframe, CampaignEvent, CampaignSetup, CampaignState, PlaceId, PlayerId, Side,
    Sortie,
};
use stealth_host::{Debrief, Host, HostEvent, Rules, Stores, save};
use stealth_mission::Mission;
use stealth_net::protocol::{PlayerInfo, PlayerStatus, ToServer};
use stealth_sim::{PlayerState, SimEvent, oflags};

pub use stealth_host::glue::describe;
use stealth_host::glue::sortie_mission;

use crate::combat::{Combat, CombatSystems};
use crate::flight::{Aircraft, FlightConstants, FlightEnd, Player, bam};
use crate::hud::{HudMessages, Waypoint, Waypoints};
use crate::mission::{ActiveMission, MissionData, PlannedRoute};
use crate::net::{NetInbox, NetLink};
use crate::settings::AppState;
use crate::world::{TileOverrides, WorldPos, wu_to_bevy};

/// The local player alone or on a listen server (a client gets his id from the server).
pub const HOST_PLAYER: PlayerId = PlayerId(0);
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
        // `--open-campaign`: start in the campaign window (screenshots).
        .insert_resource(CampaignWindow::opened(std::env::args().any(|a| a == "--open-campaign")))
        .init_resource::<UnitView>()
        .init_resource::<crate::camera::HideOwnAircraft>()
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

/// Who runs the war.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Alone.
    Local,
    /// A listen server: the war is run here, for the players who joined too.
    Host,
    /// A coop client: the server runs the war.
    Client,
}

/// What the Ops screen asks for.
#[derive(Debug, Clone)]
pub enum Action {
    TakeMission {
        offer: u32,
        base: Option<u16>,
        aircraft: Airframe,
        loadout: crate::loadout::PlannedLoadout,
        route: Option<PlannedRoute>,
    },
    OrderInsertion {
        place: PlaceId,
    },
    /// Call in AI SEAD for an offer's place.
    RequestSupport {
        offer: u32,
    },
    /// The sortie ends (the simulation's outcome codes: 0 landed and done, 7 aborted).
    EndSortie {
        outcome: u8,
    },
    Rearm {
        loadout: crate::loadout::PlannedLoadout,
    },
    /// Plan a mission of the board with the other players on it (None: plan none).
    SelectMission {
        offer: Option<u32>,
        base: u16,
    },
    /// The plan's base and route changed.
    EditPlan {
        offer: u32,
        base: u16,
        route: Vec<crate::mission::RoutePoint>,
    },
}

/// The last sortie's debrief.
pub type SortieDebrief = Debrief;

/// The campaign being played (the flight session is its world).
#[derive(Resource)]
pub struct ActiveCampaign {
    pub name: String,
    pub role: Role,
    /// The war's authority (alone, listen server); None on a client.
    pub(super) host: Option<Host>,
    /// A client's copy of the server's campaign.
    pub(super) copy: Option<CampaignState>,
    /// The campaign's own mission (its world is the theater's), the base of the sorties'.
    pub mission: Mission,
    pub rules: Rules,
    /// This player.
    pub me: PlayerId,
    /// The server's name (a client's).
    pub server: Option<String>,
    pub phase: Phase,
    pub ui: ops::OpsUi,
    pub debrief: Option<SortieDebrief>,
    /// Landed and stopped at this friendly base: rearm, or end the sortie.
    pub landed: Option<usize>,
    /// Base the player is parked at between sorties.
    pub parked: Option<usize>,
    /// The aircraft of the next sortie (the briefing's choice).
    pub aircraft: Airframe,
    /// The players (a coop game's).
    pub roster: Vec<PlayerInfo>,
    /// Asked for by the Ops screen, carried out next frame.
    pub actions: Vec<Action>,
    /// A returning client's sortie goes on (from the server's welcome).
    pub resume: Option<stealth_net::protocol::Resume>,
    started: bool,
    save_timer: f32,
    /// Seconds since the sortie ended (the debrief comes up after `END_SECONDS`).
    end_timer: Option<f32>,
    /// Seconds the flight model has ended the flight without the simulation following (an
    /// end asked for: negative).
    crash_timer: f32,
    /// Leave the campaign (back to the menu) at the next frame.
    pub exit: bool,
    /// The stores and the route chosen in the briefing for the sortie being taken.
    pub take_loadout: Option<crate::loadout::PlannedLoadout>,
    pub take_route: Option<PlannedRoute>,
}

impl ActiveCampaign {
    /// A new campaign alone on a theater.
    pub fn new(
        name: String,
        setup: CampaignSetup,
        data: &MissionData,
        rules: Rules,
    ) -> anyhow::Result<Self> {
        let host = Host::new(setup, &data.0, rules)?;
        Ok(Self::with_host(name, host, Role::Local))
    }

    /// A saved campaign, alone.
    pub fn resume(
        name: String,
        state: CampaignState,
        data: &MissionData,
        rules: Rules,
    ) -> anyhow::Result<Self> {
        let host = Host::resume(state, &data.0, rules)?;
        Ok(Self::with_host(name, host, Role::Local))
    }

    /// A campaign alone or as a listen server.
    pub fn with_host(name: String, host: Host, role: Role) -> Self {
        let mission = host.mission.clone();
        let rules = host.rules;
        let mut c = Self::blank(name, mission, rules, role, HOST_PLAYER);
        c.host = Some(host);
        c
    }

    /// A coop client's campaign: the server's copy.
    pub fn client(
        server: String,
        state: CampaignState,
        data: &MissionData,
        rules: Rules,
        me: PlayerId,
    ) -> anyhow::Result<Self> {
        let mission = stealth_host::glue::base_mission(&state.setup, &data.0)?;
        let mut c = Self::blank(server.clone(), mission, rules, Role::Client, me);
        c.copy = Some(state);
        c.server = Some(server);
        Ok(c)
    }

    fn blank(name: String, mission: Mission, rules: Rules, role: Role, me: PlayerId) -> Self {
        Self {
            name,
            role,
            host: None,
            copy: None,
            mission,
            rules,
            me,
            server: None,
            phase: Phase::Ops,
            ui: ops::OpsUi::default(),
            debrief: None,
            landed: None,
            parked: None,
            aircraft: Airframe::F117,
            roster: Vec::new(),
            actions: Vec::new(),
            resume: None,
            started: false,
            save_timer: 0.0,
            end_timer: None,
            crash_timer: 0.0,
            exit: false,
            take_loadout: None,
            take_route: None,
        }
    }

    /// The campaign as this player knows it.
    pub fn state(&self) -> &CampaignState {
        match (&self.host, &self.copy) {
            (Some(h), _) => &h.state,
            (None, Some(s)) => s,
            (None, None) => unreachable!("a campaign has a host or a copy"),
        }
    }

    /// The Ops screen (or the landing prompt) is up.
    pub fn screen_open(&self) -> bool {
        (self.phase == Phase::Ops && self.end_timer.is_none()) || self.landed.is_some()
    }

    /// Saves the campaign (alone or as the listen server).
    pub fn save(&self) {
        if let Some(h) = &self.host
            && let Err(e) = save::save(&self.name, &h.state)
        {
            warn!("saving campaign {}: {e:#}", self.name);
        }
    }

    /// The players as the PLAYERS tab shows them (alone: just this one).
    pub fn players(&self) -> Vec<PlayerInfo> {
        if !self.roster.is_empty() {
            return self.roster.clone();
        }
        let st = self.state();
        let status = match st.sortie(self.me) {
            Some(s) if self.phase == Phase::Sortie => {
                PlayerStatus::Flying { title: s.offer.title.clone(), aircraft: s.aircraft }
            }
            _ => PlayerStatus::Ops,
        };
        vec![PlayerInfo {
            id: self.me,
            name: st.player_name(self.me).unwrap_or("Pilot").to_string(),
            status,
            ping: 0,
            host: self.role == Role::Host,
            rate: 0,
        }]
    }
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

/// The rules of a campaign alone or served from this game: the game's own settings.
pub fn rules_of(s: &crate::settings::Settings, realistic_weapons: bool) -> Rules {
    Rules {
        realistic_damage: s.realistic_damage,
        realistic_guidance: s.realistic_guidance,
        no_crashes: s.no_crashes,
        continuous_time: s.continuous_time,
        realistic_weapons,
        friendly_fire: false,
    }
}

/// The local pilot's name for the campaign (the multiplayer screen's).
fn pilot_name() -> String {
    let p = crate::prefs::Prefs::load();
    let n = p.player_name.trim();
    if n.is_empty() { "Pilot".to_string() } else { n.to_string() }
}

/// `--campaign <name>`: straight into a saved campaign (or a new one under that name, in
/// `--theater <code>` with `--roe`, `--opponents`, `--sky-ratio` and `--start-hour`).
pub fn from_args(app: &mut App) {
    let args: Vec<String> = std::env::args().collect();
    let get = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let Some(name) = get("--campaign") else { return };
    let data = MissionData::load().expect("mission data");
    let tables = crate::flight::SimTables::load();
    let rules =
        rules_of(&crate::settings::Settings::from_args(), args.iter().any(|a| a == "--lockheed"));
    let saved = save::load(&name).unwrap_or_else(|e| {
        warn!("campaign {name}: {e:#}");
        None
    });
    let c = match saved {
        Some(state) => ActiveCampaign::resume(name, state, &data, rules),
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
                dynamic_weather: !args.iter().any(|a| a == "--clear-sky"),
            };
            ActiveCampaign::new(name, setup, &data, rules)
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

/// The flight session's parts a sortie start or end touches.
#[derive(SystemParam)]
pub struct Session<'w, 's> {
    commands: Commands<'w, 's>,
    combat: ResMut<'w, Combat>,
    player: Query<'w, 's, (&'static mut Aircraft, &'static mut WorldPos), With<Player>>,
    constants: ResMut<'w, FlightConstants>,
    settings: ResMut<'w, crate::settings::Settings>,
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
    link: Option<ResMut<'w, NetLink>>,
    inbox: ResMut<'w, NetInbox>,
    next: ResMut<'w, NextState<AppState>>,
    notice: ResMut<'w, crate::net::Notice>,
    players: ResMut<'w, crate::player_notices::PlayerNotices>,
}

/// First frame of the session: the simulation is made to match the campaign.
fn begin(mut c: ResMut<ActiveCampaign>, mut s: Session) {
    if c.started {
        return;
    }
    c.started = true;
    let c = &mut *c;
    c.aircraft = airframe_of(s.settings.aircraft);
    match c.role {
        Role::Client => s.combat.sim.make_mirror(c.me.0),
        Role::Local | Role::Host => {
            let host = c.host.as_mut().expect("host");
            host.state.rename(HOST_PLAYER, &pilot_name());
            let events = host.begin(&mut s.combat.sim);
            if let Some(NetLink::Host(server)) = s.link.as_deref_mut() {
                server.route(&events);
            }
            handle(c, &mut s, events);
            drain_sim(c, &mut s);
        }
    }
    if let Some(t) = s.theater.as_deref_mut() {
        mark_friendly(t, &s.combat.sim);
    }
    let base = stealth_host::glue::default_base(c.state());
    park(c, &mut s, base);
    c.ui.tab = ops::test_tab().unwrap_or(ops::Tab::Board);
    // A returning client's sortie goes on where his aircraft is.
    if let Some(r) = c.resume.take() {
        begin_sortie(c, &mut s, &r.sortie, &r.state);
    }
    // Test aid: `--campaign-insert` orders an insertion against the nearest enemy place.
    if std::env::args().any(|a| a == "--campaign-insert")
        && let Some(&place) = c.state().front(Side::Blue).first()
    {
        c.actions.push(Action::OrderInsertion { place });
    }
}

/// The flight model's landing corridors follow the places' owners (friendly bases are the
/// ones it lands on), as the simulation has them.
fn mark_friendly(t: &mut crate::world::Theater, sim: &stealth_sim::Sim) {
    for (i, so) in sim.objects().iter().enumerate().skip(3) {
        if so.flags & (oflags::AIRFIELD | oflags::CARRIER) == 0 {
            continue;
        }
        let Some(o) = t.world.objects.get_mut(i) else { continue };
        if (o.x as i32, o.y as i32) != (so.x, so.y) {
            continue;
        }
        let mut f = o.target_flags as u16 & !0x500;
        f |= (so.flags & (oflags::FRIENDLY | oflags::NEUTRAL)) as u16;
        o.target_flags = f as i16;
    }
}

/// Events the host's calls raised in the simulation (outside its steps): tiles destroyed and
/// repaired, rearming; on a listen server the players get them too.
fn drain_sim(c: &mut ActiveCampaign, s: &mut Session) {
    let Some(host) = c.host.as_mut() else { return };
    let events = host.take_sim_events(&mut s.combat.sim);
    if let Some(NetLink::Host(server)) = s.link.as_deref_mut() {
        server.after_step(&events);
    }
    for e in events {
        if e.event.is_private() && PlayerId(e.pilot) != c.me {
            continue;
        }
        match e.event {
            SimEvent::TileDestroyed(t) => {
                s.tiles.set(t.level as usize, t.col, t.row, t.index, t.shape)
            }
            SimEvent::TileRestored(t) => s.tiles.clear(t.level as usize, t.col, t.row, t.index),
            SimEvent::Resupplied => {
                let fuel = s.constants.fuel;
                if let Ok((mut a, _)) = s.player.single_mut() {
                    a.fuel = fuel;
                    a.ended = None;
                    a.has_flown = false;
                }
            }
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
    s.combat.campaign_events.clear();
    if let Some(b) = base {
        let st = s.combat.sim.start_state_at(b);
        place_player(s, &st, b);
    }
    s.view.view = crate::camera::View::Cockpit;
}

/// Puts the aircraft at a state: fresh on the runway, or in the air where a returning
/// player's aircraft is.
fn place_player(s: &mut Session, st: &PlayerState, base: usize) {
    let carrier = s.combat.sim.objects().get(base).is_some_and(|o| o.flags & oflags::CARRIER != 0);
    let fuel = s.constants.fuel;
    let Ok((mut a, mut pos)) = s.player.single_mut() else { return };
    let p = wu_to_bevy(st.x as f64, st.y as f64, if carrier { 128.0 } else { 0.0 });
    *a = Aircraft::new_on_runway(fuel);
    a.altitude = (p.y / crate::world::WU) as f32;
    a.orientation = Aircraft::from_euler(bam(st.heading as f32), 0.0, 0.0);
    if !st.on_ground {
        a.altitude = st.altitude as f32;
        a.orientation =
            Aircraft::from_euler(bam(st.heading as f32), bam(st.pitch as f32), bam(st.roll as f32));
        a.on_ground = false;
        a.gear_down = !st.gear_up;
        a.velocity = st.velocity as f32;
        a.has_flown = true;
    }
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
        c.actions.push(Action::EndSortie { outcome: 7 });
    }
    // The flight model ended the flight (its ground model differs in detail) and the
    // simulation did not follow: the sortie ends anyway.
    if c.phase == Phase::Sortie && c.end_timer.is_none() && !s.combat.sim.player_absent() {
        let ended = s.player.single().ok().and_then(|(a, _)| a.ended);
        match ended {
            Some(FlightEnd::Crash | FlightEnd::BadLanding) if s.combat.sim.chute().is_none() => {
                if c.crash_timer >= 0.0 {
                    c.crash_timer += dt;
                }
                if c.crash_timer > 3.0 {
                    c.crash_timer = -1.0;
                    let outcome = if ended == Some(FlightEnd::BadLanding) { 5 } else { 1 };
                    c.actions.push(Action::EndSortie { outcome });
                }
            }
            // Stopped on a runway the simulation does not count as a landing: the prompt.
            Some(FlightEnd::Landed) if c.landed.is_none() => {
                c.crash_timer += dt;
                if c.crash_timer > 3.0 {
                    let p = s.combat.sim.player();
                    c.landed = c.state().nearest_player_base(p.mx, p.my).map(|b| b as usize);
                }
            }
            _ => c.crash_timer = 0.0,
        }
    }
    // Test aid: take a mission of the board at once.
    if c.phase == Phase::Ops
        && c.state().clock > 2.0
        && let Some(n) = ops::test_take()
        && !std::mem::replace(&mut c.ui.test_taken, true)
    {
        let flown: Vec<u32> = c
            .state()
            .board
            .iter()
            .filter(|o| o.kind.flyable_by(c.aircraft))
            .map(|o| o.id)
            .collect();
        if let Some(&offer) = flown.get(n.min(flown.len().saturating_sub(1))) {
            let loadout = briefed_loadout(c, offer, None, c.aircraft);
            if let Some(loadout) = loadout {
                let aircraft = c.aircraft;
                c.actions.push(Action::TakeMission {
                    offer,
                    base: None,
                    aircraft,
                    loadout,
                    route: None,
                });
            }
        }
    }
    let events = match c.role {
        Role::Client => client_frame(c, &mut s, dt),
        Role::Local | Role::Host => host_frame(c, &mut s, dt),
    };
    handle(c, &mut s, events);
    drain_sim(c, &mut s);
    // The debrief comes up a few seconds after the end (the chute is down by then).
    if let Some(t) = c.end_timer.as_mut() {
        *t += dt;
        if *t > END_SECONDS {
            let base = c.parked;
            park(c, &mut s, base);
            c.ui.tab = ops::test_tab().unwrap_or(ops::Tab::Debrief);
        }
    }
    // The sky's clock.
    let day = 86_400.0 * 30.0;
    s.combat.sky_seconds = Some(((c.state().sky_hours() * 3600.0) % day) as f32);
    // Saved now and then.
    c.save_timer += dt;
    if c.save_timer > SAVE_EVERY {
        c.save_timer = 0.0;
        c.save();
    }
}

/// The stores the briefing would give a mission of the board.
pub fn briefed_loadout(
    c: &ActiveCampaign,
    offer: u32,
    base: Option<u16>,
    aircraft: Airframe,
) -> Option<crate::loadout::PlannedLoadout> {
    let o = c.state().board.iter().find(|o| o.id == offer)?.clone();
    let m = sortie_mission(&c.mission, c.state(), &preview(&o, base.unwrap_or(o.base), aircraft));
    Some(crate::loadout::PlannedLoadout::of(&m, kind_of(aircraft)))
}

/// A sortie not taken yet (for its briefing).
pub fn preview(o: &stealth_campaign::MissionOffer, base: u16, aircraft: Airframe) -> Sortie {
    Sortie { id: 0, offer: o.clone(), base, aircraft, started: 0.0, progress: default() }
}

/// The campaign is run here: the Ops screen's actions, the players' commands (listen
/// server), the campaign's frame.
fn host_frame(c: &mut ActiveCampaign, s: &mut Session, dt: f32) -> Vec<HostEvent> {
    let me = c.me;
    let mut out = Vec::new();
    let host = c.host.as_mut().expect("host");
    let combat = &mut *s.combat;
    let sim = &mut combat.sim;
    let mut server = match s.link.as_deref_mut() {
        Some(NetLink::Host(server)) => Some(server),
        _ => None,
    };
    // The players who joined: their messages.
    if let Some(server) = server.as_deref_mut() {
        out.extend(server.pump(host, sim, web_time::Instant::now()));
    }
    for a in std::mem::take(&mut c.actions) {
        let result = match a {
            Action::TakeMission { offer, base, aircraft, loadout, route } => {
                let stores = Stores { stations: loadout.stations(), select: 0 };
                c.take_loadout = Some(loadout);
                c.take_route = route;
                host.take_mission(sim, me, offer, base, aircraft, Some(stores))
            }
            Action::OrderInsertion { place } => host.order_insertion(sim, me, place),
            Action::RequestSupport { offer } => host.request_support(sim, me, offer),
            Action::EndSortie { outcome } => Ok(host.end_sortie(sim, me, outcome)),
            Action::Rearm { loadout } => {
                host.rearm(sim, me, &Stores { stations: loadout.stations(), select: 0 });
                s.msgs.show("Rearmed and refuelled");
                c.landed = None;
                Ok(Vec::new())
            }
            Action::SelectMission { offer, base } => host.select(sim, me, offer, base),
            Action::EditPlan { offer, base, route } => host.edit_plan(sim, me, offer, base, route),
        };
        match result {
            Ok(ev) => {
                c.ui.error = None;
                if let Some(server) = server.as_deref_mut() {
                    server.route(&ev);
                }
                out.extend(ev);
            }
            Err(e) => c.ui.error = Some(e),
        }
    }
    // The simulation's events of this frame's steps.
    let events = std::mem::take(&mut combat.campaign_events);
    let ev = host.tick(sim, dt as f64, &events);
    if let Some(server) = server {
        server.route(&ev);
        if combat.steps > 0 {
            server.send_frame(host, sim, web_time::Instant::now());
        }
        let roster = server.roster(host, sim);
        s.players.roster_changed(&c.roster, &roster, me);
        c.roster = roster;
    }
    out.extend(ev);
    out
}

/// The server runs the campaign: the Ops screen's actions go to it, its news come back.
fn client_frame(c: &mut ActiveCampaign, s: &mut Session, dt: f32) -> Vec<HostEvent> {
    let Some(NetLink::Client(client)) = s.link.as_deref_mut() else {
        return Vec::new();
    };
    for a in std::mem::take(&mut c.actions) {
        let msg = match a {
            Action::TakeMission { offer, base, aircraft, loadout, route } => {
                let stores = Stores { stations: loadout.stations(), select: 0 };
                c.take_loadout = Some(loadout);
                c.take_route = route;
                ToServer::TakeMission { offer, base, aircraft, stores: Some(stores) }
            }
            Action::OrderInsertion { place } => ToServer::OrderInsertion { place },
            Action::RequestSupport { offer } => ToServer::RequestSupport { offer },
            Action::EndSortie { outcome } => ToServer::EndSortie { outcome },
            Action::Rearm { loadout } => {
                s.msgs.show("Rearmed and refuelled");
                c.landed = None;
                let fuel = s.constants.fuel;
                if let Ok((mut a, _)) = s.player.single_mut() {
                    a.fuel = fuel;
                    a.ended = None;
                    a.has_flown = false;
                }
                ToServer::Rearm { stores: Stores { stations: loadout.stations(), select: 0 } }
            }
            Action::SelectMission { offer, base } => ToServer::Select { offer, base },
            Action::EditPlan { offer, base, route } => ToServer::Plan { offer, base, route },
        };
        client.send(&msg);
    }
    // The copy's clock runs on between the server's updates.
    if let Some(st) = c.copy.as_mut() {
        st.clock += dt as f64;
    }
    let mut out = Vec::new();
    for e in std::mem::take(&mut s.inbox.campaign) {
        use stealth_net::client::ClientEvent as E;
        match e {
            E::Host(ev) => out.push(ev),
            E::Campaign(st) => {
                let clock = c.copy.as_ref().map_or(0.0, |s| s.clock);
                let mut st = *st;
                // The Ops screen's clock does not jump back.
                st.clock = st.clock.max(clock - 1.0);
                c.copy = Some(st);
            }
            E::Error(e) => c.ui.error = Some(e),
            E::Roster(r) => {
                s.players.roster_changed(&c.roster, &r, c.me);
                c.roster = r;
            }
            E::Closed(why) => {
                s.notice.0 = Some(format!("Disconnected: {why}"));
                c.exit = true;
                s.next.set(AppState::Menu);
            }
            _ => {}
        }
    }
    out
}

/// The campaign's airframe of a game aircraft, and back.
pub fn airframe_of(k: crate::aircraft::AircraftKind) -> Airframe {
    match k {
        crate::aircraft::AircraftKind::F117 => Airframe::F117,
        crate::aircraft::AircraftKind::F16 => Airframe::F16,
        crate::aircraft::AircraftKind::UH60 => Airframe::UH60,
        crate::aircraft::AircraftKind::AH1Z => Airframe::AH1Z,
    }
}

pub fn kind_of(a: Airframe) -> crate::aircraft::AircraftKind {
    match a {
        Airframe::F117 => crate::aircraft::AircraftKind::F117,
        Airframe::F16 => crate::aircraft::AircraftKind::F16,
        Airframe::UH60 => crate::aircraft::AircraftKind::UH60,
        Airframe::AH1Z => crate::aircraft::AircraftKind::AH1Z,
    }
}

/// The host's events for this player and everybody.
fn handle(c: &mut ActiveCampaign, s: &mut Session, events: Vec<HostEvent>) {
    let me = c.me;
    for e in events {
        match e {
            HostEvent::SortieStarted { player, sortie, start } if player == me => {
                begin_sortie(c, s, &sortie, &start);
            }
            HostEvent::SortieFinished { player, debrief } if player == me => {
                s.msgs.show(debrief.title.clone());
                // Down under the parachute: the debrief at once (the pilotless aircraft is
                // nothing to watch); else the end stays in view a few seconds.
                let ejected = debrief.record.end == stealth_campaign::SortieEnd::Ejected;
                c.debrief = Some(debrief);
                c.landed = None;
                c.end_timer = Some(if ejected { END_SECONDS } else { 0.0 });
            }
            HostEvent::Landed { player, base } if player == me && c.phase == Phase::Sortie => {
                c.landed = Some(base);
                s.msgs.show("Landed - rearm or end the sortie");
            }
            HostEvent::Campaign(CampaignEvent::Message(text)) => {
                info!("campaign: {text}");
                s.msgs.show(text);
            }
            HostEvent::Campaign(CampaignEvent::ObjectiveAchieved { player, .. })
                if player == me =>
            {
                s.msgs.show("Objective achieved");
            }
            // A client's copy has the plans at once (the host's state has them already).
            HostEvent::Campaign(CampaignEvent::PlansChanged(plans)) => {
                if let Some(st) = c.copy.as_mut() {
                    st.plans = plans;
                }
            }
            HostEvent::Campaign(CampaignEvent::PlaceCaptured { .. }) => {
                if let Some(t) = s.theater.as_deref_mut() {
                    mark_friendly(t, &s.combat.sim);
                }
            }
            _ => {}
        }
    }
}

/// A sortie starts: the aircraft (the chosen type) on the base's runway with the mission's
/// stores and route.
fn begin_sortie(c: &mut ActiveCampaign, s: &mut Session, sortie: &Sortie, start: &PlayerState) {
    let base = sortie.base as usize;
    let m = sortie_mission(&c.mission, c.state(), sortie);
    let aircraft = kind_of(sortie.aircraft);
    c.aircraft = sortie.aircraft;
    // Another aircraft than the last sortie's: its cockpit, model and flight model.
    if s.settings.aircraft != aircraft {
        s.settings.aircraft = aircraft;
    }
    *s.constants = crate::flight::constants_for(aircraft, c.rules.no_crashes);
    let loadout = c
        .take_loadout
        .take()
        .filter(|l| l.fits(&m, aircraft))
        .unwrap_or_else(|| crate::loadout::PlannedLoadout::of(&m, aircraft));
    let route = c.take_route.take().filter(|r| r.fits(&m)).unwrap_or_else(|| PlannedRoute::of(&m));
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
    place_player(s, start, base);
    s.view.view = crate::camera::View::Cockpit;
    s.menu.open = false;
    c.phase = Phase::Sortie;
    c.parked = Some(base);
    c.landed = None;
    c.end_timer = None;
    c.crash_timer = 0.0;
    s.msgs.show(sortie.offer.title.clone());
}

/// Leaving the flight session: the campaign is saved and closed (a client leaves the server,
/// a listen server stops), and the Ops screen's egui context goes with it (the menu brings
/// its own).
#[allow(clippy::too_many_arguments)]
fn leave(
    mut commands: Commands,
    c: Option<Res<ActiveCampaign>>,
    cams: Query<Entity, With<ops::OpsCamera>>,
    mut view: ResMut<UnitView>,
    mut sites: ResMut<crate::world::StreamSites>,
    link: Option<ResMut<NetLink>>,
) {
    for e in &cams {
        commands.entity(e).try_despawn();
    }
    view.close();
    unit_view::follow_cleanup(&mut commands, &mut view);
    commands.insert_resource(crate::camera::HideOwnAircraft(false));
    sites.follow = None;
    #[cfg(feature = "xr")]
    commands.remove_resource::<crate::vr_menu::WorldPanel>();
    if let Some(mut link) = link {
        match &mut *link {
            NetLink::Client(client) => client.leave(),
            NetLink::Host(server) => server.shutdown("The host left the game"),
        }
        commands.remove_resource::<NetLink>();
    }
    if let Some(c) = c {
        c.save();
        commands.remove_resource::<ActiveCampaign>();
    }
}
