//! The combat simulation (crate stealth-sim, the original game logic: radar and EMV, threats, AI,
//! missiles, weapons, damage, objectives) driving the flight.
//!
//! * On entering the flight a [`Sim`] is built from the active mission (`mission.rs` makes sure
//!   there is one) and the static tables in `assets/sim/sim.json`. The player starts at the
//!   simulation's start state.
//! * The simulation runs on original frames at a fixed `fs` = 15 frames per (virtual) second:
//!   every tick gets a [`PlayerState`] built from the flight model and the one-shot commands
//!   (keys) collected since the previous tick. Time acceleration (Shift-Z) doubles the virtual
//!   clock, like the original's sim time = real time x accel.
//! * Its outputs are published every frame into the cockpit resources (HUD messages, stores,
//!   waypoints, cues, lights, EMV gauge, MFD contacts and camera data) and into the flight model
//!   (bay doors, damage effects, fuel transfers). World changes (destroyed tile objects) go to
//!   `world::TileOverrides`; moving objects are drawn by `sim_render.rs`.
//! * Mission end: a simple overlay with the outcome, then back to the menu.

use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::math::DVec3;
use bevy::prelude::*;
use stealth_sim::{
    Commands as SimCommands, FlightUnit, HudMode as SimHud, LandingMode, MissionEnd, N_PROJ,
    PlayerState, Projectile, Sim, SimData, SimEvent, SimOptions, dmg, fflags, oflags,
};

use crate::camera::{View, ViewState};
use crate::flight::{
    Aircraft, FlightConstants, FlightDamage, FlightEnd, FlightSystems, Player, bam, to_bam,
};
use crate::hud::{
    Bay, HudCues, HudMessages, HudMode, HudState, Stores, TargetBox, Waypoint, Waypoints,
};
use crate::input::{Action, ActionEvent, FlightControls};
use crate::mfd::{
    Contact, ContactKind, ContactRef, Contacts, MfdState, MissionClock, MissionOrders, TrackCam,
};
use crate::mission::{ActiveMission, PlannedRoute, clean_text};
use crate::net::{NetInbox, NetLink, SimMsg};
use crate::panel::{Countermeasures, Emv, Warnings};
use crate::settings::{AppState, MissionSetup, SetupSet, StartPose};
use crate::world::{TileOverrides, WU, WorldPos, bevy_to_wu, wu_to_bevy};

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeAccel>()
            .init_resource::<PlayerCollision>()
            .init_resource::<crate::hud::DamageFlash>()
            .add_systems(
                OnEnter(AppState::Flight),
                (
                    crate::mission::ensure_mission.in_set(SetupSet::Mission),
                    start_combat.in_set(SetupSet::World),
                ),
            )
            .add_systems(OnExit(AppState::Flight), stop_combat.in_set(FlightTeardown))
            .add_systems(Update, apply_time_accel)
            .add_systems(
                Update,
                (detect_collision, collect_commands, step_combat, publish)
                    .chain()
                    .in_set(CombatSystems)
                    .after(FlightSystems)
                    .run_if(in_state(AppState::Flight).and_then(resource_exists::<Combat>)),
            )
            .add_systems(
                Update,
                end_overlay.after(CombatSystems).run_if(in_state(AppState::Flight)),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSystems;

/// Original frames per sim second (as on a fast machine).
pub const FS: u8 = 15;
/// At most this many sim frames per rendered frame (slow frames are dropped beyond it).
const MAX_STEPS: u32 = 30;

/// Static simulation tables (assets/sim/sim.json), loaded once.
#[derive(Resource, Clone)]
pub struct SimDataRes(pub Arc<SimData>);

fn load_sim_data() -> anyhow::Result<SimData> {
    SimData::load_json(&crate::asset_path::asset("sim"))
}

/// The running simulation and its bookkeeping.
#[derive(Resource)]
pub struct Combat {
    pub sim: Sim,
    pub data: Arc<SimData>,
    /// Virtual seconds not yet simulated.
    accum: f32,
    /// One-shot commands collected since the last sim frame.
    pending: SimCommands,
    /// END MISSION was asked for (a campaign ends the sortie instead, `campaign.rs`).
    pub quit: bool,
    /// Flight units and projectiles before the last step, for render interpolation.
    pub prev_flights: Vec<FlightUnit>,
    pub prev_proj: [Projectile; N_PROJ],
    /// Classic missiles' smoothed paths as drawn (`sim_render::smooth_projectiles`).
    pub proj_smooth: [Option<crate::sim_render::Smoothed>; N_PROJ],
    /// Fraction of a sim frame elapsed since the last step (0..1; a coop client's: of the
    /// gap between the server's last two frames).
    pub alpha: f32,
    /// Sim frames between the last two states (a coop client's server frames come a few
    /// frames apart).
    pub frame_gap: f32,
    /// The player's own rounds' alpha (a coop client flies them at his own pace).
    pub own_alpha: f32,
    /// A coop client's playout of the server's frames.
    playout: crate::net_interp::Playout,
    /// The other pilots' aircraft as sampled, and the frame they are drawn at.
    pub remote_track: crate::net_interp::RemoteTrack,
    pub remote_time: f64,
    /// Frames stepped here (a listen server's samples of the other pilots).
    host_frame: u32,
    /// Explosions of the steps of this frame: (map x, map y, viewZ altitude, kind).
    pub explosions: Vec<(i32, i32, i32, i32)>,
    /// Gun rounds that hit the ground in the steps of this frame (map x, map y).
    pub round_impacts: Vec<(f64, f64)>,
    /// Signal flares the downed pilot fired in the steps of this frame (map x, map y).
    pub signal_flares: Vec<(i32, i32)>,
    /// Tile objects destroyed in earlier missions of a tour, still to be shown destroyed.
    pub prior_tiles: Vec<stealth_sim::TileDamage>,
    /// Campaign: the simulation's events (all pilots') since the campaign last took them
    /// (`campaign/mod.rs`).
    pub campaign_events: Vec<stealth_sim::PilotEvent>,
    /// Campaign: the sky's clock (seconds of the day), replacing the mission clock.
    pub sky_seconds: Option<f32>,
    /// Things destroyed on the ground in the steps of this frame (they burn, fires.rs).
    pub kills: Vec<Kill>,
    /// Map position of the falling wreck (it burns where it lands).
    wreck: Option<(i32, i32)>,
    /// Number of sim frames run this frame.
    pub steps: u32,
    sim_waypoint: i32,
    pub end: Option<EndInfo>,
    flight_end_timer: f32,
    lockheed: bool,
    /// WLD names of the mission world (object type names).
    names: Vec<String>,
    log: bool,
    /// The flight so far, for the debrief and the Tacview export.
    pub record: crate::recorder::FlightRecord,
}

/// Something destroyed on the ground: map position and whether it was a vehicle (else a
/// building, installation or ship).
#[derive(Debug, Clone, Copy)]
pub struct Kill {
    pub x: i32,
    pub y: i32,
    pub vehicle: bool,
}

/// Object type names of vehicles (small fires); everything else burns like a building.
fn is_vehicle(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["mobile", "tanks", "boat", "airplane", "ss missile", "scud", "trawler", "trail"]
        .iter()
        .any(|k| n.contains(k))
        || n == "tank"
        || n == "missile"
}

/// What the end overlay shows.
#[derive(Debug, Clone)]
pub struct EndInfo {
    pub title: String,
    pub lines: Vec<String>,
}

/// Time acceleration (Shift-Z / Shift-X: x2) on top of `--time-scale`.
#[derive(Resource)]
pub struct TimeAccel {
    pub base: f32,
    pub on: bool,
}

impl Default for TimeAccel {
    fn default() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let base = args
            .windows(2)
            .find(|w| w[0] == "--time-scale")
            .and_then(|w| w[1].parse::<f32>().ok())
            .unwrap_or(1.0);
        Self { base, on: false }
    }
}

fn apply_time_accel(
    accel: Res<TimeAccel>,
    mut time: ResMut<Time<Virtual>>,
    combat: Option<Res<Combat>>,
    link: Option<Res<NetLink>>,
) {
    // Waiting on the ground for the rescue goes faster; a campaign runs in real time, a coop
    // game (everybody's clock is the server's) without `--time-scale` either.
    let campaign = combat.as_ref().is_some_and(|c| c.sim.campaign().is_some());
    let fast = if combat.is_some_and(|c| c.sim.csar().is_some()) { 4.0 } else { 2.0 };
    let base = if link.is_some() { 1.0 } else { accel.base };
    let want = base * if accel.on && !campaign { fast } else { 1.0 };
    if time.relative_speed() != want {
        time.set_relative_speed(want);
    }
}

