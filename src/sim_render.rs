//! Draws the combat simulation's moving objects (the per-frame draw list):
//! aircraft (STFLT models by `aircraftTypes[spec].model`), projectiles (`sams[spec].model`),
//! countermeasure cartridges, gun rounds, the falling wreck and the parachute (explosions and
//! smoke: explosions.rs). Positions are interpolated between the 15 Hz simulation frames.
//!
//! Also drives the model state slots of the world: lights (10/11) from the palette, the turning
//! antennas (15/16) and the per-object aircraft slots (8, 12, 13, 14, 18-20).

use std::collections::{HashMap, HashSet};

use bevy::math::DVec3;
use bevy::prelude::*;
use stealth_sim::{N_PROJ, fflags};

use crate::combat::{Combat, CombatSystems, unit_position};
use crate::flight::{Aircraft, Player, bam};
use crate::models::{ModelAssets, ModelGlobals, ModelLibrary, ModelState};
use crate::palette_material::{
    FLAG_LINE, LEVEL_AIRCRAFT, PaletteMaterial, PaletteParams, TAG_DOT_FAR, dot_size_tenths,
    mesh_tag, spot_size,
};
use crate::player::{AIRCRAFT_MODEL_SCALE, OwnLights, OwnModel};
use crate::settings::AppState;
use crate::world::{TileObject, WorldPos, wu_to_bevy};

pub struct SimRenderPlugin;

impl Plugin for SimRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimVisuals>()
            .init_resource::<AaaVisuals>()
            .init_resource::<RemoteVisuals>()
            .add_systems(OnExit(AppState::Flight), cleanup)
            .add_systems(
                Update,
                (
                    sync_objects.run_if(resource_exists::<Combat>),
                    sync_aaa.run_if(resource_exists::<Combat>),
                    sync_remote_pilots.run_if(resource_exists::<Combat>),
                    own_muzzle_flash.run_if(resource_exists::<Combat>),
                    world_model_state,
                    own_shadow,
                )
                    .in_set(SimRenderSystems)
                    .after(CombatSystems)
                    .run_if(in_state(AppState::Flight)),
            );
    }
}

/// The systems placing the simulation's objects.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimRenderSystems;

/// Marks entities spawned for simulation objects.
#[derive(Component)]
pub struct SimObject;

/// Palette colours of the effects.
const FLARE: u8 = 15;
const CHAFF: u8 = 7;
const DECOY: u8 = 13;
const TRACER: u8 = 17;
const ENEMY_TRACER: u8 = 41;
/// The other players' rounds: the same ammunition as ours.
const FRIENDLY_TRACER: u8 = TRACER;
/// A coasting missile (motor burnt out): black by day, white at night (in every flight
/// palette), so it stands out against the sky.
const COASTING: u8 = 5;
const COASTING_NIGHT: u8 = 15;
/// The spotting dot of another aircraft far away: dark grey by day, light grey at night (a
/// little fainter than a coasting missile's). `SPOT_OFF`: hidden while its lights are on.
const SPOT: u8 = 8;
const SPOT_NIGHT: u8 = 7;
const SPOT_OFF: u8 = 0;
/// Size in metres of the aircraft a spotting dot stands for: a fighter, a helicopter's rotor,
/// a Geran drone.
const SPOT_METRES: f32 = 12.0;
const SPOT_HELICOPTER_METRES: f32 = 17.0;
const SPOT_DRONE_METRES: f32 = 3.0;
/// A spotting dot and its current look.
type SpotDot = (Entity, u8);
/// A tracer's streak behind the round: this many sim frames of its travel.
const TRACER_FRAMES: f64 = 0.6;
const CHUTE: u8 = 15;
/// The canopy's gores (alternating orange and white) and its suspension lines.
const CANOPY_COLOURS: [u8; 2] = [0xCC, 0xE0];
const CANOPY_LINES: u8 = 0xE6;
const SIGNAL_FLARE: u8 = 41;
/// Seconds a signal flare burns.
const SIGNAL_FLARE_LIFE: f32 = 25.0;
/// STFLT model of the falling wreck.
const WRECK_MODEL: i32 = 11;
/// The Mi-24's aircraft type points at STFLT 15, a missile shape with a motor flame (the
/// data has no Hind model); drawn as the only helicopter model, the UH-60 (25).
const HIND_DATA_MODEL: i32 = 15;
const HELICOPTER_MODEL: i32 = 25;

/// The remake's own aircraft models (the Geran drones) are numbered from here, in
/// `models/drones.glb` (tools/models/drones.py).
const REMAKE_MODELS: i32 = 100;
/// The low-level air defences' models (ZSU-23-4, S-60, MANPADS team) are numbered from here,
/// in `models/aaa.glb` (tools/models/air_defence.py).
const AAA_MODELS: i32 = 110;
/// Their state slots: turret (or crew) azimuth and gun (or launcher) elevation.
const AAA_TURRET_SLOT: usize = 21;
const AAA_GUN_SLOT: usize = 22;
/// A MANPADS team is not drawn further away than this (metres).
const MANPADS_RANGE: f64 = 3000.0;
/// Detail level range multiplier of the guns (seen from further than an aircraft's parts).
const AAA_LOD_RANGE: f32 = 4.0;

/// STFLT model drawn for an aircraft type's model number.
fn aircraft_model(model: i32) -> i32 {
    if model == HIND_DATA_MODEL { HELICOPTER_MODEL } else { model }
}

/// Model file (stem) holding a model number.
fn model_file(model: i32) -> &'static str {
    if model >= AAA_MODELS {
        "aaa"
    } else if model >= REMAKE_MODELS {
        "drones"
    } else {
        "stflt"
    }
}

struct Cartridge {
    entity: Entity,
    key: (i32, i32, i32),
    alt: f32,
}

#[derive(Resource, Default)]
struct SimVisuals {
    /// Aircraft model, its key and its spotting dot with the dot's current look.
    aircraft: HashMap<usize, (Entity, (i32, bool), SpotDot)>,
    /// Projectile model, spec and missile dot with its current look: the motor's glow
    /// (`TRACER`), black by day (`COASTING`) or white at night (`COASTING_NIGHT`).
    proj: [Option<(Entity, i32, Option<(Entity, u8)>)>; N_PROJ],
    cms: [Option<Cartridge>; 4],
    /// Gun rounds: the dot at the round and its tracer line.
    bullets: Vec<(Entity, Entity)>,
    /// The other pilots' rounds (as many as were ever in flight at once).
    other_bullets: Vec<(Entity, Entity)>,
    wreck: Option<Entity>,
    chute: Option<Entity>,
    canopy: Option<Entity>,
    /// Signal flares of the downed pilot: (dot, seconds since fired, map x, map y).
    signal_flares: Vec<(Entity, f32, i32, i32)>,
    /// Solid palette-colour materials (word 0xFF00 | index).
    materials: HashMap<u8, Handle<PaletteMaterial>>,
}

impl SimVisuals {
    fn material(
        &mut self,
        materials: &mut Assets<PaletteMaterial>,
        globals: &Handle<bevy::render::storage::ShaderBuffer>,
        idx: u8,
    ) -> Handle<PaletteMaterial> {
        solid_material(&mut self.materials, materials, globals, idx)
    }
}

/// The solid palette-colour material (word 0xFF00 | index) of `cache`, made on first use.
fn solid_material(
    cache: &mut HashMap<u8, Handle<PaletteMaterial>>,
    materials: &mut Assets<PaletteMaterial>,
    globals: &Handle<bevy::render::storage::ShaderBuffer>,
    idx: u8,
) -> Handle<PaletteMaterial> {
    cache
        .entry(idx)
        .or_insert_with(|| {
            materials.add(PaletteMaterial {
                params: PaletteParams { word: 0xFF00 | idx as u32, flags: FLAG_LINE, ..default() },
                globals: globals.clone(),
            })
        })
        .clone()
}

