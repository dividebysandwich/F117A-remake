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
    Commands as SimCommands, FlightUnit, HudMode as SimHud, LandingMode, MissionEnd, N_PROJ, PlayerState, Projectile, Sim,
    SimData, SimEvent, SimOptions, dmg, fflags, oflags,
};

use crate::camera::{View, ViewState};
use crate::flight::{Aircraft, FlightConstants, FlightDamage, FlightEnd, FlightSystems, Player, bam, to_bam};
use crate::hud::{Bay, HudCues, HudMessages, HudMode, HudState, Stores, TargetBox, Waypoint, Waypoints};
use crate::input::{Action, ActionEvent, FlightControls};
use crate::mfd::{Contact, ContactKind, Contacts, MfdState, MissionClock, MissionOrders, RightMfd, TrackCam};
use crate::mission::{ActiveMission, PlannedRoute, clean_text};
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
                (crate::mission::ensure_mission.in_set(SetupSet::Mission), start_combat.in_set(SetupSet::World)),
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
            .add_systems(Update, end_overlay.after(CombatSystems).run_if(in_state(AppState::Flight)));
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
    quit: bool,
    /// Flight units and projectiles before the last step, for render interpolation.
    pub prev_flights: Vec<FlightUnit>,
    pub prev_proj: [Projectile; N_PROJ],
    /// Fraction of a sim frame elapsed since the last step (0..1).
    pub alpha: f32,
    /// Explosions of the steps of this frame: (map x, map y, viewZ altitude, kind).
    pub explosions: Vec<(i32, i32, i32, i32)>,
    /// Signal flares the downed pilot fired in the steps of this frame (map x, map y).
    pub signal_flares: Vec<(i32, i32)>,
    /// Tile objects destroyed in earlier missions of a tour, still to be shown destroyed.
    prior_tiles: Vec<stealth_sim::TileDamage>,
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
    ["mobile", "tanks", "boat", "airplane", "ss missile", "scud", "trawler", "trail"].iter().any(|k| n.contains(k))
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