#[allow(clippy::too_many_arguments)]
fn start_combat(
    mut commands: Commands,
    mission: Option<Res<ActiveMission>>,
    data: Option<Res<SimDataRes>>,
    constants: Res<FlightConstants>,
    mut setup: ResMut<MissionSetup>,
    mut orders: ResMut<MissionOrders>,
    mut clock: ResMut<MissionClock>,
    mut wps: ResMut<Waypoints>,
    mut accel: ResMut<TimeAccel>,
    route: Option<Res<crate::mission::PlannedRoute>>,
    loadout: Option<Res<crate::loadout::PlannedLoadout>>,
    career: Option<Res<crate::career::CareerFlight>>,
    settings: Res<crate::settings::Settings>,
    campaign: Option<Res<crate::campaign::ActiveCampaign>>,
) {
    accel.on = false;
    let Some(m) = mission else {
        warn!("no mission: flying without the combat simulation");
        return;
    };
    let m = &m.0;
    let data = match data {
        Some(d) => d.0.clone(),
        None => match load_sim_data() {
            Ok(d) => {
                let d = Arc::new(d);
                commands.insert_resource(SimDataRes(d.clone()));
                d
            }
            Err(e) => {
                warn!("no simulation data, flying without the combat simulation: {e:#}");
                return;
            }
        },
    };
    // Remake theaters have no tiles in the exported original data: take them from the theater file.
    let data = if data.theaters.get(m.settings.theater).is_none() {
        let mut d = (*data).clone();
        let theater = crate::world::Theater::load(&m.theater_code.to_ascii_lowercase());
        let tiles = theater.sim_tiles();
        d.theaters.resize(m.settings.theater, tiles.clone());
        d.theaters.push(tiles);
        Arc::new(d)
    } else {
        data
    };
    let landing = match constants.landing {
        0 => LandingMode::NoCrashes,
        2 => LandingMode::Realistic,
        _ => LandingMode::Easy,
    };
    // A tour sortie starts with what the pilot destroyed earlier in the tour.
    let prior_damage = career
        .and_then(|c| crate::career::Pilot::load(&c.0))
        .and_then(|p| p.tour)
        .map(|t| t.damage);
    let prior_tiles = prior_damage.as_ref().map(|d| d.tiles.clone()).unwrap_or_default();
    let external_stores = !settings.aircraft.spec().bomb_bay;
    let opts = match &campaign {
        // A campaign's rules are its host's (a coop server's for everybody).
        Some(c) => SimOptions { external_stores, ..c.rules.sim_options() },
        None => SimOptions {
            landing,
            prior_damage,
            external_stores,
            realistic_damage: settings.realistic_damage,
            realistic_guidance: settings.realistic_guidance,
            air_defence: settings.air_defence,
            ..default()
        },
    };
    let mut sim = Sim::with_options(&data, m, opts);
    // The stores chosen in the menu, else the briefed ones on this aircraft.
    let loadout = loadout
        .filter(|l| l.fits(m, settings.aircraft))
        .map(|l| l.clone())
        .unwrap_or_else(|| crate::loadout::PlannedLoadout::of(m, settings.aircraft));
    sim.set_loadout(&loadout.stations());
    sim.select_bay(loadout.first_choice(m));
    // Test aid (`--damage bits,hits`): start damaged (bits as in `stealth_sim::dmg`, 0x20 fire).
    let args: Vec<String> = std::env::args().collect();
    if let Some((bits, hits)) =
        args.windows(2).find(|w| w[0] == "--damage").and_then(|w| w[1].split_once(',')).and_then(
            |(b, h)| {
                let b = b.trim();
                let bits = match b.strip_prefix("0x") {
                    Some(hex) => u8::from_str_radix(hex, 16).ok()?,
                    None => b.parse().ok()?,
                };
                Some((bits, h.trim().parse::<i32>().ok()?))
            },
        )
    {
        sim.set_damage(bits, hits);
    }
    // The player starts at the sim's start state (the same formula as `mission::start_pose`).
    let st = sim.start_state();
    setup.start = Some(StartPose {
        east_wu: st.x as f64,
        north_wu: st.y as f64,
        heading: bam(st.heading as f32),
        carrier: m.takeoff.carrier,
    });
    orders.lines = m.orders.lines.iter().map(|l| (l.color, clean_text(&l.text))).collect();
    // 1 clock tick = 2 s after 06:00 / 18:00.
    *clock = MissionClock {
        start_seconds: m.time.base_hour as f32 * 3600.0 + m.time.tod_ticks as f32 * 2.0,
        elapsed: Some(0.0),
    };
    // The route from the mission planner, else the mission's own waypoints.
    let route =
        route.filter(|r| r.fits(m)).map(|r| r.clone()).unwrap_or_else(|| PlannedRoute::of(m));
    *wps = Waypoints {
        points: route
            .points
            .iter()
            .map(|p| Waypoint { east: p.x as f64 * 32.0, north: (0x8000 - p.y) as f64 * 32.0 })
            .collect(),
        current: 0,
        mission_current: 0,
        anchors: route.anchors(),
    };
    let names = m.world.names.clone();
    commands.insert_resource(Combat {
        prior_tiles,
        campaign_events: Vec::new(),
        sky_seconds: None,
        prev_flights: sim.flights().to_vec(),
        prev_proj: *sim.projectiles(),
        proj_smooth: [None; N_PROJ],
        sim,
        data,
        accum: 0.0,
        pending: SimCommands::default(),
        quit: false,
        alpha: 0.0,
        frame_gap: 1.0,
        own_alpha: 0.0,
        playout: default(),
        remote_track: default(),
        remote_time: 0.0,
        host_frame: 0,
        explosions: Vec::new(),
        round_impacts: Vec::new(),
        signal_flares: Vec::new(),
        kills: Vec::new(),
        wreck: None,
        steps: 0,
        sim_waypoint: 0,
        end: None,
        flight_end_timer: 0.0,
        lockheed: m.settings.plane == stealth_mission::Plane::Lockheed,
        names,
        log: std::env::args().any(|a| a == "--log-flight" || a == "--log-sim"),
        record: default(),
    });
}

/// Despawns the player aircraft (with everything parented to it) when leaving the flight.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlightTeardown;

#[allow(clippy::too_many_arguments)]
fn stop_combat(
    mut commands: Commands,
    players: Query<Entity, With<Player>>,
    overlays: Query<Entity, With<EndOverlay>>,
    mut accel: ResMut<TimeAccel>,
) {
    commands.remove_resource::<Combat>();
    commands.remove_resource::<ActiveMission>();
    for e in players.iter().chain(overlays.iter()) {
        commands.entity(e).try_despawn();
    }
    accel.on = false;
}

/// Keys -> one-shot simulation commands, collected until the next frame.
fn collect_commands(
    mut actions: MessageReader<ActionEvent>,
    mut locks: MessageReader<crate::avionics::LockRequest>,
    mut pods: MessageReader<crate::avionics::PodDesignate>,
    mut combat: ResMut<Combat>,
    mut accel: ResMut<TimeAccel>,
) {
    let combat = &mut *combat;
    let c = &mut combat.pending;
    for crate::avionics::PodDesignate(x, y) in pods.read() {
        c.designate_at = Some((*x, *y));
    }
    for crate::avionics::LockRequest(target) in locks.read() {
        match *target {
            ContactRef::Unit(i) => c.lock_unit = u16::try_from(i).ok(),
            ContactRef::Object(i) => c.lock_object = u16::try_from(i).ok(),
            ContactRef::Point(x, y) => c.designate_at = Some((x, y)),
        }
    }
    for ActionEvent(a) in actions.read() {
        match a {
            Action::SelectWeapon => c.next_bay = true,
            Action::FireWeapon => c.fire = true,
            Action::Flare => c.flare = true,
            Action::Chaff => c.chaff = true,
            Action::Decoy => c.decoy = true,
            Action::IrJammer => c.toggle_irj ^= true,
            Action::Ecm => c.toggle_ecm ^= true,
            Action::BayDoors => c.toggle_bay ^= true,
            Action::DesignateTarget => c.designate = true,
            Action::SelectTarget => c.next_target = true,
            Action::CamAhead => c.camera_dir = Some(0),
            Action::NearestTarget => c.nearest_target = true,
            Action::ClearTarget => c.clear_target = true,
            Action::CamRight => c.camera_dir = Some(0x4000),
            Action::CamRear => c.camera_dir = Some(0x8000),
            Action::CamLeft => c.camera_dir = Some(0xC000),
            Action::Eject => c.eject = true,
            Action::Training => c.toggle_training ^= true,
            Action::Resupply => c.resupply = true,
            Action::TimeAccel => accel.on = true,
            Action::TimeNormal => accel.on = false,
            Action::Quit => combat.quit = true,
            _ => {}
        }
    }
}

fn sim_hud_mode(m: HudMode) -> SimHud {
    match m {
        HudMode::Nav => SimHud::Nav,
        HudMode::Air => SimHud::Air,
        HudMode::Gnd => SimHud::Gnd,
    }
}

fn is_cockpit_view(v: View) -> bool {
    matches!(v, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear)
}