fn cleanup(
    mut commands: Commands,
    q: Query<Entity, With<SimObject>>,
    mut vis: ResMut<SimVisuals>,
    mut aaa: ResMut<AaaVisuals>,
    mut remote: ResMut<RemoteVisuals>,
) {
    *aaa = AaaVisuals::default();
    *remote = RemoteVisuals::default();
    for e in &q {
        commands.entity(e).try_despawn();
    }
    *vis = SimVisuals::default();
}

/// World position (Bevy metres) of a map point at a viewZ altitude.
pub fn map_point(x: f64, y: f64, alt: f64) -> bevy::math::DVec3 {
    wu_to_bevy(x * 32.0, (0x8000 as f64 - y) * 32.0, alt)
}

/// A projectile's position (Bevy world) and attitude as drawn: interpolated between the last two
/// simulation frames, a classic missile on its smoothed path. None when the slot is free.
pub fn projectile_pose(combat: &Combat, s: usize) -> Option<(DVec3, Quat)> {
    let m = *combat.sim.projectiles().get(s)?;
    if m.ttl == 0 {
        return None;
    }
    if let Some(sm) = combat.proj_smooth[s] {
        let v = sm.vel;
        let rot = if v.length() > 1.0 {
            let heading = v.x.atan2(-v.z);
            let pitch = v.y.atan2(v.x.hypot(v.z));
            Aircraft::from_euler(heading as f32, pitch as f32, 0.0)
        } else {
            attitude(m.heading, m.pitch, 0)
        };
        return Some((sm.pos, rot));
    }
    let p = combat.prev_proj[s];
    let mut at = drawn_point(combat, s, &m);
    if p.ttl != 0 && p.spec == m.spec && (p.x - m.x).abs() < 0x400 && (p.y - m.y).abs() < 0x400 {
        let before = drawn_point(combat, s, &p);
        at = before.lerp(at, combat.alpha as f64);
    }
    Some((at, attitude(m.heading, m.pitch, m.roll)))
}

/// A classic missile as drawn. The simulation moves it in whole map units (32 world units, some
/// 8 of them a frame) and turns it a step at a time, rolling it by twice each correction: its
/// raw path zigzags by several degrees and it rocks from side to side. It is drawn on a
/// smoothed path instead (position and velocity following the simulation's, without lag at a
/// steady velocity), pointing along it with its wings level. Only the picture changes: where
/// the simulation has it, its locks and its fuse stay as they are.
#[derive(Clone, Copy, Debug)]
pub struct Smoothed {
    pos: DVec3,
    vel: DVec3,
}

/// Smoothing time constants (s) of the drawn position and velocity.
const SMOOTH_POS: f64 = 0.12;
const SMOOTH_VEL: f64 = 0.3;
/// Drawn this far (m) from the simulation's point, a missile starts its smoothed path afresh.
const SMOOTH_RESET: f64 = 300.0;
/// Enemy missiles: from this far (m) to this close, their altitude goes from the launcher's
/// frame to the player's (see `drawn_point`).
const TRUE_ALT_FROM: f64 = 8000.0;
const TRUE_ALT_TO: f64 = 1000.0;

/// Missiles under classic guidance are drawn smoothed.
fn smoothed(m: &stealth_sim::Projectile) -> bool {
    m.flight.is_none() && m.fall.is_none() && crate::trails::is_missile(m.spec)
}

/// Where a projectile record is drawn (Bevy world). The simulation flies everything at viewZ,
/// its compressed altitude, and draws the aircraft at that number, but the player flies at the
/// true altitude: above 8192 ft an enemy missile would fly below the player and the hit would
/// show beneath it. Closing on the player, its altitude goes over to the true one.
fn drawn_point(combat: &Combat, s: usize, m: &stealth_sim::Projectile) -> DVec3 {
    let (x, y, mut z) = m.map_pos();
    if s < 8 && m.flight.is_none() && z > 8192.0 {
        let p = combat.sim.player();
        let d = (x - p.mx as f64).hypot(y - p.my as f64) * 32.0 * crate::world::WU;
        let w = ((TRUE_ALT_FROM - d) / (TRUE_ALT_FROM - TRUE_ALT_TO)).clamp(0.0, 1.0);
        let w = w * w * (3.0 - 2.0 * w);
        z += (true_altitude(z) - z) * w;
    }
    map_point(x, y, z)
}

/// The altitude (ft) of a viewZ (`stealth_sim::math::view_z` inverted).
fn true_altitude(z: f64) -> f64 {
    if z < 8192.0 {
        z
    } else if z < 12288.0 {
        8192.0 + (z - 8192.0) * 2.0
    } else {
        16384.0 + (z - 12288.0) * 4.0
    }
}

/// Moves the classic missiles' smoothed paths on by `dt` seconds (after the simulation steps).
pub fn smooth_projectiles(combat: &mut Combat, dt: f32) {
    let dt = dt as f64;
    for s in 0..N_PROJ {
        let m = combat.sim.projectiles()[s];
        if m.ttl == 0 || !smoothed(&m) {
            combat.proj_smooth[s] = None;
            continue;
        }
        let p = combat.prev_proj[s];
        let mut target = drawn_point(combat, s, &m);
        let fresh = p.ttl == 0 || p.spec != m.spec;
        // The simulation's velocity: its last step, else the record's speed along its nose.
        let raw_vel = if fresh {
            attitude(m.heading, m.pitch, 0).as_dquat()
                * DVec3::NEG_Z
                * (m.speed as f64 * 128.0 * crate::world::WU)
        } else {
            let before = drawn_point(combat, s, &p);
            target = before.lerp(target, combat.alpha as f64);
            (drawn_point(combat, s, &m) - before) * crate::combat::FS as f64
        };
        let sm = match combat.proj_smooth[s] {
            Some(sm) if !fresh && sm.pos.distance(target) < SMOOTH_RESET => sm,
            _ => Smoothed { pos: target, vel: raw_vel },
        };
        combat.proj_smooth[s] = Some(sm.step(target, raw_vel, dt));
    }
}