fn apply_time_accel(accel: Res<TimeAccel>, mut time: ResMut<Time<Virtual>>, combat: Option<Res<Combat>>) {
    // Waiting on the ground for the rescue goes faster.
    let fast = if combat.is_some_and(|c| c.sim.csar().is_some()) { 4.0 } else { 2.0 };
    let want = accel.base * if accel.on { fast } else { 1.0 };
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
    career: Option<Res<crate::career::CareerFlight>>,
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
    let prior_damage = career.and_then(|c| crate::career::Pilot::load(&c.0)).and_then(|p| p.tour).map(|t| t.damage);
    let prior_tiles = prior_damage.as_ref().map(|d| d.tiles.clone()).unwrap_or_default();
    let sim = Sim::with_options(&data, m, SimOptions { landing, prior_damage, ..default() });
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
    *clock = MissionClock { start_seconds: m.time.base_hour as f32 * 3600.0 + m.time.tod_ticks as f32 * 2.0, elapsed: Some(0.0) };
    // The route from the mission planner, else the mission's own waypoints.
    let route = route.filter(|r| r.fits(m)).map(|r| r.clone()).unwrap_or_else(|| PlannedRoute::of(m));
    *wps = Waypoints {
        points: route.points.iter().map(|p| Waypoint { east: p.x as f64 * 32.0, north: (0x8000 - p.y) as f64 * 32.0 }).collect(),
        current: 0,
        mission_current: 0,
        anchors: route.anchors(),
    };
    let names = m.world.names.clone();
    commands.insert_resource(Combat {
        prior_tiles,
        prev_flights: sim.flights().to_vec(),
        prev_proj: *sim.projectiles(),
        sim,
        data,
        accum: 0.0,
        pending: SimCommands::default(),
        quit: false,
        alpha: 0.0,
        explosions: Vec::new(),
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
fn collect_commands(mut actions: MessageReader<ActionEvent>, mut combat: ResMut<Combat>, mut accel: ResMut<TimeAccel>) {
    let combat = &mut *combat;
    let c = &mut combat.pending;
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
        camera_mfd: mfd.right == RightMfd::Camera,
        external_view: !is_cockpit_view(view.view),
        hud_visible: view.view == View::Cockpit,
        cmd: SimCommands { gun, ..cmd },
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
    hulls: Query<(&crate::models::CollisionHull, &crate::models::LodLevel, &crate::models::LodShown, &GlobalTransform, &InheritedVisibility)>,
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
    let StepParams { time, mut player, controls, hud, mfd, view, mut msgs, mut accel, mut tiles, collision, mut flash, mut sounds } = p;
    let combat = &mut *combat;
    combat.explosions.clear();
    combat.signal_flares.clear();
    for t in std::mem::take(&mut combat.prior_tiles) {
        tiles.set(t.level as usize, t.col, t.row, t.index, t.shape);
    }
    combat.kills.clear();
    combat.steps = 0;
    let Ok((mut a, pos)) = player.single_mut() else { return };
    if combat.quit && combat.sim.mission_end().is_none() {
        combat.sim.quit();
        combat.end = Some(end_info(&combat.sim, None::<MissionEnd>));
    }
    let dt = 1.0 / FS as f32;
    combat.accum += time.delta_secs();
    while combat.accum >= dt && combat.sim.mission_end().is_none() {
        if combat.steps >= MAX_STEPS {
            combat.accum = 0.0;
            break;
        }
        combat.accum -= dt;
        combat.steps += 1;
        let cmd = std::mem::take(&mut combat.pending);
        let ps = player_state(&a, pos, controls.gun, collision.0, &hud, &mfd, &view, cmd);
        combat.prev_flights.clear();
        combat.prev_flights.extend_from_slice(combat.sim.flights());
        combat.prev_proj = *combat.sim.projectiles();
        let events = combat.sim.step(&ps, FS);
        for e in events {
            if combat.log {
                log_event(&combat.data, &e, a.time);
            }
            match e {
                SimEvent::Message(s) => msgs.show(s),
                SimEvent::CancelTimeAccel => accel.on = false,
                SimEvent::DamageFlash => flash.0 = 1,
                SimEvent::Sound { id, priority } => {
                    sounds.write(crate::sound::PlaySound { id, priority });
                }
                SimEvent::Rwr(tone) => {
                    sounds.write(crate::sound::PlaySound { id: crate::sound::rwr_id(tone), priority: 1 });
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
                SimEvent::SignalFlare { x, y } => combat.signal_flares.push((x, y)),
                // A drone that got through sets its target area on fire.
                SimEvent::DroneImpact { x, y, .. } => combat.kills.push(Kill { x, y, vehicle: false }),
                SimEvent::TileDestroyed(t) => tiles.set(t.level as usize, t.col, t.row, t.index, t.shape),
                SimEvent::GroundDestroyed { object, .. } => {
                    if let Some(o) = combat.sim.objects().get(object) {
                        let name = combat.names.get((o.shape & 0x7F) as usize).map_or("", |n| n.as_str());
                        combat.kills.push(Kill { x: o.x, y: o.y, vehicle: object != 0 && is_vehicle(name) });
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
        let w = combat.sim.wreck();
        if w.alt > 0 && w.x != 0 {
            combat.wreck = Some((w.x, w.y));
        } else if let Some((x, y)) = combat.wreck.take() {
            combat.kills.push(Kill { x, y, vehicle: true });
        }
    }
    combat.alpha = (combat.accum / dt).clamp(0.0, 1.0);
    // The flight model can end the flight on its own (its ground and runway model differs in
    // detail); if the simulation does not follow within 3 s, end the mission anyway.
    // After an ejection the pilotless aircraft may crash: the parachute and the rescue go on.
    if let Some(end) = a.ended
        && combat.end.is_none()
        && combat.sim.chute().is_none()
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
        SimEvent::Sound { .. } | SimEvent::Rwr(_) | SimEvent::Log(_) | SimEvent::CancelTimeAccel => {}
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
}

/// Map units -> world units (east, north).
pub fn map_wu(x: i32, y: i32) -> (f64, f64) {
    (x as f64 * 32.0, (0x8000 - y) as f64 * 32.0)
}

/// Interpolated world position (wu east, wu north, viewZ) of a flight unit.
pub fn unit_position(prev: Option<&FlightUnit>, u: &FlightUnit, alpha: f32) -> (f64, f64, f64) {
    let cur = (u.wx as f64, (0x100000 - u.wy) as f64, u.alt as f64);
    let Some(p) = prev.filter(|p| (p.wx - u.wx).abs() < 0x4000 && (p.wy - u.wy).abs() < 0x4000) else { return cur };
    let old = (p.wx as f64, (0x100000 - p.wy) as f64, p.alt as f64);
    let t = alpha as f64;
    (old.0 + (cur.0 - old.0) * t, old.1 + (cur.1 - old.1) * t, old.2 + (cur.2 - old.2) * t)
}

/// Projects a world point (wu east, north, viewZ) seen from the eye point (aircraft frame) into
/// the 320x200 cockpit screen (f = 256, vertical 0.75 f, centre (159, 52)); None behind the eye.
pub fn project_cockpit(a: &Aircraft, pos: &WorldPos, eye: Vec3, east: f64, north: f64, alt: f64) -> Option<(i32, i32)> {
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
pub fn direction_cockpit(a: &Aircraft, pos: &WorldPos, eye: Vec3, east: f64, north: f64, alt: f64) -> Option<Vec3> {
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
    let Outputs { mut player, mut stores, mut wps, mut msgs, mut clock, mut emv, mut warnings, mut cm, mut contacts, mut cam, mut cues, hud, view, accel, settings, mut emitters } = out;
    let eye = settings.aircraft.spec().eye;
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
    };
    if a.damage != damage {
        a.damage = damage;
    }

    // Stores and gun.
    let (bays, cur) = sim.bays();
    let new_bays: Vec<Bay> = bays
        .iter()
        .map(|&(w, count)| {
            let wt = &data.weapons[(w.max(0) as usize).min(data.weapons.len() - 1)];
            Bay {
                name: wt.long_name.clone(),
                count: count.max(0) as u32,
                picture: wt.picture.max(0) as usize,
                air_to_air: wt.spec > 0 && data.missile(wt.spec as i32).class == 7,
                range_km: if wt.spec > 0 { data.missile(wt.spec as i32).lock_range.max(0) as f32 } else { 0.0 },
            }
        })
        .collect();
    let gun = (!combat.lockheed).then(|| sim.gun_ammo().max(0) as u32);
    if stores.selected != cur
        || stores.gun_rounds != gun
        || stores.bays.len() != new_bays.len()
        || stores.bays.iter().zip(&new_bays).any(|(a, b)| a.count != b.count || a.name != b.name)
    {
        stores.bays = new_bays;
        stores.selected = cur;
        stores.gun_rounds = gun;
    }

    // Waypoints: the mission logic switches to the next one when an objective is done.
    if waypoint_changed {
        let w = wps.for_mission_waypoint(o.waypoint.max(0) as usize);
        wps.current = w;
        wps.mission_current = w;
    }
    msgs.training = sim.invulnerable();
    msgs.accel = accel.on;
    clock.elapsed = Some(sim.mission_tick() as f32);

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
    contacts.radar_arc = sim.radar_arc().filter(|a| a.frames_left > 0).map(|a| crate::mfd::MapArc {
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
        let kind = if sim.drones().is_some_and(|d| d.contains(i)) { ContactKind::Drone } else { ContactKind::Aircraft };
        contacts.list.push(Contact { kind, east, north, alt: alt as f32, heading: u.heading as u16, friendly: sim.friendly_aircraft(u.spec) });
    }
    for (_, m, _, _) in sim.active_projectiles() {
        let (east, north) = map_wu(m.x, m.y);
        contacts.list.push(Contact { kind: ContactKind::Missile, east, north, alt: m.alt as f32, heading: m.heading as u16, friendly: false });
    }
    let objects = sim.objects();
    for (i, ob) in objects.iter().enumerate().take(sim.ground_unit_count()).skip(3) {
        if ob.unit_type > 0 && ob.flags & (oflags::DESTROYED | oflags::FRIENDLY | oflags::NEUTRAL) == 0 && i < objects.len() {
            let (east, north) = map_wu(ob.x, ob.y);
            contacts.list.push(Contact { kind: ContactKind::GroundRadar, east, north, alt: 0.0, heading: 0, friendly: false });
        }
    }
    let heading_bam = to_bam(a.euler().0);
    emitters.list = sim
        .emitters()
        .iter()
        .map(|e| {
            let name = match e.source {
                stealth_sim::EmitterSource::Site(_) => sim.threat_name(e.radar_type).to_string(),
                stealth_sim::EmitterSource::Aircraft(i) => sim.aircraft_name(sim.flights()[i].spec),
            };
            crate::rwr::view(e, &name, heading_bam)
        })
        .collect();
    let done = [o.primary_done, o.secondary_done];
    for (k, t) in sim.target_slots().iter().enumerate() {
        if done[k] || !(1..=3).contains(&t.mission_type) {
            continue;
        }
        if let Some(ob) = usize::try_from(t.object).ok().and_then(|i| objects.get(i))
            && ob.flags & oflags::DESTROYED == 0
            && (ob.x, ob.y) != (0, 0)
        {
            let (east, north) = map_wu(ob.x, ob.y);
            contacts.list.push(Contact { kind: ContactKind::Target, east, north, alt: 0.0, heading: 0, friendly: false });
        }
    }

    // Tracking camera data and HUD cues.
    let t = sim.targeting();
    let player_view = sim.player();
    let (h, _, _) = a.euler();
    let heading = to_bam(h) as i32;
    let mut info = TrackCam { live: cam.live, ..default() };
    let mut new_cues = HudCues::default();
    let cockpit = view.view == View::Cockpit;
    let (w, _) = bays.get(cur).copied().unwrap_or((0, 0));
    let spec = data.weapons[(w.max(0) as usize).min(data.weapons.len() - 1)].spec as i32;
    if let Some(g) = t.ground.filter(|&g| g < objects.len()) {
        let ob = objects[g];
        let place = sim.place_name(g);
        let kind = combat.names.get((ob.shape & 0x7F) as usize).cloned().unwrap_or_default();
        info.name = if place.eq_ignore_ascii_case(&kind) || place.is_empty() { [kind, String::new()] } else { [place, kind] };
        info.range_km = Some(t.range as f32 * 32.0 * WU as f32 / 1000.0);
        info.bearing = Some(((t.bearing as u16) as u32 * 360 / 65536) as i32);
        info.zoom = camera_zoom(t.range) + 1;
        let (east, north) = map_wu(ob.x, ob.y);
        if cockpit && hud.mode == HudMode::Gnd && t.box_size > 0 {
            new_cues.target_dir = direction_cockpit(&a, pos, eye, east, north, 0.0);
            new_cues.target_box = on_hud(project_cockpit(&a, pos, eye, east, north, 0.0)).map(|(x, y)| TargetBox {
                x,
                y,
                size: t.box_size,
                colour: t.box_colour,
                oval: t.lock_valid,
                red: t.in_range,
            });
        }
        if cockpit && hud.mode == HudMode::Gnd && (28..=30).contains(&spec) {
            // Flight-path guide: 128 map units beyond the target along the heading, own altitude.
            let hr = bam(heading as f32);
            let (qx, qy) = (ob.x as f64 + hr.sin() as f64 * 128.0, ob.y as f64 - hr.cos() as f64 * 128.0);
            new_cues.diamond_dir = direction_cockpit(&a, pos, eye, qx * 32.0, (0x8000 as f64 - qy) * 32.0, player_view.view_z as f64);
            new_cues.diamond =
                on_hud(project_cockpit(&a, pos, eye, qx * 32.0, (0x8000 as f64 - qy) * 32.0, player_view.view_z as f64));
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
            new_cues.target_box = on_hud(project_cockpit(&a, pos, eye, east, north, alt)).map(|(x, y)| TargetBox {
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
        if let Some((x, y)) = t.ccip {
            let (east, north) = map_wu(x, y);
            new_cues.ccip_dir = direction_cockpit(&a, pos, eye, east, north, 0.0);
            new_cues.ccip = Some(on_hud(project_cockpit(&a, pos, eye, east, north, 0.0)).unwrap_or((0xA0, 0x5A)));
        }
        new_cues.ranging_bar = t.ranging_bar;
        new_cues.lock = t.lock_valid;
        new_cues.in_range = t.in_range;
    }
    *cam = info;
    *cues = new_cues;
}

/// The end-of-mission overlay.
#[derive(Component)]
struct EndOverlay {
    timer: f32,
}

/// Seconds the overlay stays before returning to the menu (any key after 1 s).
const END_SECONDS: f32 = 8.0;

fn end_overlay(
    mut commands: Commands,
    combat: Option<Res<Combat>>,
    real: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay: Query<&mut EndOverlay>,
    mut next: ResMut<NextState<AppState>>,
    mission: Option<Res<ActiveMission>>,
) {
    let Some(end) = combat.as_ref().and_then(|c| c.end.clone()) else { return };
    match overlay.single_mut() {
        Ok(mut o) => {
            o.timer += real.delta_secs();
            if o.timer > END_SECONDS || (o.timer > 1.0 && keys.get_just_pressed().next().is_some()) {
                if let Some(c) = &combat {
                    commands.insert_resource(crate::debrief::Debrief {
                        title: end.title.clone(),
                        report: c.sim.mission_report(),
                        damage: Some(c.sim.world_damage()),
                        mission: mission.as_ref().map(|m| m.0.clone()),
                        career: Vec::new(),
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