/// The flight model's state in the original variables.
#[allow(clippy::too_many_arguments)]
fn player_state(
    a: &Aircraft,
    pos: &WorldPos,
    gun: bool,
    trigger: bool,
    collided: bool,
    hud: &HudState,
    mfd: &MfdState,
    view: &ViewState,
    cmd: SimCommands,
) -> PlayerState {
    let (east, north, _) = bevy_to_wu(pos.0);
    let (h, p, r) = a.euler();
    PlayerState {
        x: east.round() as i32,
        y: north.round() as i32,
        altitude: a.altitude.clamp(0.0, 65535.0) as u16,
        heading: to_bam(h).round() as i32 as u16,
        pitch: to_bam(p).round() as i32 as i16,
        roll: to_bam(r).round() as i32 as i16,
        velocity: a.velocity.clamp(0.0, 65535.0) as u16,
        gear_up: !a.gear_down,
        flaps: a.flaps,
        brakes: a.brakes,
        on_ground: a.on_ground,
        autopilot: a.autopilot(),
        climb_rate: a.climb_rate as i32,
        fuel: a.fuel.max(0.0) as i32,
        collided,
        hud_mode: sim_hud_mode(hud.mode),
        camera_mfd: mfd.camera_shown(),
        external_view: !is_cockpit_view(view.view),
        hud_visible: view.view == View::Cockpit,
        ccrp: hud.ccrp,
        cmd: SimCommands { gun, trigger, ..cmd },
    }
}

#[derive(SystemParam)]
struct StepParams<'w, 's> {
    time: Res<'w, Time>,
    player: Query<'w, 's, (&'static mut Aircraft, &'static WorldPos), With<Player>>,
    controls: Res<'w, FlightControls>,
    hud: Res<'w, HudState>,
    mfd: Res<'w, MfdState>,
    view: Res<'w, ViewState>,
    msgs: ResMut<'w, HudMessages>,
    accel: ResMut<'w, TimeAccel>,
    tiles: ResMut<'w, TileOverrides>,
    collision: Res<'w, PlayerCollision>,
    flash: ResMut<'w, crate::hud::DamageFlash>,
    sounds: MessageWriter<'w, crate::sound::PlaySound>,
    link: Option<ResMut<'w, NetLink>>,
    inbox: ResMut<'w, NetInbox>,
}

/// The player is inside a world object (the "camera inside an object" test, feeding the
/// sim's collision rule).
#[derive(Resource, Default)]
pub struct PlayerCollision(pub bool);

/// Tests the pilot's eye (the original tests the camera point) against the original plane hulls of
/// visible tile objects.
fn detect_collision(
    mut collision: ResMut<PlayerCollision>,
    player: Query<&GlobalTransform, With<crate::camera::HeadAnchor>>,
    hulls: Query<(
        &crate::models::CollisionHull,
        &crate::models::LodLevel,
        &crate::models::LodShown,
        &GlobalTransform,
        &InheritedVisibility,
    )>,
    tile_roots: Query<(), With<crate::world::TileObject>>,
) {
    collision.0 = false;
    let Ok(p) = player.single() else { return };
    let p = p.translation();
    for (hull, lod, shown, tf, vis) in &hulls {
        if !vis.get() || !shown.main || !tile_roots.contains(lod.root) {
            continue;
        }
        let scale = tf.scale().x.max(1e-6);
        // Quick reject in metres before the inverse transform.
        if p.distance(tf.translation()) > (hull.radius + 2.0) * scale {
            continue;
        }
        let local = tf.affine().inverse().transform_point3(p);
        if hull.contains(local) {
            collision.0 = true;
            return;
        }
    }
}

fn step_combat(mut combat: ResMut<Combat>, p: StepParams) {
    let StepParams {
        time,
        mut player,
        controls,
        hud,
        mfd,
        view,
        mut msgs,
        mut accel,
        mut tiles,
        collision,
        mut flash,
        mut sounds,
        mut link,
        mut inbox,
    } = p;
    let combat = &mut *combat;
    combat.explosions.clear();
    combat.round_impacts.clear();
    combat.signal_flares.clear();
    for t in std::mem::take(&mut combat.prior_tiles) {
        tiles.set(t.level as usize, t.col, t.row, t.index, t.shape);
    }
    combat.kills.clear();
    combat.steps = 0;
    let Ok((mut a, pos)) = player.single_mut() else { return };
    let mut a = &mut *a;
    let campaign = combat.sim.campaign().is_some();
    if combat.quit && combat.sim.mission_end().is_none() && !campaign {
        combat.sim.quit();
        combat.end = Some(end_info(&combat.sim, None::<MissionEnd>));
    }
    let dt = 1.0 / FS as f32;
    combat.accum += time.delta_secs();
    let mut fx = Fx {
        msgs: &mut msgs,
        accel: &mut accel,
        tiles: &mut tiles,
        flash: &mut flash,
        sounds: &mut sounds,
    };
    // A coop client flies on the server's simulation: its frames instead of steps.
    if let Some(NetLink::Client(client)) = link.as_deref_mut() {
        while combat.accum >= dt {
            combat.accum -= dt;
            let cmd = std::mem::take(&mut combat.pending);
            let ps = player_state(
                &a,
                pos,
                controls.gun,
                controls.fire,
                collision.0,
                &hud,
                &mfd,
                &view,
                cmd,
            );
            client.send_input(&ps);
            // His own rounds fly here; what they hit on his screen he reports.
            combat.sim.set_local_player(&ps);
            for hit in combat.sim.fly_own_rounds(&ps) {
                client.send_hit(hit);
            }
        }
        let ps = player_state(&a, pos, false, false, collision.0, &hud, &mfd, &view, default());
        combat.sim.set_local_player(&ps);
        // The server's frames play out a little behind, at a steady pace; what came between
        // them (changes, events) goes in order with them.
        let fs = FS as f64;
        for m in &inbox.sim {
            if let SimMsg::Fast(step, ..) = m {
                combat.playout.received(*step, fs);
            }
        }
        combat.playout.advance(time.delta_secs() as f64, fs);
        let mut queue = std::mem::take(&mut inbox.sim).into_iter();
        while let Some(m) = queue.next() {
            match m {
                SimMsg::Slow(w) => {
                    let before: Vec<bool> =
                        combat.sim.aaa_units().iter().map(|u| u.destroyed).collect();
                    combat.sim.apply_world_slow(&w);
                    burn_aaa(combat, &before);
                }
                SimMsg::Events(events) => {
                    for e in events {
                        take_event(combat, e, &mut a, &mut fx);
                    }
                }
                SimMsg::Fast(step, w, tick, mine) => {
                    if !combat.playout.due() {
                        inbox.sim.push(SimMsg::Fast(step, w, tick, mine));
                        inbox.sim.extend(queue);
                        break;
                    }
                    combat.prev_flights.clear();
                    combat.prev_flights.extend_from_slice(combat.sim.flights());
                    combat.prev_proj = *combat.sim.projectiles();
                    combat.sim.apply_world_fast(&w);
                    combat.sim.set_host_step(step);
                    if let Some(mine) = &mine {
                        combat.sim.apply_pilot_snap(mine);
                    }
                    combat.sim.apply_pilot_tick(&tick);
                    for p in &w.pilots {
                        combat.remote_track.sample(p.id, step, p.view);
                    }
                    combat.remote_track.retain(&w.present);
                    let gap = combat.playout.applied(step);
                    for _ in 0..gap.min(MAX_STEPS) {
                        combat.sim.mirror_frame();
                    }
                    combat.sim.set_local_player(&ps);
                    combat.steps += 1;
                    combat.record.sample(&combat.sim);
                    wreck_fire(combat);
                }
            }
        }
    }
    while link.as_deref().is_none_or(|l| !l.is_client())
        && combat.accum >= dt
        && combat.sim.mission_end().is_none()
    {
        if combat.steps >= MAX_STEPS {
            combat.accum = 0.0;
            break;
        }
        combat.accum -= dt;
        combat.steps += 1;
        let cmd = std::mem::take(&mut combat.pending);
        let ps =
            player_state(&a, pos, controls.gun, controls.fire, collision.0, &hud, &mfd, &view, cmd);
        combat.prev_flights.clear();
        combat.prev_flights.extend_from_slice(combat.sim.flights());
        combat.prev_proj = *combat.sim.projectiles();
        let aaa_before: Vec<bool> = combat.sim.aaa_units().iter().map(|u| u.destroyed).collect();
        // A listen server steps the joined players' aircraft too.
        let mut inputs = Vec::new();
        if let Some(NetLink::Host(server)) = link.as_deref_mut() {
            inputs = server.inputs();
        }
        inputs.push((0, ps));
        let events = combat.sim.step_pilots(&inputs, FS);
        if let Some(NetLink::Host(server)) = link.as_deref_mut() {
            server.after_step(&events);
        }
        burn_aaa(combat, &aaa_before);
        for e in events {
            if campaign {
                combat.campaign_events.push(e.clone());
            }
            // The other pilots' own messages, sounds and warnings are theirs.
            if e.event.is_private() && e.pilot != 0 {
                continue;
            }
            take_event(combat, e.event, &mut a, &mut fx);
        }
        combat.record.sample(&combat.sim);
        wreck_fire(combat);
        // The other pilots as of this step (a listen server's).
        combat.host_frame = combat.host_frame.wrapping_add(1);
        if link.as_deref().is_some() {
            let me = combat.sim.active_pilot();
            let others = combat.sim.remote_pilots();
            let ids: Vec<u8> = others.iter().map(|p| p.id).collect();
            for p in others.iter().filter(|p| p.id != me) {
                combat.remote_track.sample(p.id, combat.host_frame, p.view);
            }
            combat.remote_track.retain(&ids);
        }
    }
    if link.as_deref().is_some_and(|l| l.is_client()) {
        combat.own_alpha = (combat.accum / dt).clamp(0.0, 1.0);
        combat.alpha = combat.playout.alpha();
        combat.frame_gap = combat.playout.span() as f32;
        combat.remote_time = combat.playout.now().unwrap_or(0.0);
    } else {
        combat.alpha = (combat.accum / dt).clamp(0.0, 1.0);
        combat.own_alpha = combat.alpha;
        combat.frame_gap = 1.0;
        combat.remote_time = combat.host_frame as f64 - 1.0 + combat.alpha as f64;
    }
    crate::sim_render::smooth_projectiles(combat, time.delta_secs());
    // The flight model can end the flight on its own (its ground and runway model differs in
    // detail); if the simulation does not follow within 3 s, end the mission anyway.
    // After an ejection the pilotless aircraft may crash: the parachute and the rescue go on.
    if let Some(end) = a.ended
        && combat.end.is_none()
        && combat.sim.chute().is_none()
        && !campaign
    {
        combat.flight_end_timer += time.delta_secs();
        if combat.flight_end_timer > 3.0 {
            combat.sim.quit();
            combat.end = Some(end_info(&combat.sim, Some(end)));
        }
    }
}