impl Smoothed {
    /// `dt` seconds on: the velocity follows the simulation's, the position moves with it and
    /// is drawn toward the simulation's point.
    fn step(self, target: DVec3, raw_vel: DVec3, dt: f64) -> Self {
        let vel = self.vel + (raw_vel - self.vel) * (1.0 - (-dt / SMOOTH_VEL).exp());
        let pos = self.pos + vel * dt;
        Self { pos: pos + (target - pos) * (1.0 - (-dt / SMOOTH_POS).exp()), vel }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missile flying straight at 10 degrees off a map axis at 1200 m/s, the simulation's
    /// way (whole 15.6 m map units at 15 Hz), drawn at 60 Hz: the raw interpolated path changes
    /// direction by degrees from one simulation frame to the next, the smoothed one by a
    /// fraction of that, staying as close to the true path as the simulation's points.
    #[test]
    fn smoothed_missile_path_is_straight() {
        let unit = 32.0 * crate::world::WU;
        let dir = DVec3::new(10f64.to_radians().sin(), 0.0, -10f64.to_radians().cos());
        let sim_at = |k: i32| {
            let p = dir * 1200.0 * k as f64 / 15.0;
            DVec3::new((p.x / unit).floor() * unit, 0.0, (p.z / unit).floor() * unit)
        };
        let angle = |a: DVec3, b: DVec3| a.angle_between(b).to_degrees();
        let (mut sm, mut raw_prev, mut sm_prev) =
            (Smoothed { pos: DVec3::ZERO, vel: dir * 1200.0 }, None, None);
        let off = |p: DVec3| (p - dir * p.dot(dir)).length();
        let (mut raw_turn, mut sm_turn, mut raw_off, mut sm_off): (f64, f64, f64, f64) =
            (0.0, 0.0, 0.0, 0.0);
        for f in 0..60 * 4 {
            let k = f / 4;
            let alpha = (f % 4) as f64 / 4.0;
            let raw = sim_at(k).lerp(sim_at(k + 1), alpha);
            let raw_vel = (sim_at(k + 1) - sim_at(k)) * 15.0;
            sm = sm.step(raw, raw_vel, 1.0 / 60.0);
            if f > 30 {
                if let Some(v) = raw_prev {
                    raw_turn = raw_turn.max(angle(v, raw_vel));
                }
                if let Some(v) = sm_prev {
                    sm_turn = sm_turn.max(angle(v, sm.vel));
                }
                raw_off = raw_off.max(off(raw));
                sm_off = sm_off.max(off(sm.pos));
            }
            raw_prev = Some(raw_vel);
            sm_prev = Some(sm.vel);
        }
        assert!(raw_turn > 3.0, "the raw path zigzags: {raw_turn}");
        assert!(sm_turn < raw_turn / 5.0, "smoothed {sm_turn} raw {raw_turn}");
        // No farther off the true path than the simulation's own whole map units.
        assert!(sm_off <= raw_off, "off the true path: smoothed {sm_off} m, raw {raw_off} m");
    }
}

pub fn attitude(heading: i32, pitch: i32, roll: i32) -> Quat {
    Aircraft::from_euler(
        bam(heading as u16 as f32),
        bam(pitch as i16 as f32),
        bam(roll as i16 as f32),
    )
}

/// Anti-collision lights (tools/models/anticollision.py), each a flash pattern on a state slot:
/// (slot, period s, flashes as (start s, length s)). White strobes flash once a second
/// (fighters) or twice in quick succession (transports, like airliner strobes); red beacons at
/// about 50 a minute, as their rotating mirrors look from outside.
const ANTICOLLISION: [(usize, f32, &[(f32, f32)]); 3] = [
    (18, 1.0, &[(0.0, 0.07)]),
    (19, 1.2, &[(0.0, 0.06), (0.14, 0.06)]),
    (20, 1.15, &[(0.55, 0.13)]),
];

/// Anti-collision light slots of an aircraft at `t` seconds, all dark unless `on`. `phase`
/// (0..1) puts aircraft out of step with each other.
pub fn anticollision(t: f32, phase: f32, on: bool) -> [(usize, i16); 3] {
    ANTICOLLISION.map(|(slot, period, flashes)| {
        let at = (t / period + phase).fract() * period;
        (slot, (on && flashes.iter().any(|&(start, len)| at >= start && at < start + len)) as i16)
    })
}

/// Friendly aircraft: last detail level range multiplier (32769 model units x 40 = 80 km).
const FRIENDLY_LOD_RANGE: f32 = 40.0;

/// `no_fog`: no distance haze (friendly aircraft stay visible from far away).
fn spawn_model(
    commands: &mut Commands,
    assets: &ModelAssets,
    file: &Handle<bevy::gltf::Gltf>,
    model: i32,
    no_fog: bool,
) -> Option<Entity> {
    let e = assets.spawn(
        commands,
        file,
        &format!("{}_{:03}", model_file(model), model.clamp(0, 999)),
        LEVEL_AIRCRAFT,
        no_fog,
    )?;
    commands.entity(e).insert((
        SimObject,
        WorldPos::default(),
        Transform::from_scale(Vec3::splat(AIRCRAFT_MODEL_SCALE as f32)),
        Visibility::Inherited,
    ));
    Some(e)
}

/// The low-level air defences as drawn: a model per unit, a tracer per Shilka shell.
#[derive(Resource, Default)]
struct AaaVisuals {
    units: Vec<Entity>,
    tracers: Vec<Entity>,
    /// Campaign: the AI's missiles at AI aircraft (a glowing dot each).
    ai_missiles: Vec<Entity>,
    material: Option<Handle<PaletteMaterial>>,
}

/// Bevy world position of a point in the air defences' metres (east, north, up).
fn aaa_point(p: [f64; 3]) -> DVec3 {
    let wu = crate::world::WU;
    wu_to_bevy(p[0] / wu, p[1] / wu, p[2] / 0.3048)
}

/// A 16-bit angle of radians (clockwise).
fn bam16(a: f32) -> i16 {
    (a / std::f32::consts::TAU * 65536.0).round() as i32 as i16
}

/// The guns and MANPADS teams turn towards the player as they aim (each placed at its own
/// heading), show their wreck once destroyed; the Shilkas' shells are yellow tracer streaks like
/// the player's gun rounds, without a dot at the head.
#[allow(clippy::too_many_arguments)]
fn sync_aaa(
    mut commands: Commands,
    combat: Res<Combat>,
    mut av: ResMut<AaaVisuals>,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    assets: ModelAssets,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    player: Query<&WorldPos, (With<Player>, Without<SimObject>)>,
    mut q: Query<
        (&mut WorldPos, &mut Transform, Option<&mut ModelState>, &mut Visibility),
        With<SimObject>,
    >,
) {
    let sim = &combat.sim;
    let units = sim.aaa_units();
    if av.units.len() != units.len() {
        for e in av.units.drain(..) {
            commands.entity(e).try_despawn();
        }
        let file = library.load(&server, "aaa");
        for u in units {
            let model = AAA_MODELS + u.kind as i32;
            let Some(e) = spawn_model(&mut commands, &assets, &file, model, false) else { break };
            commands.entity(e).insert(crate::models::LodRange(AAA_LOD_RANGE));
            av.units.push(e);
        }
        // Not loaded yet: again next frame.
        if av.units.len() != units.len() {
            for e in av.units.drain(..) {
                commands.entity(e).try_despawn();
            }
            return;
        }
    }
    let eye = player.single().map_or(DVec3::ZERO, |p| p.0);
    for (i, (u, &e)) in units.iter().zip(&av.units).enumerate() {
        let Ok((mut pos, mut tf, state, mut vis)) = q.get_mut(e) else { continue };
        let at = map_point(u.x as f64 + 0.5, u.y as f64 + 0.5, 0.0);
        pos.0 = at;
        // Each unit stands at its own heading (golden-angle spread).
        let heading = (i as f32 * 2.399_963).rem_euclid(std::f32::consts::TAU);
        tf.rotation = Quat::from_rotation_y(-heading);
        let hidden =
            u.kind == stealth_sim::aaa::AaaKind::Manpads && at.distance(eye) > MANPADS_RANGE;
        vis.set_if_neq(if hidden { Visibility::Hidden } else { Visibility::Inherited });
        if let Some(mut s) = state {
            s.0[1] = u.destroyed as i16;
            s.0[AAA_TURRET_SLOT] = bam16(u.aim_az - heading);
            // The Patriot launcher fires at a fixed elevation.
            let el = if u.kind == stealth_sim::aaa::AaaKind::PatriotLauncher {
                stealth_sim::campaign::PATRIOT_ELEVATION
            } else {
                u.aim_el
            };
            s.0[AAA_GUN_SLOT] = bam16(-el);
        }
    }

    // Tracers: the Shilkas' shells (an S-60 shell is only seen when it bursts), also those
    // the guns fire at AI aircraft in a campaign.
    let shells: Vec<_> =
        sim.aaa_shells().iter().chain(sim.ai_shells()).filter(|s| !s.flak).collect();
    let material = av
        .material
        .get_or_insert_with(|| {
            let globals = assets.palette.globals.clone();
            materials.add(PaletteMaterial {
                params: PaletteParams {
                    word: 0xFF00 | TRACER as u32,
                    flags: FLAG_LINE,
                    ..default()
                },
                globals,
            })
        })
        .clone();
    while av.tracers.len() < shells.len() {
        let line = spawn_tracer(&mut commands, material.clone());
        av.tracers.push(line);
    }
    let dt = 1.0 / crate::combat::FS as f64;
    let back = (1.0 - combat.alpha as f64) * dt;
    for (k, &line) in av.tracers.iter().enumerate() {
        let Ok((mut pos, mut tf, _, mut vis)) = q.get_mut(line) else { continue };
        let Some(s) = shells.get(k) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let at = |t: f64| aaa_point(std::array::from_fn(|a| s.pos[a] - s.vel[a] * t));
        let head = at(back);
        pos.0 = head;
        let streak = (at(back + TRACER_FRAMES * dt) - head).as_vec3();
        tf.rotation = Quat::from_rotation_arc(Vec3::Z, streak.normalize_or(Vec3::Z));
        tf.scale = Vec3::new(1.0, 1.0, streak.length().max(0.01));
        vis.set_if_neq(Visibility::Inherited);
    }

    // The AI's missiles: the motor's glow.
    let missiles = sim.ai_missiles();
    while av.ai_missiles.len() < missiles.len() {
        let dot = spawn_dot(&mut commands, &assets, material.clone(), &[Vec3::ZERO]);
        av.ai_missiles.push(dot);
    }
    for (k, &dot) in av.ai_missiles.iter().enumerate() {
        let Ok((mut pos, _, _, mut vis)) = q.get_mut(dot) else { continue };
        let Some(m) = missiles.get(k) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        pos.0 = aaa_point(std::array::from_fn(|a| m.pos[a] - m.vel[a] * back));
        vis.set_if_neq(Visibility::Inherited);
    }
}

/// A camera-facing dot of constant screen size (a parent carrying the position).
fn spawn_dot(
    commands: &mut Commands,
    assets: &ModelAssets,
    material: Handle<PaletteMaterial>,
    offsets: &[Vec3],
) -> Entity {
    spawn_dot_scaled(commands, assets, material, offsets, 0)
}

/// [`spawn_dot`] with more mesh tag bits (`dot_scale`, `dot_size_tenths`, `TAG_DOT_FAR`).
fn spawn_dot_scaled(
    commands: &mut Commands,
    assets: &ModelAssets,
    material: Handle<PaletteMaterial>,
    offsets: &[Vec3],
    tag: u32,
) -> Entity {
    let parent = commands
        .spawn((SimObject, WorldPos::default(), Transform::default(), Visibility::Hidden))
        .id();
    for &o in offsets {
        let dot = commands
            .spawn((
                crate::dots::DotLight::bundle(
                    o,
                    &assets.dots,
                    mesh_tag(LEVEL_AIRCRAFT, 0, true) | tag,
                ),
                MeshMaterial3d(material.clone()),
            ))
            .id();
        commands.entity(parent).add_child(dot);
    }
    parent
}

/// A spotting dot on an aircraft model, so it can be seen far away without its lights (the
/// shader drops it once the model is big enough to make out); its look is set by `spot_look`.
fn spawn_spot_dot(
    commands: &mut Commands,
    assets: &ModelAssets,
    aircraft: Entity,
    metres: f32,
) -> SpotDot {
    let tag = mesh_tag(LEVEL_AIRCRAFT, 0, true) | spot_size(metres);
    let dot = commands
        .spawn((crate::dots::DotLight::bundle(Vec3::ZERO, &assets.dots, tag), Visibility::Hidden))
        .id();
    commands.entity(aircraft).add_child(dot);
    (dot, SPOT_OFF)
}

/// Gives a spotting dot its look (`SPOT`, `SPOT_NIGHT` or hidden: `SPOT_OFF`) when it changes.
fn spot_look(
    commands: &mut Commands,
    (dot, cur): &mut SpotDot,
    look: u8,
    material: impl FnOnce(u8) -> Handle<PaletteMaterial>,
) {
    if *cur == look {
        return;
    }
    *cur = look;
    let mut d = commands.entity(*dot);
    if look == SPOT_OFF {
        d.insert(Visibility::Hidden);
    } else {
        d.insert((Visibility::Inherited, MeshMaterial3d(material(look))));
    }
}

/// Puts a round's dot and tracer line where the round is, `gap` sim frames (the last step's)
/// behind (hidden for a free slot).
fn place_tracer(
    b: &stealth_sim::Bullet,
    dot: Entity,
    line: Entity,
    alpha: f32,
    gap: f64,
    q: &mut Query<
        (&mut WorldPos, &mut Transform, Option<&mut ModelState>, &mut Visibility),
        With<SimObject>,
    >,
) {
    let fine = stealth_sim::ROUND_FINE as f64;
    let (x, y, alt) = b.map_pos();
    let step = (b.vx as f64 / (32.0 * fine), b.vy as f64 / (32.0 * fine), b.vz as f64 / fine);
    let back = (1.0 - alpha as f64) * gap;
    let head = map_point(x - step.0 * back, y - step.1 * back, alt - step.2 * back);
    let tail = map_point(
        x - step.0 * (back + TRACER_FRAMES),
        y - step.1 * (back + TRACER_FRAMES),
        alt - step.2 * (back + TRACER_FRAMES),
    );
    let streak = (tail - head).as_vec3();
    for e in [dot, line] {
        if let Ok((mut pos, mut tf, _, mut v)) = q.get_mut(e) {
            if b.x != 0 {
                pos.0 = head;
                if e == line {
                    tf.rotation = Quat::from_rotation_arc(Vec3::Z, streak.normalize_or(Vec3::Z));
                    tf.scale = Vec3::new(1.0, 1.0, streak.length().max(0.01));
                }
                v.set_if_neq(Visibility::Inherited);
            } else {
                v.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// A tracer line: a unit segment along +Z from the round, scaled to the streak.
fn spawn_tracer(commands: &mut Commands, material: Handle<PaletteMaterial>) -> Entity {
    let e = commands
        .spawn((
            SimObject,
            WorldPos::default(),
            Transform::default(),
            Visibility::Hidden,
            MeshMaterial3d(material),
            bevy::mesh::MeshTag(mesh_tag(LEVEL_AIRCRAFT, 0, true)),
            bevy::light::NotShadowCaster,
            bevy::camera::visibility::NoFrustumCulling,
        ))
        .id();
    commands.queue(move |world: &mut World| {
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::LineList,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![Vec3::ZERO, Vec3::Z]);
        let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
        if let Ok(mut ent) = world.get_entity_mut(e) {
            ent.insert(Mesh3d(h));
        }
    });
    e
}

/// The parachute canopy over the pilot (origin at their feet, metres): a round canopy of 24
/// gores in two colours, open underneath, and the suspension lines down to the risers above
/// the pilot's head.
fn spawn_canopy(commands: &mut Commands, assets: &ModelAssets) -> Entity {
    const GORES: usize = 24;
    const RISERS: f32 = 2.3;
    const SKIRT: (f32, f32) = (3.4, 8.6);
    const CROWN: f32 = 2.6;
    // Rings from the skirt up to the apex vent: (radius, height).
    let rings: Vec<(f32, f32)> = [0.0f32, 25.0, 50.0, 70.0, 85.0]
        .iter()
        .map(|d| (SKIRT.0 * d.to_radians().cos(), SKIRT.1 + CROWN * d.to_radians().sin()))
        .collect();
    let point = |(r, h): (f32, f32), k: usize| {
        let a = k as f32 / GORES as f32 * std::f32::consts::TAU;
        Vec3::new(r * a.cos(), h, r * a.sin())
    };
    let mut gores = [Vec::new(), Vec::new()];
    for k in 0..GORES {
        let tris = &mut gores[k % 2];
        for w in rings.windows(2) {
            let (a, b, c, d) =
                (point(w[0], k), point(w[0], k + 1), point(w[1], k + 1), point(w[1], k));
            // Both sides: it is seen from below as much as from outside.
            for q in [[a, c, b, a, d, c], [a, b, c, a, c, d]] {
                tris.extend_from_slice(&q);
            }
        }
    }
    let lines: Vec<Vec3> =
        (0..GORES).flat_map(|k| [Vec3::Y * RISERS, point(rings[0], k)]).collect();
    let root = commands
        .spawn((SimObject, WorldPos::default(), Transform::default(), Visibility::Hidden))
        .id();
    let parts = [
        (
            bevy::mesh::PrimitiveTopology::TriangleList,
            std::mem::take(&mut gores[0]),
            assets.palette.material(CANOPY_COLOURS[0]),
        ),
        (
            bevy::mesh::PrimitiveTopology::TriangleList,
            std::mem::take(&mut gores[1]),
            assets.palette.material(CANOPY_COLOURS[1]),
        ),
        (
            bevy::mesh::PrimitiveTopology::LineList,
            lines,
            assets.palette.line_material(CANOPY_LINES),
        ),
    ];
    for (topology, positions, material) in parts {
        let part = commands
            .spawn((
                Transform::default(),
                MeshMaterial3d(material),
                bevy::mesh::MeshTag(mesh_tag(LEVEL_AIRCRAFT, 0, false)),
                bevy::light::NotShadowCaster,
            ))
            .id();
        commands.entity(root).add_child(part);
        commands.queue(move |world: &mut World| {
            let mut mesh = Mesh::new(topology, bevy::asset::RenderAssetUsages::RENDER_WORLD);
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
            if let Ok(mut e) = world.get_entity_mut(part) {
                e.insert(Mesh3d(h));
            }
        });
    }
    root
}

#[allow(clippy::too_many_arguments)]
fn sync_objects(
    mut commands: Commands,
    combat: Res<Combat>,
    mut vis: ResMut<SimVisuals>,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    assets: ModelAssets,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    time: Res<Time>,
    mut q: Query<
        (&mut WorldPos, &mut Transform, Option<&mut ModelState>, &mut Visibility),
        With<SimObject>,
    >,
    mut explosions: ResMut<crate::explosions::Explosions>,
) {
    let vis = &mut *vis;
    let sim = &combat.sim;
    let alpha = combat.alpha;
    let file = library.load(&server, "stflt");
    let lights = assets.palette.lights_on();
    let globals = assets.palette.globals.clone();
    let dt = time.delta_secs();

    // --- aircraft
    let mut seen = HashSet::new();
    for (i, u, model) in sim.active_aircraft() {
        let model = aircraft_model(model);
        if u.flags & fflags::DESTROYED != 0 || (u.wx == 0 && u.wy == 0) {
            continue;
        }
        let friendly = sim.friendly_unit(i, u.spec);
        let key = (model, friendly);
        if vis.aircraft.get(&i).is_some_and(|&(_, k, _)| k != key) {
            if let Some((e, ..)) = vis.aircraft.remove(&i) {
                commands.entity(e).try_despawn();
            }
        }
        if !vis.aircraft.contains_key(&i) {
            let file = library.load(&server, model_file(model));
            let Some(e) = spawn_model(&mut commands, &assets, &file, model, friendly) else {
                continue;
            };
            if friendly {
                commands.entity(e).insert(crate::models::LodRange(FRIENDLY_LOD_RANGE));
            }
            let metres = if model == HELICOPTER_MODEL {
                SPOT_HELICOPTER_METRES
            } else if model >= REMAKE_MODELS {
                SPOT_DRONE_METRES
            } else {
                SPOT_METRES
            };
            let dot = spawn_spot_dot(&mut commands, &assets, e, metres);
            vis.aircraft.insert(i, (e, key, dot));
        }
        seen.insert(i);
        let nav = sim.aircraft_lights(i);
        let look = if nav {
            SPOT_OFF
        } else if lights {
            SPOT_NIGHT
        } else {
            SPOT
        };
        let Some((e, _, dot)) = vis.aircraft.get_mut(&i) else { continue };
        let e = *e;
        spot_look(&mut commands, dot, look, |c| {
            solid_material(&mut vis.materials, &mut materials, &globals, c)
        });
        let prev = combat.prev_flights.get(i);
        let (east, north, alt) = unit_position(prev, &u, alpha);
        let rot = attitude(u.heading, u.pitch, u.bank);
        let rot = prev.map_or(rot, |p| attitude(p.heading, p.pitch, p.bank).slerp(rot, alpha));
        if let Ok((mut pos, mut tf, state, _)) = q.get_mut(e) {
            pos.0 = wu_to_bevy(east, north, alt);
            tf.rotation = rot;
            if let Some(mut s) = state {
                // Slots 12 (nav lights), 13 (airborne, gear up), 14 (STFLT 1/10/15) and
                // 18-20 (anti-collision lights, with the nav lights near home airfields).
                let special = matches!(model, 1 | 10 | 15);
                let [s18, s19, s20] =
                    anticollision(time.elapsed_secs(), (i as f32 * 0.618).fract(), nav);
                let v = [
                    (10, lights as i16),
                    (11, lights as i16),
                    (12, nav as i16),
                    (13, (!special && u.alt > 12 && u.flags & fflags::DESTROYED == 0) as i16),
                    (14, special as i16),
                    s18,
                    s19,
                    s20,
                ];
                if v.iter().any(|&(k, x)| s.0[k] != x) {
                    for (k, x) in v {
                        s.0[k] = x;
                    }
                }
            }
        }
    }
    vis.aircraft.retain(|i, (e, ..)| {
        let keep = seen.contains(i);
        if !keep {
            commands.entity(*e).try_despawn();
        }
        keep
    });

    // --- projectiles (missiles, bombs, equipment)
    let projs = sim.projectiles();
    for s in 0..N_PROJ {
        let m = projs[s];
        let model = if m.ttl != 0 { sim_missile_model(&combat, m.spec) } else { -1 };
        if vis.proj[s].is_some_and(|(_, sp, _)| m.ttl == 0 || sp != m.spec) {
            if let Some((e, ..)) = vis.proj[s].take() {
                commands.entity(e).try_despawn();
            }
        }
        if m.ttl == 0 {
            continue;
        }
        if vis.proj[s].is_none() {
            let Some(e) = spawn_model(&mut commands, &assets, &file, model, false) else {
                continue;
            };
            // A dot on missiles (not on bombs / equipment), so they can be followed: the
            // rocket motor's glow, then a dark dot once it burns out.
            let mut dot = None;
            if crate::trails::is_missile(m.spec) {
                let d = commands
                    .spawn(crate::dots::DotLight::bundle(
                        Vec3::ZERO,
                        &assets.dots,
                        mesh_tag(LEVEL_AIRCRAFT, 0, true),
                    ))
                    .id();
                commands.entity(e).add_child(d);
                dot = Some((d, 0));
            }
            vis.proj[s] = Some((e, m.spec, dot));
        }
        // The dot's look: glowing while the motor burns, then black by day, white at night.
        let look = if m.motor_visible() {
            TRACER
        } else if lights {
            COASTING_NIGHT
        } else {
            COASTING
        };
        if let Some((_, _, Some((dot, cur)))) = vis.proj[s]
            && cur != look
        {
            let material = vis.material(&mut materials, &globals, look);
            let mut d = commands.entity(dot);
            d.insert(MeshMaterial3d(material));
            if look == TRACER {
                d.insert(crate::lighting::LightEmitter {
                    color: Vec3::new(1.6, 0.9, 0.4),
                    range: 60.0,
                });
            } else {
                d.remove::<crate::lighting::LightEmitter>();
            }
            if let Some((_, _, Some((_, cur)))) = vis.proj[s].as_mut() {
                *cur = look;
            }
        }
        let Some((e, ..)) = vis.proj[s] else { continue };
        if let (Ok((mut pos, mut tf, _, _)), Some((p, rot))) =
            (q.get_mut(e), projectile_pose(&combat, s))
        {
            pos.0 = p;
            tf.rotation = rot;
        }
    }

    // --- countermeasure cartridges: flares and chaff fall slowly from the release point
    // (the simulation keeps them at the player's altitude), the decoy hangs in the air.
    let view_z = sim.player().view_z as f32;
    for (k, c) in sim.countermeasures().iter().enumerate() {
        let key = (c.x, c.y, c.kind);
        if vis.cms[k].as_ref().is_some_and(|v| c.ttl <= 0 || v.key != key) {
            if let Some(v) = vis.cms[k].take() {
                commands.entity(v.entity).try_despawn();
            }
        }
        if c.ttl <= 0 {
            continue;
        }
        let cart = vis.cms[k].get_or_insert_with(|| {
            let (colour, offsets): (u8, Vec<Vec3>) = match c.kind {
                1 => (FLARE, vec![Vec3::ZERO]),
                2 => (
                    CHAFF,
                    vec![
                        Vec3::ZERO,
                        Vec3::new(6.0, 2.0, 3.0),
                        Vec3::new(-5.0, -3.0, 4.0),
                        Vec3::new(2.0, 4.0, -6.0),
                    ],
                ),
                _ => (DECOY, vec![Vec3::ZERO]),
            };
            let material = vis.materials.get(&colour).cloned().unwrap_or_else(|| {
                let h = materials.add(PaletteMaterial {
                    params: PaletteParams {
                        word: 0xFF00 | colour as u32,
                        flags: FLAG_LINE,
                        ..default()
                    },
                    globals: globals.clone(),
                });
                vis.materials.insert(colour, h.clone());
                h
            });
            let entity = spawn_dot(&mut commands, &assets, material, &offsets);
            if c.kind == 1 {
                commands.entity(entity).insert(crate::lighting::LightEmitter {
                    color: Vec3::new(2.0, 1.8, 1.4),
                    range: 150.0,
                });
            }
            Cartridge { entity, key, alt: view_z }
        });
        let fall = match c.kind {
            1 => 60.0,
            2 => 15.0,
            _ => 0.0,
        };
        cart.alt = (cart.alt - fall * dt).max(0.0);
        if let Ok((mut pos, _, _, mut v)) = q.get_mut(cart.entity) {
            pos.0 = map_point(c.x as f64 + 0.5, c.y as f64 + 0.5, cart.alt as f64);
            *v = Visibility::Inherited;
        }
    }

    // --- gun rounds: a bright line along the path of each round (tracers), drawn one sim
    // frame behind like everything else
    let (bullets, n) = sim.bullets();
    if vis.bullets.len() != bullets.len() {
        for &(dot, line) in &vis.bullets {
            commands.entity(dot).try_despawn();
            commands.entity(line).try_despawn();
        }
        vis.bullets.clear();
    }
    while vis.bullets.len() < bullets.len() {
        let colour = if vis.bullets.len() < n { TRACER } else { ENEMY_TRACER };
        let material = vis.material(&mut materials, &globals, colour);
        let dot =
            spawn_dot_scaled(&mut commands, &assets, material.clone(), &[Vec3::ZERO], TAG_DOT_FAR);
        let line = spawn_tracer(&mut commands, material);
        vis.bullets.push((dot, line));
    }
    for (k, (b, &(dot, line))) in bullets.iter().zip(&vis.bullets).enumerate() {
        // The own rounds at his pace, the enemy's with the world.
        let (a, gap) =
            if k < n { (combat.own_alpha, 1.0) } else { (alpha, combat.frame_gap as f64) };
        place_tracer(b, dot, line, a, gap, &mut q);
    }
    // The other pilots' rounds (theirs on the host, flown from their trigger on a client).
    let others = sim.other_rounds();
    while vis.other_bullets.len() < others.len() {
        let material = vis.material(&mut materials, &globals, FRIENDLY_TRACER);
        let dot = spawn_dot_scaled(
            &mut commands,
            &assets,
            material.clone(),
            &[Vec3::ZERO],
            dot_size_tenths(OTHER_GUN_DOT_TENTHS) | TAG_DOT_FAR,
        );
        let line = spawn_tracer(&mut commands, material);
        vis.other_bullets.push((dot, line));
    }
    let none = stealth_sim::Bullet::default();
    // A coop client flies them on by the server's frames, a few at a time.
    let gap = combat.frame_gap as f64;
    for (k, &(dot, line)) in vis.other_bullets.iter().enumerate() {
        place_tracer(others.get(k).unwrap_or(&none), dot, line, alpha, gap, &mut q);
    }

    // --- falling wreck (model 11) until it hits the ground
    let w = sim.wreck();
    if w.alt > 0 && w.x != 0 {
        if vis.wreck.is_none() {
            vis.wreck = spawn_model(&mut commands, &assets, &file, WRECK_MODEL, false);
        }
        if let Some(e) = vis.wreck
            && let Ok((mut pos, mut tf, _, _)) = q.get_mut(e)
        {
            pos.0 = map_point(w.x as f64 + 0.5, w.y as f64 + 0.5, w.alt as f64);
            tf.rotation =
                Quat::from_rotation_y(time.elapsed_secs() * 3.0) * Quat::from_rotation_x(-0.6);
        }
    } else if let Some(e) = vis.wreck.take() {
        commands.entity(e).try_despawn();
        explosions
            .pending
            .push((map_point(w.x as f64, w.y as f64, 0.0), crate::explosions::Blast::Crash));
    }

    // --- parachute: the pilot as a dot, the canopy over them until they land
    match sim.chute() {
        Some((x, y, alt)) => {
            if vis.chute.is_none() {
                let material = vis.material(&mut materials, &globals, CHUTE);
                vis.chute = Some(spawn_dot(&mut commands, &assets, material, &[Vec3::ZERO]));
            }
            if alt > 0 && vis.canopy.is_none() {
                vis.canopy = Some(spawn_canopy(&mut commands, &assets));
            } else if alt <= 0
                && let Some(e) = vis.canopy.take()
            {
                commands.entity(e).try_despawn();
            }
            let at = map_point(x as f64 + 0.5, y as f64 + 0.5, alt as f64);
            for e in [vis.chute, vis.canopy].into_iter().flatten() {
                if let Ok((mut pos, _, _, mut v)) = q.get_mut(e) {
                    pos.0 = at;
                    *v = Visibility::Inherited;
                }
            }
        }
        None => {
            for e in [vis.chute.take(), vis.canopy.take()].into_iter().flatten() {
                commands.entity(e).try_despawn();
            }
        }
    }

    // --- signal flares: a red star climbing to 150 m, then sinking slowly
    for &(x, y) in &combat.signal_flares {
        let material = vis.material(&mut materials, &globals, SIGNAL_FLARE);
        let e = spawn_dot(&mut commands, &assets, material, &[Vec3::ZERO]);
        vis.signal_flares.push((e, 0.0, x, y));
    }
    vis.signal_flares.retain_mut(|(e, age, x, y)| {
        *age += dt;
        if *age > SIGNAL_FLARE_LIFE {
            commands.entity(*e).try_despawn();
            return false;
        }
        // World units: up at 60 per second to 300, then down at 5 per second.
        let alt = if *age < 5.0 { *age * 60.0 } else { 300.0 - (*age - 5.0) * 5.0 };
        if let Ok((mut pos, _, _, mut v)) = q.get_mut(*e) {
            pos.0 = map_point(*x as f64 + 0.5, *y as f64 + 0.5, alt as f64);
            v.set_if_neq(Visibility::Inherited);
        }
        commands.entity(*e).insert(crate::lighting::LightEmitter {
            color: Vec3::new(2.0, 0.2, 0.1),
            range: 400.0,
        });
        true
    });
}

fn sim_missile_model(combat: &Combat, spec: i32) -> i32 {
    combat.data.missile(spec).model as i32
}

/// Lights (slots 10/11) for the world and the own aircraft, the antenna angles (15/16) and the
/// own aircraft's (STFLT 24) gear (5/6), brakes (7), bay doors (8), navigation (12) and
/// anti-collision lights (18-20, the AI aircraft's rule).
#[allow(clippy::type_complexity)]
fn world_model_state(
    time: Res<Time>,
    palette: Option<Res<crate::palette::Palette>>,
    combat: Option<Res<Combat>>,
    mut globals: ResMut<ModelGlobals>,
    mut last_lights: Local<Option<bool>>,
    mut angles: Local<[f32; 2]>,
    mut gear: Local<f32>,
    mut tiles: ParamSet<(
        Query<(&TileObject, &mut ModelState)>,
        Query<(&TileObject, &mut ModelState), Added<TileObject>>,
    )>,
    mut own: Query<&mut ModelState, (Or<(With<OwnModel>, With<OwnLights>)>, Without<TileObject>)>,
    player: Query<&Aircraft, With<Player>>,
) {
    // Slot 15 += 1152 and slot 16 += 1024 per sim frame time unit, ~15 per second.
    let dt = time.delta_secs() * crate::combat::FS as f32;
    angles[0] = (angles[0] + 1152.0 * dt).rem_euclid(65536.0);
    angles[1] = (angles[1] + 1024.0 * dt).rem_euclid(65536.0);
    let a16 = angles[1] as u16 as i16;
    globals.angles = [angles[0] as u16 as i16, a16, a16.wrapping_mul(4)];

    let Some(palette) = palette else { return };
    let lights = palette.lights_on();
    let all = *last_lights != Some(lights);
    *last_lights = Some(lights);
    let set = |t: &TileObject, s: &mut Mut<ModelState>| {
        let v = (lights && !t.destroyed) as i16;
        if s.0[10] != v || s.0[11] != v {
            s.0[10] = v;
            s.0[11] = v;
        }
    };
    if all {
        for (t, mut s) in tiles.p0().iter_mut() {
            set(t, &mut s);
        }
    } else {
        for (t, mut s) in tiles.p1().iter_mut() {
            set(t, &mut s);
        }
    }
    let door = combat.as_ref().map_or(0, |c| c.sim.bay_door_anim()) as i16;
    let Ok(a) = player.single() else { return };
    // Gear position counts 0 (up) .. 3 (down), one step per ~0.2 s (key 6).
    let target = if a.gear_down { 3.0 } else { 0.0 };
    let step = time.delta_secs() * 5.0;
    *gear = if *gear < target { (*gear + step).min(target) } else { (*gear - step).max(target) };
    let c072 = gear.floor() as i16;
    let brakes = (a.velocity != 0.0 && a.on_ground && a.brakes) as i16;
    let nav = combat.as_ref().is_some_and(|c| c.sim.player_lights());
    let [s18, s19, s20] = anticollision(time.elapsed_secs(), 0.0, nav);
    for mut s in &mut own {
        let v = [
            (5, (c072 != 0) as i16),
            (6, c072 - 1),
            (7, brakes),
            (8, door),
            (10, lights as i16),
            (11, lights as i16),
            (12, nav as i16),
            s18,
            s19,
            s20,
        ];
        if v.iter().any(|&(k, x)| s.0[k] != x) {
            for (k, x) in v {
                s.0[k] = x;
            }
        }
    }
}

/// The own aircraft's shadow (STFLT 17): flat on the ground under the aircraft,
/// by day below 1000 ft.
#[derive(Component)]
pub struct OwnShadow;

/// Colour of the shadow model (STFLT 17).
const SHADOW_COLOUR: u8 = 239;

#[allow(clippy::type_complexity)]
fn own_shadow(
    mut commands: Commands,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    assets: ModelAssets,
    player: Query<(&Aircraft, &WorldPos), (With<Player>, Without<OwnShadow>)>,
    mut shadow: Query<(&mut WorldPos, &mut Transform, &mut Visibility), With<OwnShadow>>,
    settings: Res<crate::settings::Settings>,
    view: Res<crate::camera::ViewState>,
) {
    let Ok((a, pos)) = player.single() else { return };
    let Ok((mut spos, mut tf, mut vis)) = shadow.single_mut() else {
        let file = library.load(&server, "stflt");
        let spec = settings.aircraft.spec();
        match spec.shadow {
            crate::aircraft::Shadow::Model(m) => {
                if let Some(e) = spawn_model(&mut commands, &assets, &file, m as i32, false) {
                    commands.entity(e).insert((OwnShadow, Visibility::Hidden));
                }
            }
            crate::aircraft::Shadow::Flattened => {
                // The exterior model's outline in the shadow model's colour.
                let Some(mesh) = assets.flattened(&file, &format!("{}_lod0", spec.model)) else {
                    return;
                };
                let material = assets.palette.material(SHADOW_COLOUR);
                let e = commands
                    .spawn((
                        OwnShadow,
                        SimObject,
                        WorldPos::default(),
                        Transform::from_scale(Vec3::splat(AIRCRAFT_MODEL_SCALE as f32)),
                        Visibility::Hidden,
                        MeshMaterial3d(material),
                        bevy::mesh::MeshTag(mesh_tag(LEVEL_AIRCRAFT, 0, false)),
                        bevy::light::NotShadowCaster,
                    ))
                    .id();
                commands.queue(move |world: &mut World| {
                    let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
                    if let Ok(mut e) = world.get_entity_mut(e) {
                        e.insert(Mesh3d(h));
                    }
                });
            }
        }
        return;
    };
    // With real shadows the visible own model casts its own (external views).
    let external = matches!(
        view.view,
        crate::camera::View::Chase
            | crate::camera::View::Side
            | crate::camera::View::Tactical
            | crate::camera::View::Weapon
    );
    let show =
        a.altitude < 1000.0 && !assets.palette.lights_on() && !(settings.shadows && external);
    vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    if show {
        let (e, n, _) = crate::world::bevy_to_wu(pos.0);
        // A little above the ground so it does not fight with the ground overlays.
        spos.0 = wu_to_bevy(e, n, a.ground_altitude as f64 + 0.6);
        let (h, _, _) = a.euler();
        tf.rotation = Aircraft::from_euler(h, 0.0, 0.0);
    }
}

/// The other pilots' aircraft (coop) as drawn: per pilot the entity, its model, its spotting
/// dot and its muzzle flash; their weapons in flight by (pilot, slot): the model, its spec and
/// its dot with the dot's look.
#[derive(Resource, Default)]
pub struct RemoteVisuals {
    planes: HashMap<u8, (Entity, &'static str, SpotDot, Entity)>,
    missiles: HashMap<(u8, u8), RemoteMissileVisual>,
    /// The own cannon's muzzle flash, as the others see it (external views only).
    own_muzzle: Option<Entity>,
    /// Solid palette-colour materials (word 0xFF00 | index).
    materials: HashMap<u8, Handle<PaletteMaterial>>,
}

/// Another pilot's weapon in flight: its model, its spec, and its dot with the dot's look.
type RemoteMissileVisual = (Entity, i32, Option<(Entity, u8)>);

/// The muzzle flash of another pilot's cannon: a glowing dot at the nose (metres ahead of the
/// aircraft's middle), seen from far away.
const MUZZLE_AHEAD: f32 = 9.0;
/// The other pilots' gun rounds and muzzle flashes are drawn this much bigger than the own
/// rounds (in tenths).
const OTHER_GUN_DOT_TENTHS: u32 = 14;

/// The muzzle flash flickers while the cannon fires: shown in this frame.
fn muzzle_flash_on(time: &Time, id: u8) -> bool {
    !((time.elapsed_secs() * 25.0) as u32 + id as u32).is_multiple_of(3)
}

/// The own cannon's muzzle flash in the external views, as the other pilots see it.
#[allow(clippy::too_many_arguments)]
fn own_muzzle_flash(
    mut commands: Commands,
    combat: Res<Combat>,
    mut vis: ResMut<RemoteVisuals>,
    assets: ModelAssets,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    time: Res<Time>,
    view: Res<crate::camera::ViewState>,
    settings: Res<crate::settings::Settings>,
    player: Query<(&Aircraft, &WorldPos), (With<Player>, Without<SimObject>)>,
    mut q: Query<(&mut WorldPos, &mut Visibility), With<SimObject>>,
) {
    let vis = &mut *vis;
    let sim = &combat.sim;
    let Ok((a, pos)) = player.single() else { return };
    let e = *vis.own_muzzle.get_or_insert_with(|| {
        let globals = assets.palette.globals.clone();
        let material = solid_material(&mut vis.materials, &mut materials, &globals, TRACER);
        let e = spawn_dot_scaled(
            &mut commands,
            &assets,
            material,
            &[Vec3::ZERO],
            dot_size_tenths(OTHER_GUN_DOT_TENTHS),
        );
        commands
            .entity(e)
            .insert(crate::lighting::LightEmitter { color: Vec3::new(2.0, 1.5, 0.8), range: 80.0 });
        e
    });
    let firing = sim.gun_firing() && sim.has_gun() && !view.first_person();
    let flash = firing && muzzle_flash_on(&time, sim.active_pilot());
    if let Ok((mut wp, mut v)) = q.get_mut(e) {
        // At the nose of the model as drawn (lifted off the flight model's point).
        let nose = Vec3::new(0.0, settings.aircraft.spec().model_lift, -MUZZLE_AHEAD);
        wp.0 = pos.0 + (a.orientation * nose).as_dvec3();
        v.set_if_neq(if flash { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Another pilot's aircraft as drawn: its middle and attitude between the samples around the
/// time shown (carried on a little past the last).
pub fn remote_pose(combat: &Combat, p: &stealth_sim::net::RemotePilot) -> (DVec3, Quat) {
    let (old, cur, t) = combat.remote_track.at(p.id, combat.remote_time, p.view);
    let lerp = |a: i32, b: i32| a as f64 + (b - a) as f64 * t;
    let pos = wu_to_bevy(lerp(old.wx, cur.wx), lerp(old.wy, cur.wy), lerp(old.alt, cur.alt));
    let rot = attitude(old.heading, old.pitch, old.roll)
        .slerp(attitude(cur.heading, cur.pitch, cur.roll), t.min(1.0) as f32);
    (pos, rot)
}

/// One of the other pilots' weapons in flight as drawn: between its last two frames.
pub fn remote_missile_pose(
    combat: &Combat,
    key: (u8, u8),
    m: &stealth_sim::Projectile,
) -> (DVec3, Quat) {
    let (x, y, z) = m.map_pos();
    let mut at = map_point(x, y, z);
    if let Some((_, p)) = combat.prev_remote.iter().find(|(k, _)| *k == key)
        && p.spec == m.spec
        && (p.x - m.x).abs() < 0x400
        && (p.y - m.y).abs() < 0x400
    {
        let (px, py, pz) = p.map_pos();
        at = map_point(px, py, pz).lerp(at, combat.alpha as f64);
    }
    (at, attitude(m.heading, m.pitch, m.roll))
}

/// The other pilots' aircraft, interpolated between the simulation's frames like the AI's,
/// their cannon's muzzle flash, and their missiles and bombs.
#[allow(clippy::too_many_arguments)]
fn sync_remote_pilots(
    mut commands: Commands,
    combat: Res<Combat>,
    mut vis: ResMut<RemoteVisuals>,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    assets: ModelAssets,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    time: Res<Time>,
    mut q: Query<(&mut WorldPos, &mut Transform, Option<&mut ModelState>), With<SimObject>>,
    mut muzzles: Query<&mut Visibility, Without<SimObject>>,
) {
    let vis = &mut *vis;
    let sim = &combat.sim;
    let lights = assets.palette.lights_on();
    let globals = assets.palette.globals.clone();
    let me = sim.active_pilot();
    let others: Vec<_> = sim.remote_pilots().into_iter().filter(|p| p.id != me).collect();
    let file = library.load(&server, "stflt");
    vis.planes.retain(|id, (e, ..)| {
        let keep = others.iter().any(|p| p.id == *id && p.chute.is_none());
        if !keep {
            commands.entity(*e).try_despawn();
        }
        keep
    });
    for p in &others {
        if p.chute.is_some() {
            continue;
        }
        let kind = if p.external_stores {
            crate::aircraft::AircraftKind::F16
        } else {
            crate::aircraft::AircraftKind::F117
        };
        let spec = kind.spec();
        if vis.planes.get(&p.id).is_some_and(|v| v.1 != spec.model) {
            if let Some((e, ..)) = vis.planes.remove(&p.id) {
                commands.entity(e).try_despawn();
            }
        }
        if !vis.planes.contains_key(&p.id) {
            let Some(e) = assets.spawn(&mut commands, &file, spec.model, LEVEL_AIRCRAFT, true)
            else {
                continue;
            };
            commands.entity(e).insert((
                SimObject,
                WorldPos::default(),
                Transform::from_scale(Vec3::splat(AIRCRAFT_MODEL_SCALE as f32)),
                Visibility::Inherited,
                crate::models::LodRange(FRIENDLY_LOD_RANGE),
            ));
            let dot = spawn_spot_dot(&mut commands, &assets, e, SPOT_METRES);
            let tag = mesh_tag(LEVEL_AIRCRAFT, 0, true) | dot_size_tenths(OTHER_GUN_DOT_TENTHS);
            let nose = Vec3::NEG_Z * MUZZLE_AHEAD / AIRCRAFT_MODEL_SCALE as f32;
            let material = solid_material(&mut vis.materials, &mut materials, &globals, TRACER);
            let muzzle = commands
                .spawn((
                    crate::dots::DotLight::bundle(nose, &assets.dots, tag),
                    MeshMaterial3d(material),
                    Visibility::Hidden,
                    crate::lighting::LightEmitter { color: Vec3::new(2.0, 1.5, 0.8), range: 80.0 },
                ))
                .id();
            commands.entity(e).add_child(muzzle);
            vis.planes.insert(p.id, (e, spec.model, dot, muzzle));
        }
        let look = if lights { SPOT_NIGHT } else { SPOT };
        let Some(v) = vis.planes.get_mut(&p.id) else { continue };
        spot_look(&mut commands, &mut v.2, look, |c| {
            solid_material(&mut vis.materials, &mut materials, &globals, c)
        });
        // The muzzle flash flickers while the cannon fires.
        let firing = p.gun_firing && !p.lockheed;
        let flash = firing && muzzle_flash_on(&time, p.id);
        if let Ok(mut m) = muzzles.get_mut(v.3) {
            m.set_if_neq(if flash { Visibility::Inherited } else { Visibility::Hidden });
        }
        let (pos, rot) = remote_pose(&combat, p);
        if let Ok((mut wp, mut tf, state)) = q.get_mut(v.0) {
            wp.0 = pos + (rot * Vec3::Y * spec.model_lift).as_dvec3();
            tf.rotation = rot;
            if let Some(mut s) = state {
                // As the own model: the gear (slots 5, 6: down 3 .. up 0), the bay doors, the
                // lights.
                let gear = if p.gear_up && !p.on_ground { 0 } else { 3 };
                let v = [
                    (5, (gear != 0) as i16),
                    (6, gear - 1),
                    (8, p.door_anim.min(2) as i16),
                    (10, lights as i16),
                    (11, lights as i16),
                ];
                if v.iter().any(|&(k, x)| s.0[k] != x) {
                    for (k, x) in v {
                        s.0[k] = x;
                    }
                }
            }
        }
    }
    // Their weapons in flight, with a dot on the missiles like the own: the motor's glow, then
    // black by day, white at night.
    let missiles: Vec<((u8, u8), stealth_sim::Projectile)> =
        others.iter().flat_map(|p| p.missiles.iter().map(move |&(s, m)| ((p.id, s), m))).collect();
    vis.missiles.retain(|key, (e, spec, _)| {
        let keep = missiles.iter().any(|(k, m)| k == key && m.spec == *spec);
        if !keep {
            commands.entity(*e).try_despawn();
        }
        keep
    });
    for (key, m) in &missiles {
        if !vis.missiles.contains_key(key) {
            let model = sim_missile_model(&combat, m.spec);
            let file = library.load(&server, model_file(model));
            let Some(e) = spawn_model(&mut commands, &assets, &file, model, true) else { break };
            let mut dot = None;
            if crate::trails::is_missile(m.spec) {
                let d = commands
                    .spawn(crate::dots::DotLight::bundle(
                        Vec3::ZERO,
                        &assets.dots,
                        mesh_tag(LEVEL_AIRCRAFT, 0, true),
                    ))
                    .id();
                commands.entity(e).add_child(d);
                dot = Some((d, 0));
            }
            vis.missiles.insert(*key, (e, m.spec, dot));
        }
        let look = if m.motor_visible() {
            TRACER
        } else if lights {
            COASTING_NIGHT
        } else {
            COASTING
        };
        let Some((e, _, dot)) = vis.missiles.get_mut(key) else { continue };
        if let Some((d, cur)) = dot
            && *cur != look
        {
            *cur = look;
            let material = solid_material(&mut vis.materials, &mut materials, &globals, look);
            let mut d = commands.entity(*d);
            d.insert(MeshMaterial3d(material));
            if look == TRACER {
                d.insert(crate::lighting::LightEmitter {
                    color: Vec3::new(1.6, 0.9, 0.4),
                    range: 60.0,
                });
            } else {
                d.remove::<crate::lighting::LightEmitter>();
            }
        }
        if let Ok((mut wp, mut tf, _)) = q.get_mut(*e) {
            let (pos, rot) = remote_missile_pose(&combat, *key, m);
            wp.0 = pos;
            tf.rotation = rot;
        }
    }
}