/// `--log-flight` / `--log-sim`: simulation events on stdout (test automation).
fn log_event(data: &SimData, e: &SimEvent, t: f32) {
    match e {
        SimEvent::Sound { .. }
        | SimEvent::Rwr(_)
        | SimEvent::Log(_)
        | SimEvent::CancelTimeAccel
        | SimEvent::RoundImpact { .. } => {}
        SimEvent::MissileLaunched { slot, spec, launcher } => {
            println!("sim t={t:.1} launch slot {slot} {} by {launcher:?}", data.missile(*spec).name)
        }
        e => println!("sim t={t:.1} {e:?}"),
    }
}

/// Text for the end overlay.
trait EndSource {
    fn describe(self) -> String;
}

impl EndSource for Option<MissionEnd> {
    fn describe(self) -> String {
        match self {
            Some(e) => match (e.outcome, e.landing_type) {
                (0, 2) => "Ejected - landed by parachute".into(),
                (0, 4) => "Ejected - rescued by CSAR".into(),
                (8, _) => "Ejected - captured".into(),
                (0, _) => "Safe landing".into(),
                (1, _) => "Crashed".into(),
                (2, _) => "Collision".into(),
                (5, _) => "Bad landing".into(),
                (6, _) => "Ejection failed".into(),
                _ => "Mission aborted".into(),
            },
            None => "Mission aborted".into(),
        }
    }
}

impl EndSource for Option<FlightEnd> {
    fn describe(self) -> String {
        match self {
            Some(FlightEnd::Landed) => "Safe landing".into(),
            Some(FlightEnd::BadLanding) => "Bad landing".into(),
            Some(FlightEnd::Crash) => "Crashed".into(),
            None => "Mission aborted".into(),
        }
    }
}

fn end_info(sim: &Sim, how: impl EndSource) -> EndInfo {
    let o = sim.objectives();
    let done = |b: bool| if b { "accomplished" } else { "not accomplished" };
    // Debrief log kinds: 1/12 world object, 2 tile object, 3 aircraft.
    let (mut air, mut ground) = (0, 0);
    for e in sim.event_log() {
        match e.kind & 0x3F {
            3 => air += 1,
            1 | 2 | 12 => ground += 1,
            _ => {}
        }
    }
    let d = sim.damage_effects();
    let mut lines = vec![
        format!("Primary objective {}", done(o.primary_done)),
        format!("Secondary objective {}", done(o.secondary_done)),
        format!("Kills: {air} aircraft, {ground} ground targets"),
        format!("Mission time {}:{:02}", sim.mission_tick() / 60, sim.mission_tick() % 60),
    ];
    if d.bits != 0 {
        lines.push(format!("Damage: {} hits", d.hits));
    }
    if sim.invulnerable() {
        lines.push("Training mission".into());
    }
    EndInfo { title: how.describe(), lines }
}

#[derive(SystemParam)]
struct Outputs<'w, 's> {
    player: Query<'w, 's, (&'static mut Aircraft, &'static WorldPos), With<Player>>,
    stores: ResMut<'w, Stores>,
    wps: ResMut<'w, Waypoints>,
    msgs: ResMut<'w, HudMessages>,
    clock: ResMut<'w, MissionClock>,
    emv: ResMut<'w, Emv>,
    warnings: ResMut<'w, Warnings>,
    cm: ResMut<'w, Countermeasures>,
    contacts: ResMut<'w, Contacts>,
    cam: ResMut<'w, TrackCam>,
    cues: ResMut<'w, HudCues>,
    hud: Res<'w, HudState>,
    view: Res<'w, ViewState>,
    accel: Res<'w, TimeAccel>,
    settings: Res<'w, crate::settings::Settings>,
    emitters: ResMut<'w, crate::rwr::Emitters>,
    ships: Option<Res<'w, crate::world::Ships>>,
    tiles: Res<'w, TileOverrides>,
}

/// Map units -> world units (east, north).
pub fn map_wu(x: i32, y: i32) -> (f64, f64) {
    (x as f64 * 32.0, (0x8000 - y) as f64 * 32.0)
}

/// Interpolated world position (wu east, wu north, viewZ) of a flight unit.
pub fn unit_position(prev: Option<&FlightUnit>, u: &FlightUnit, alpha: f32) -> (f64, f64, f64) {
    let cur = (u.wx as f64, (0x100000 - u.wy) as f64, u.alt as f64);
    let Some(p) = prev.filter(|p| (p.wx - u.wx).abs() < 0x4000 && (p.wy - u.wy).abs() < 0x4000)
    else {
        return cur;
    };
    let old = (p.wx as f64, (0x100000 - p.wy) as f64, p.alt as f64);
    let t = alpha as f64;
    (old.0 + (cur.0 - old.0) * t, old.1 + (cur.1 - old.1) * t, old.2 + (cur.2 - old.2) * t)
}

/// Projects a world point (wu east, north, viewZ) seen from the eye point (aircraft frame) into
/// the 320x200 cockpit screen (f = 256, vertical 0.75 f, centre (159, 52)); None behind the eye.
pub fn project_cockpit(
    a: &Aircraft,
    pos: &WorldPos,
    eye: Vec3,
    east: f64,
    north: f64,
    alt: f64,
) -> Option<(i32, i32)> {
    let eye = pos.0 + (a.orientation * eye).as_dvec3();
    let d: DVec3 = wu_to_bevy(east, north, alt) - eye;
    let l = a.orientation.inverse() * d.as_vec3();
    let z = -l.z;
    if z < 1.0 {
        return None;
    }
    let sx = (160.0 + crate::cockpit::FOCAL * l.x / z).floor();
    let sy = (52.0 - crate::cockpit::VSCALE * crate::cockpit::FOCAL * l.y / z).round();
    (sx.abs() < 10000.0 && sy.abs() < 10000.0).then_some((sx as i32, sy as i32))
}

/// Direction from the cockpit eye to a world point (wu east, north, viewZ) in the aircraft
/// frame (x right, y up, -z forward), normalised.
pub fn direction_cockpit(
    a: &Aircraft,
    pos: &WorldPos,
    eye: Vec3,
    east: f64,
    north: f64,
    alt: f64,
) -> Option<Vec3> {
    let eye = pos.0 + (a.orientation * eye).as_dvec3();
    let d: DVec3 = wu_to_bevy(east, north, alt) - eye;
    (a.orientation.inverse() * d.as_vec3()).try_normalize()
}

fn on_hud(p: Option<(i32, i32)>) -> Option<(i32, i32)> {
    p.filter(|&(x, y)| (0..320).contains(&x) && (0..91).contains(&y))
}

/// Tracking-camera zoom shift (focal 256 << z) for a range in map units.
/// ASSUMED (the original's zoom rule is not decoded): the target stays a few pixels to a
/// quarter of the MFD wide, i.e. zoom up one step per doubling of the range beyond 2 km.
pub fn camera_zoom(range: i32) -> u8 {
    let mut z = 0u8;
    while z < 6 && range >= (32 << z) {
        z += 1;
    }
    z
}

/// Publishes the simulation state into the cockpit and the flight model (every frame).
fn publish(mut combat: ResMut<Combat>, out: Outputs) {
    let o = combat.sim.objectives();
    let waypoint_changed = o.waypoint != combat.sim_waypoint;
    combat.sim_waypoint = o.waypoint;
    let combat = &*combat;
    let Outputs {
        mut player,
        mut stores,
        mut wps,
        mut msgs,
        mut clock,
        mut emv,
        mut warnings,
        mut cm,
        mut contacts,
        mut cam,
        mut cues,
        hud,
        view,
        accel,
        settings,
        mut emitters,
        ships,
        tiles,
    } = out;
    let eye = settings.cockpit_eye();
    let sim = &combat.sim;
    let data = &combat.data;
    let Ok((mut a, pos)) = player.single_mut() else { return };

    // Flight model: bay doors and damage.
    a.bay_open = sim.bay_doors_open();
    let d = sim.damage_effects();
    let damage = FlightDamage {
        throttle_cap: d.throttle_cap.map(|c| c as f32),
        stick_jitter: d.stick_jitter as f32,
        autopilot_failed: d.autopilot_failed,
        fuel_leak: d.fuel_leak as f32,
        heavy_stall: d.heavy_stall,
        fragile: d.fragile,
        roll_trim: d.controls.roll as f32 / 1000.0,
        pitch_trim: d.controls.pitch as f32 / 1000.0,
        roll_swing: d.controls.roll_swing as f32 / 1000.0,
        pitch_swing: d.controls.pitch_swing as f32 / 1000.0,
        swing_period: d.controls.period as f32 / 10.0,
    };
    if a.damage != damage {
        a.damage = damage;
    }

    // Stores (one entry per weapon carried) and gun.
    let (groups, cur) = sim.weapon_groups();
    let cur = cur.unwrap_or(0);
    let new_bays: Vec<Bay> = groups
        .iter()
        .map(|&(w, count)| {
            let wt = &data.weapons[(w.max(0) as usize).min(data.weapons.len() - 1)];
            Bay {
                name: wt.long_name.clone(),
                count: count.max(0) as u32,
                picture: wt.picture.max(0) as usize,
                air_to_air: wt.spec > 0 && data.missile(wt.spec as i32).class == 7,
                range_km: if wt.spec > 0 {
                    data.missile(wt.spec as i32).lock_range.max(0) as f32
                } else {
                    0.0
                },
            }
        })
        .collect();
    let gun = (!combat.lockheed).then(|| sim.gun_ammo().max(0) as u32);
    let gun_selected = sim.gun_selected();
    if stores.selected != cur
        || stores.gun_selected != gun_selected
        || stores.gun_rounds != gun
        || stores.bays.len() != new_bays.len()
        || stores.bays.iter().zip(&new_bays).any(|(a, b)| a.count != b.count || a.name != b.name)
    {
        stores.bays = new_bays;
        stores.selected = cur;
        stores.gun_rounds = gun;
        stores.gun_selected = gun_selected;
    }

    // Waypoints: the mission logic switches to the next one when an objective is done.
    if waypoint_changed {
        let w = wps.for_mission_waypoint(o.waypoint.max(0) as usize);
        wps.current = w;
        wps.mission_current = w;
    }
    msgs.training = sim.invulnerable();
    msgs.accel = accel.on;
    match combat.sky_seconds {
        Some(s) => *clock = MissionClock { start_seconds: 0.0, elapsed: Some(s) },
        None => clock.elapsed = Some(sim.mission_tick() as f32),
    }

    // Panel: EMV gauge, lights, countermeasures.
    let (egr, ear) = sim.signal_bars();
    *emv = Emv {
        emv: sim.emv().clamp(0, 99) as f32 / 99.0,
        egr: egr.map_or(0.0, |b| b.length.clamp(0, 99) as f32 / 99.0),
        ear: ear.map_or(0.0, |b| b.length.clamp(0, 99) as f32 / 99.0),
    };
    let l = sim.lights();
    let bits = d.bits;
    *warnings = Warnings {
        radar_missile: l.rad,
        ir_missile: l.ir,
        tracked: l.trak,
        lock: l.lock,
        missile_warning: bits & dmg::MW != 0,
        engine: bits & dmg::ENG != 0,
        fire_control: bits & dmg::FC != 0,
        jammer: bits & dmg::JAM != 0,
        fire: bits & dmg::FIRE != 0,
        fuel: false,
        bay_doors: bits & dmg::BD != 0,
        avionics: bits & dmg::AV != 0,
        emv_warning: l.emv_warning,
    };
    let (flares, chaff, decoys) = sim.stores();
    let lamp = |on: bool| if on { 1.0 } else { 0.0 };
    *cm = Countermeasures {
        chaff: chaff.max(0) as u32,
        flares: flares.max(0) as u32,
        decoys: decoys.max(0) as u32,
        ecm: sim.ecm(),
        ir_jammer: sim.ir_jammer(),
        active: [lamp(l.chaff), lamp(l.flare), lamp(l.decoy)],
    };

    // MFD contacts: aircraft, missiles, ground radars and the mission targets.
    contacts.list.clear();
    contacts.radar_arc =
        sim.radar_arc().filter(|a| a.frames_left > 0).map(|a| crate::mfd::MapArc {
            x: a.x as f64,
            y: a.y as f64,
            radius: a.radius_km as f64 * 64.0,
            arc: a.arc.map(|(s, e)| (s as u8, e as u8)),
            colour: a.colour,
        });
    for (i, u, _) in sim.active_aircraft() {
        if u.flags & fflags::AIRBORNE == 0 || u.flags & fflags::DESTROYED != 0 {
            continue;
        }
        let (east, north, alt) = unit_position(combat.prev_flights.get(i), &u, combat.alpha);
        let kind = if sim.drones().is_some_and(|d| d.contains(i)) {
            ContactKind::Drone
        } else {
            ContactKind::Aircraft
        };
        let friendly = sim.friendly_aircraft(u.spec);
        contacts.list.push(Contact {
            kind,
            east,
            north,
            alt: alt as f32,
            heading: u.heading as u16,
            friendly,
            target: Some(ContactRef::Unit(i)),
            threat: None,
        });
    }
    // The other players: friendly aircraft (never a lock), and their weapons in flight.
    let me = sim.active_pilot();
    for p in sim.remote_pilots().iter().filter(|p| p.id != me && p.chute.is_none()) {
        if !p.on_ground {
            let v = combat.remote_track.view_at(p.id, combat.remote_time, p.view);
            contacts.list.push(Contact {
                kind: ContactKind::Aircraft,
                east: v.wx as f64,
                north: v.wy as f64,
                alt: v.view_z as f32,
                heading: v.heading as u16,
                friendly: true,
                target: None,
                threat: None,
            });
        }
        for m in &p.missiles {
            let (east, north) = map_wu(m.x, m.y);
            contacts.list.push(Contact {
                kind: ContactKind::Missile,
                east,
                north,
                alt: m.alt as f32,
                heading: m.heading as u16,
                friendly: true,
                target: None,
                threat: None,
            });
        }
    }
    for (_, m, _, _) in sim.active_projectiles() {
        let (east, north) = map_wu(m.x, m.y);
        contacts.list.push(Contact {
            kind: ContactKind::Missile,
            east,
            north,
            alt: m.alt as f32,
            heading: m.heading as u16,
            friendly: false,
            target: None,
            threat: None,
        });
    }
    let objects = sim.objects();
    for (i, ob) in objects.iter().enumerate().take(sim.ground_unit_count()).skip(3) {
        if ob.unit_type > 0
            && ob.flags & (oflags::DESTROYED | oflags::FRIENDLY | oflags::NEUTRAL) == 0
            && i < objects.len()
            && !sim.object_is_ship(i)
        {
            let (east, north) = map_wu(ob.x, ob.y);
            let label = crate::rwr::rwr_label(sim.threat_name(ob.unit_type), false);
            let mut sym = [0u8; 3];
            for (d, s) in sym.iter_mut().zip(label.bytes()) {
                *d = s;
            }
            let ring_km = crate::rwr::km(sim.site_lock_range(ob.unit_type));
            let threat = Some((ring_km, sym));
            contacts.list.push(Contact {
                kind: ContactKind::GroundRadar,
                east,
                north,
                alt: 0.0,
                heading: 0,
                friendly: false,
                target: Some(ContactRef::Object(i)),
                threat,
            });
        }
    }
    // Ships (on the datalink): the world's ship objects (armed ones with their threat ring)
    // and the theater's ships among the tile objects, lockable unless friendly.
    let mut ship_at = Vec::new();
    for (i, ob) in objects.iter().enumerate().skip(3) {
        if ob.flags & oflags::DESTROYED != 0 || !sim.object_is_ship(i) || (ob.x, ob.y) == (0, 0) {
            continue;
        }
        let (east, north) = map_wu(ob.x, ob.y);
        let friendly = ob.flags & (oflags::FRIENDLY | oflags::NEUTRAL) != 0;
        let armed = ob.unit_type > 0 && i < sim.ground_unit_count() && !friendly;
        let threat = armed.then(|| (crate::rwr::km(sim.site_lock_range(ob.unit_type)), [0u8; 3]));
        let target = (!friendly).then_some(ContactRef::Object(i));
        contacts.list.push(Contact {
            kind: ContactKind::Ship,
            east,
            north,
            alt: 0.0,
            heading: 0,
            friendly,
            target,
            threat,
        });
        ship_at.push((ob.x, ob.y));
    }
    for s in ships.iter().flat_map(|s| s.0.iter()) {
        let (x, y) = s.map_xy();
        if ship_at.iter().any(|&(ox, oy)| (ox - x).abs() < 2 && (oy - y).abs() < 2)
            || tiles.shapes.get(&s.key).is_some_and(|&sh| sh != s.shape as i32)
        {
            continue;
        }
        // Friendly where the world has a friendly or neutral object (a carrier).
        let friendly = objects.iter().any(|o| {
            (o.x - x).abs() < 2
                && (o.y - y).abs() < 2
                && o.flags & (oflags::FRIENDLY | oflags::NEUTRAL) != 0
        });
        let target = (!friendly).then_some(ContactRef::Point(x, y));
        contacts.list.push(Contact {
            kind: ContactKind::Ship,
            east: s.east,
            north: s.north,
            alt: 0.0,
            heading: 0,
            friendly,
            target,
            threat: None,
        });
    }
    let heading_bam = to_bam(a.euler().0);
    emitters.list = sim
        .emitters()
        .iter()
        .map(|e| {
            let name = match e.source {
                stealth_sim::EmitterSource::Site(_) => sim.threat_name(e.radar_type).to_string(),
                stealth_sim::EmitterSource::Aircraft(i) => sim.aircraft_name(sim.flights()[i].spec),
                stealth_sim::EmitterSource::Gun(_) => "ZSU-23-4".to_string(),
            };
            crate::rwr::view(e, &name, heading_bam)
        })
        .collect();
    let done = [o.primary_done, o.secondary_done];
    for (k, t) in sim.target_slots().iter().enumerate() {
        if done[k] || !(1..=3).contains(&t.mission_type) {
            continue;
        }
        if let Some((i, ob)) =
            usize::try_from(t.object).ok().and_then(|i| Some((i, objects.get(i)?)))
            && ob.flags & oflags::DESTROYED == 0
            && (ob.x, ob.y) != (0, 0)
        {
            let (east, north) = map_wu(ob.x, ob.y);
            contacts.list.push(Contact {
                kind: ContactKind::Target,
                east,
                north,
                alt: 0.0,
                heading: 0,
                friendly: false,
                target: Some(ContactRef::Object(i)),
                threat: None,
            });
        }
    }
    let locks = sim.targeting();
    contacts.locked = locks.air.map(ContactRef::Unit).or(locks.ground.map(|g| {
        // A ship tile object locked by its point: the lock box goes on its symbol.
        let o = objects.get(g).copied().unwrap_or_default();
        let near = |k: &Contact| matches!(k.target, Some(ContactRef::Point(x, y)) if (x - o.x).abs() <= 1 && (y - o.y).abs() <= 1);
        contacts.list.iter().find(|k| near(k)).and_then(|k| k.target).unwrap_or(ContactRef::Object(g))
    }));

    // Tracking camera data and HUD cues.
    let t = sim.targeting();
    let player_view = sim.player();
    let (h, _, _) = a.euler();
    let heading = to_bam(h) as i32;
    let mut info = TrackCam { live: cam.live, ..default() };
    let mut new_cues = HudCues::default();
    let cockpit = view.view == View::Cockpit;
    let spec = groups.get(cur).map_or(0, |&(w, _)| {
        data.weapons[(w.max(0) as usize).min(data.weapons.len() - 1)].spec as i32
    });
    if let Some(g) = t.ground.filter(|&g| g < objects.len()) {
        let ob = objects[g];
        let place = sim.place_name(g);
        let kind = combat.names.get((ob.shape & 0x7F) as usize).cloned().unwrap_or_default();
        info.name = if place.eq_ignore_ascii_case(&kind) || place.is_empty() {
            [kind, String::new()]
        } else {
            [place, kind]
        };
        info.range_km = Some(t.range as f32 * 32.0 * WU as f32 / 1000.0);
        info.bearing = Some(((t.bearing as u16) as u32 * 360 / 65536) as i32);
        info.zoom = camera_zoom(t.range) + 1;
        let (east, north) = map_wu(ob.x, ob.y);
        if cockpit && hud.mode == HudMode::Gnd && t.box_size > 0 {
            new_cues.target_dir = direction_cockpit(&a, pos, eye, east, north, 0.0);
            new_cues.target_box =
                on_hud(project_cockpit(&a, pos, eye, east, north, 0.0)).map(|(x, y)| TargetBox {
                    x,
                    y,
                    size: t.box_size,
                    colour: t.box_colour,
                    oval: t.lock_valid,
                    red: t.in_range,
                });
        }
        if cockpit && hud.mode == HudMode::Gnd && spec == 28 {
            // Flight-path guide: 128 map units beyond the target along the heading, own altitude.
            let hr = bam(heading as f32);
            let (qx, qy) =
                (ob.x as f64 + hr.sin() as f64 * 128.0, ob.y as f64 - hr.cos() as f64 * 128.0);
            new_cues.diamond_dir = direction_cockpit(
                &a,
                pos,
                eye,
                qx * 32.0,
                (0x8000 as f64 - qy) * 32.0,
                player_view.view_z as f64,
            );
            new_cues.diamond = on_hud(project_cockpit(
                &a,
                pos,
                eye,
                qx * 32.0,
                (0x8000 as f64 - qy) * 32.0,
                player_view.view_z as f64,
            ));
        }
    } else if let Some(u) = t.air.and_then(|i| sim.flights().get(i).map(|u| (i, *u))) {
        let (i, u) = u;
        let ty = &sim.aircraft_types[(u.spec.max(0) as usize).min(sim.aircraft_types.len() - 1)];
        info.name = [ty.name.clone(), ty.nato.clone()];
        let (east, north, alt) = unit_position(combat.prev_flights.get(i), &u, combat.alpha);
        let (pe, pn, _) = bevy_to_wu(pos.0);
        let range_mu = ((east - pe).hypot(north - pn) / 32.0) as i32;
        info.range_km = Some(range_mu as f32 * 32.0 * WU as f32 / 1000.0);
        let brg = to_bam(((east - pe) as f32).atan2((north - pn) as f32)) as i32;
        info.bearing = Some(((brg as u16) as u32 * 360 / 65536) as i32);
        info.zoom = camera_zoom(range_mu) + 1;
        if cockpit && hud.mode == HudMode::Air {
            new_cues.target_dir = direction_cockpit(&a, pos, eye, east, north, alt);
            new_cues.target_box =
                on_hud(project_cockpit(&a, pos, eye, east, north, alt)).map(|(x, y)| TargetBox {
                    x,
                    y,
                    size: 9,
                    colour: t.box_colour,
                    oval: t.lock_valid,
                    red: t.in_range,
                });
        }
    }
    if cockpit {
        if hud.mode == HudMode::Gnd
            && !sim.gun_selected()
            && let Some(cue) = bomb_cue(
                &a,
                pos,
                eye,
                spec,
                sim.opts.fix_original_bugs,
                hud.ccrp,
                t.ground.and_then(|g| objects.get(g)),
                &mut new_cues,
            )
        {
            new_cues.bomb = Some(cue);
        }
        new_cues.ranging_bar = t.ranging_bar;
        new_cues.lock = t.lock_valid;
        new_cues.in_range = t.in_range;
        if sim.gun_selected() {
            // The locked aircraft (position and velocity over the last sim frame) or ground
            // object, else a target 1000 m ahead flying along with the aircraft.
            let air = t.air.is_some_and(|i| i < sim.flights().len());
            let target = if let Some(i) = t.air.filter(|&i| i < sim.flights().len()) {
                let u = sim.flights()[i];
                let (e, n, alt) = unit_position(combat.prev_flights.get(i), &u, combat.alpha);
                let v = combat.prev_flights.get(i).map_or(DVec3::ZERO, |p| {
                    let (e0, n0, alt0) = unit_position(None, p, 0.0);
                    let (e1, n1, alt1) = unit_position(None, &u, 0.0);
                    (wu_to_bevy(e1, n1, alt1) - wu_to_bevy(e0, n0, alt0)) * FS as f64
                });
                Some((wu_to_bevy(e, n, alt), v))
            } else {
                t.ground.and_then(|g| objects.get(g)).map(|ob| {
                    let (e, n) = map_wu(ob.x, ob.y);
                    (wu_to_bevy(e, n, 0.0), DVec3::ZERO)
                })
            };
            let ground_y = wu_to_bevy(0.0, 0.0, a.ground_altitude as f64).y;
            new_cues.gun = Some(gun_cue(&a, pos, eye, target, air, ground_y));
        }
    }
    *cam = info;
    *cues = new_cues;
}

/// Gun pipper: the direction in which the target (position, velocity; Bevy frame) has to be
/// for the rounds fired now to hit it. A round leaves along the nose at the muzzle speed plus
/// the aircraft's speed and falls (`stealth_sim` gun), so with the target at range R the
/// pipper is D(t) = (u - v) t + g t^2 / 2 with |D(t)| = R.
///
/// Unless an aircraft is locked (`air`), when the rounds reach the ground (at height `ground_y`)
/// within `GROUND_RANGE` the pipper is where they hit it, with the range to that point.
fn gun_cue(
    a: &Aircraft,
    pos: &WorldPos,
    eye: Vec3,
    target: Option<(DVec3, DVec3)>,
    air: bool,
    ground_y: f64,
) -> crate::hud::GunCue {
    const DEFAULT_RANGE: f64 = 1000.0;
    /// Range the rounds are effective to (m).
    const GUN_RANGE: f64 = 2000.0;
    /// Farthest ground impact the pipper shows (m).
    const GROUND_RANGE: f64 = 4000.0;
    let from = pos.0 + (a.orientation * eye).as_dvec3();
    let nose = (a.orientation * Vec3::NEG_Z).as_dvec3();
    let speed = a.knots() as f64 * 0.514_444;
    let u = nose * (stealth_sim::GUN_MUZZLE_SPEED + speed);
    let g = DVec3::new(0.0, -stealth_sim::GUN_GRAVITY, 0.0);
    let cue = |d: DVec3, range: f64, range_m: Option<f32>, in_range: bool| {
        let dir = d.normalize_or_zero();
        let (e, n, alt) = bevy_to_wu(from + dir * range);
        crate::hud::GunCue {
            dir: a.orientation.inverse() * dir.as_vec3(),
            screen: project_cockpit(a, pos, eye, e, n, alt),
            range_m,
            in_range,
        }
    };
    if !air && let Some(d) = ground_impact(from, u, ground_y).filter(|d| d.length() <= GROUND_RANGE)
    {
        let range = d.length();
        return cue(d, range, Some(range as f32), target.is_some() && range < GUN_RANGE);
    }
    let (range, v) = match target {
        Some((p, v)) => ((p - from).length(), v),
        None => (DEFAULT_RANGE, a.path_dir().as_dvec3() * speed),
    };
    let rel = u - v;
    let mut t = range / rel.length().max(1.0);
    let mut d = rel * t + g * (0.5 * t * t);
    for _ in 0..4 {
        t *= range / d.length().max(1e-6);
        d = rel * t + g * (0.5 * t * t);
    }
    cue(d, range, target.map(|_| range as f32), target.is_some() && range < GUN_RANGE)
}

/// Bombsight for an unguided bomb (`stealth_sim::ballistics`, as the simulation releases it):
/// CCIP puts the pipper (`cues.ccip_dir`, `cues.ccip`) where a bomb released now lands; CCRP
/// gives the time to the release point on the locked `target`.
#[allow(clippy::too_many_arguments)]
fn bomb_cue(
    a: &Aircraft,
    pos: &WorldPos,
    eye: Vec3,
    spec: i32,
    fix_original_bugs: bool,
    ccrp: bool,
    target: Option<&stealth_sim::GroundObject>,
    cues: &mut HudCues,
) -> Option<crate::hud::BombCue> {
    use stealth_sim::ballistics;
    let k = ballistics::drag(spec, fix_original_bugs)?;
    let heading = to_bam(a.euler().0) as i32;
    let v = ballistics::release_velocity(heading, a.velocity as i32, a.climb_rate as i32);
    let (d, fall) =
        ballistics::impact(v, k, a.altitude as f64 - ballistics::RELEASE_DROP, 1.0 / FS as f64)?;
    let (east, north, _) = bevy_to_wu(pos.0);
    // The offset is east and south.
    let (ie, inn) = (east + d[0], north - d[1]);
    let mut cue =
        crate::hud::BombCue { ccrp, fall_secs: fall as f32, range_m: None, release_secs: None };
    if ccrp {
        let vh = v[0].hypot(v[1]);
        if let Some(o) = target.filter(|_| vh > 1.0) {
            let (te, tn) = ((o.x as f64 + 0.5) * 32.0, (0x8000 as f64 - o.y as f64 - 0.5) * 32.0);
            let along = ((te - ie) * v[0] - (tn - inn) * v[1]) / vh;
            cue.release_secs = Some((along / vh) as f32);
            cue.range_m =
                Some(((te - east).hypot(tn - north).hypot(a.altitude as f64) * WU) as f32);
        }
    } else {
        cues.ccip_dir = direction_cockpit(a, pos, eye, ie, inn, 0.0);
        cues.ccip =
            Some(on_hud(project_cockpit(a, pos, eye, ie, inn, 0.0)).unwrap_or((0xA0, 0x5A)));
        cue.range_m = Some(((ie - east).hypot(inn - north).hypot(a.altitude as f64) * WU) as f32);
    }
    Some(cue)
}

/// Where rounds fired from `from` with velocity `u` hit the ground plane at height `ground_y`,
/// relative to `from` (None from below it).
fn ground_impact(from: DVec3, u: DVec3, ground_y: f64) -> Option<DVec3> {
    let h = from.y - ground_y;
    if h <= 0.0 {
        return None;
    }
    let g = stealth_sim::GUN_GRAVITY;
    let t = (u.y + (u.y * u.y + 2.0 * g * h).sqrt()) / g;
    Some(u * t + DVec3::new(0.0, -0.5 * g * t * t, 0.0))
}

/// The end-of-mission overlay.
#[derive(Component)]
struct EndOverlay {
    timer: f32,
}

/// Seconds the overlay stays before returning to the menu (any key, or in VR the trigger or A / X,
/// after 1 s; at once after END MISSION in the in-flight menu, which shows EXITING... over it).
const END_SECONDS: f32 = 8.0;

#[allow(clippy::too_many_arguments)]
fn end_overlay(
    mut commands: Commands,
    combat: Option<Res<Combat>>,
    real: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay: Query<&mut EndOverlay>,
    mut next: ResMut<NextState<AppState>>,
    mission: Option<Res<ActiveMission>>,
    menu: Res<crate::flight_menu::FlightMenu>,
    hands: Res<crate::cockpit3d::Hands>,
    settings: Res<crate::settings::Settings>,
) {
    let Some(end) = combat.as_ref().and_then(|c| c.end.clone()) else { return };
    let vr_pressed = hands
        .hands
        .iter()
        .any(|h| h.trigger_pulled() || h.just_pressed(crate::cockpit3d::BUTTON_PRIMARY));
    match overlay.single_mut() {
        Ok(mut o) => {
            o.timer += real.delta_secs();
            if menu.exiting
                || o.timer > END_SECONDS
                || (o.timer > 1.0 && (keys.get_just_pressed().next().is_some() || vr_pressed))
            {
                if let Some(c) = &combat {
                    if settings.tacview
                        && let Some(m) = &mission
                    {
                        crate::tacview::export(&c.record, &m.0, &c.data, settings.aircraft.name());
                    }
                    commands.insert_resource(crate::debrief::Debrief {
                        title: end.title.clone(),
                        report: c.sim.mission_report(),
                        damage: Some(c.sim.world_damage()),
                        mission: mission.as_ref().map(|m| m.0.clone()),
                        career: Vec::new(),
                        record: c.record.clone(),
                    });
                }
                next.set(AppState::Debrief);
            }
        }
        Err(_) => {
            info!("mission end: {} / {}", end.title, end.lines.join(" / "));
            if combat.as_ref().is_some_and(|c| c.log) {
                println!("mission end: {} / {}", end.title, end.lines.join(" / "));
            }
            let mut text = format!("{}\n\n", end.title.to_uppercase());
            for l in &end.lines {
                text.push_str(l);
                text.push('\n');
            }
            text.push_str("\nPress any key");
            commands
                .spawn((
                    EndOverlay { timer: 0.0 },
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                ))
                .with_children(|p| {
                    p.spawn((
                        Node { padding: UiRect::all(px(24)), ..default() },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
                    ))
                    .with_children(|p| {
                        p.spawn((
                            Text::new(text),
                            TextFont { font_size: bevy::text::FontSize::Px(28.0), ..default() },
                            TextColor(Color::srgb(1.0, 0.85, 0.3)),
                            TextLayout::justify(Justify::Center),
                        ));
                    });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(knots: f32) -> (Aircraft, WorldPos) {
        let mut a = Aircraft::new_on_runway(5000.0);
        a.velocity = knots * 27.0;
        (a, WorldPos(DVec3::new(0.0, 1000.0, 0.0)))
    }

    /// Mils of a direction off the boresight: (right, up).
    fn mils(d: Vec3) -> (f32, f32) {
        ((d.x / -d.z).atan() * 1000.0, (d.y / -d.z).atan() * 1000.0)
    }

    #[test]
    fn pipper_drops_with_range() {
        let (a, pos) = level(400.0);
        let v = DVec3::new(0.0, 0.0, -400.0 * 0.514_444);
        let at = |m: f64| {
            gun_cue(&a, &pos, Vec3::ZERO, Some((pos.0 + DVec3::new(0.0, 0.0, -m), v)), true, 0.0)
        };
        let (near, far) = (mils(at(500.0).dir), mils(at(1500.0).dir));
        // A target flying along: the rounds' drop only (g t / 2 v at 1050 m/s).
        let want = |m: f32| 0.5 * 9.81 * (m / 1050.0) / 1050.0 * 1000.0;
        assert!(near.0.abs() < 0.01 && (near.1 + want(500.0)).abs() < 0.2, "{near:?}");
        assert!((far.1 + want(1500.0)).abs() < 0.3, "{far:?}");
        assert!(at(1500.0).in_range && !at(2500.0).in_range);
        assert_eq!(at(1500.0).range_m.map(|r| r.round()), Some(1500.0));
    }

    #[test]
    fn pipper_leads_a_crossing_target() {
        let (a, pos) = level(400.0);
        // 1000 m ahead crossing left to right at 200 m/s: the target has to be left of where
        // the rounds go, by its travel during their flight.
        let cue = gun_cue(
            &a,
            &pos,
            Vec3::ZERO,
            Some((pos.0 + DVec3::new(0.0, 0.0, -1000.0), DVec3::new(200.0, 0.0, 0.0))),
            true,
            0.0,
        );
        let (right, _) = mils(cue.dir);
        let t = 1000.0 / (1050.0 + 400.0 * 0.514_444);
        let want = -(200.0f32 * t / 1000.0).atan() * 1000.0;
        assert!((right - want).abs() < 3.0, "{right} mils, want {want}");
        // No target: a target 1000 m ahead flying along.
        // No target, the ground far below the rounds' reach: a target 1000 m ahead flying along.
        let free = gun_cue(&a, &pos, Vec3::ZERO, None, false, 0.0);
        assert!(free.range_m.is_none() && !free.in_range);
        assert!(mils(free.dir).0.abs() < 0.01 && mils(free.dir).1 < 0.0);
    }

    #[test]
    fn pipper_on_the_ground_in_a_dive() {
        let (mut a, pos) = level(400.0);
        a.orientation = Quat::from_rotation_x(-30f32.to_radians());
        // 1000 m up in a 30 degree dive: the rounds hit the ground about 2000 m away, a little
        // short of the boresight.
        let cue = gun_cue(&a, &pos, Vec3::ZERO, None, false, 0.0);
        let range = cue.range_m.expect("ground impact") as f64;
        assert!((1900.0..2000.0).contains(&range), "{range}");
        let hit = pos.0 + (a.orientation * cue.dir).as_dvec3() * range;
        assert!(hit.y.abs() < 0.5, "{hit}");
        assert!(mils(cue.dir).1 < 0.0 && !cue.in_range);
        // A ground target locked: SHOOT within the gun's range.
        let target = Some((hit, DVec3::ZERO));
        assert!(gun_cue(&a, &pos, Vec3::ZERO, target, false, 0.0).in_range);
        // The ground 600 m higher: the impact is that much nearer.
        assert!(gun_cue(&a, &pos, Vec3::ZERO, None, false, 600.0).range_m.unwrap() < 900.0);
    }
}

/// Where the step's events go besides the simulation's own bookkeeping.
struct Fx<'a, 'w> {
    msgs: &'a mut HudMessages,
    accel: &'a mut TimeAccel,
    tiles: &'a mut TileOverrides,
    flash: &'a mut crate::hud::DamageFlash,
    sounds: &'a mut MessageWriter<'w, crate::sound::PlaySound>,
}

/// A destroyed gun or launcher burns like a vehicle.
fn burn_aaa(combat: &mut Combat, before: &[bool]) {
    let fires: Vec<Kill> = combat
        .sim
        .aaa_units()
        .iter()
        .zip(before)
        .filter(|(u, was)| u.destroyed && !**was)
        .map(|(u, _)| Kill { x: u.x, y: u.y, vehicle: true })
        .collect();
    combat.kills.extend(fires);
}

/// The falling wreck burns where it lands.
fn wreck_fire(combat: &mut Combat) {
    let w = combat.sim.wreck();
    if w.alt > 0 && w.x != 0 {
        combat.wreck = Some((w.x, w.y));
    } else if let Some((x, y)) = combat.wreck.take() {
        combat.kills.push(Kill { x, y, vehicle: true });
    }
}

/// One of this player's simulation events: messages, sounds, effects, fires, the end.
fn take_event(combat: &mut Combat, e: SimEvent, a: &mut Aircraft, fx: &mut Fx) {
    if combat.log {
        log_event(&combat.data, &e, a.time);
    }
    combat.record.event(&combat.sim, &e);
    match e {
        SimEvent::Message(s) => fx.msgs.show(s),
        SimEvent::CancelTimeAccel => fx.accel.on = false,
        SimEvent::DamageFlash => fx.flash.0 = 1,
        SimEvent::Sound { id, priority } => {
            fx.sounds.write(crate::sound::PlaySound { id, priority });
        }
        SimEvent::Rwr(tone) => {
            fx.sounds
                .write(crate::sound::PlaySound { id: crate::sound::rwr_id(tone), priority: 1 });
        }
        SimEvent::FuelTransfer { lbs } => a.fuel += lbs as f32,
        SimEvent::Resupplied => a.fuel = 5000.0,
        SimEvent::CollisionBounce => {
            a.altitude += 500.0;
            a.autopilot_alt = 0.0;
        }
        SimEvent::Explosion { x, y, alt, kind } => {
            combat.explosions.push((x, y, alt, kind));
            // Crashed aircraft leave a burning wreck.
            if kind == -16 {
                combat.kills.push(Kill { x, y, vehicle: true });
            }
        }
        // An S-60 shell burst: a black puff where it went off (true altitude, ft).
        SimEvent::Flak { x, y, alt } => {
            combat.explosions.push((x, y, alt, crate::explosions::FLAK));
            let p = combat.sim.player();
            if (x - p.mx).abs() + (y - p.my).abs() < 100 {
                fx.sounds
                    .write(crate::sound::PlaySound { id: crate::sound::AIRBURST, priority: 1 });
            }
        }
        SimEvent::RoundImpact { x, y } => {
            let fine = (32 * stealth_sim::ROUND_FINE) as f64;
            combat.round_impacts.push((x as f64 / fine, y as f64 / fine));
        }
        SimEvent::SignalFlare { x, y } => combat.signal_flares.push((x, y)),
        // A drone that got through sets its target area on fire.
        SimEvent::DroneImpact { x, y, .. } => combat.kills.push(Kill { x, y, vehicle: false }),
        SimEvent::TileDestroyed(t) => {
            fx.tiles.set(t.level as usize, t.col, t.row, t.index, t.shape)
        }
        SimEvent::TileRestored(t) => fx.tiles.clear(t.level as usize, t.col, t.row, t.index),
        SimEvent::GroundDestroyed { object, .. } => {
            if let Some(o) = combat.sim.objects().get(object) {
                let name = combat.names.get((o.shape & 0x7F) as usize).map_or("", |n| n.as_str());
                combat.kills.push(Kill {
                    x: o.x,
                    y: o.y,
                    vehicle: object != 0 && is_vehicle(name),
                });
            }
        }
        // Aircraft on the ground burn at once, airborne ones where the wreck lands.
        SimEvent::AircraftDestroyed { unit, .. } => {
            if let Some(u) = combat.sim.flights().get(unit).filter(|u| u.alt <= 8 && u.x != 0) {
                combat.kills.push(Kill { x: u.x, y: u.y, vehicle: true });
            }
        }
        SimEvent::MissionEnd(e) => {
            combat.end = Some(end_info(&combat.sim, Some(e)));
            if a.ended.is_none() {
                a.ended = Some(match e.outcome {
                    0 => FlightEnd::Landed,
                    5 => FlightEnd::BadLanding,
                    _ => FlightEnd::Crash,
                });
            }
        }
        _ => {}
    }
}
