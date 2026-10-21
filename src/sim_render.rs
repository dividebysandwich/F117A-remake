//! Draws the combat simulation's moving objects (the per-frame draw list):
//! aircraft (STFLT models by `aircraftTypes[spec].model`), projectiles (`sams[spec].model`),
//! countermeasure cartridges, gun rounds, the falling wreck and the parachute (explosions and
//! smoke: explosions.rs). Positions are interpolated between the 15 Hz simulation frames.
//!
//! Also drives the model state slots of the world: lights (10/11) from the palette, the turning
//! antennas (15/16) and the per-object aircraft slots (8, 12, 13, 14, 18-20).

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use stealth_sim::{N_PROJ, fflags};

use crate::combat::{Combat, CombatSystems, unit_position};
use crate::flight::{Aircraft, Player, bam};
use crate::models::{ModelAssets, ModelGlobals, ModelLibrary, ModelState};
use crate::palette_material::{
    FLAG_LINE, LEVEL_AIRCRAFT, PaletteMaterial, PaletteParams, mesh_tag,
};
use crate::player::{AIRCRAFT_MODEL_SCALE, OwnLights, OwnModel};
use crate::settings::AppState;
use crate::world::{TileObject, WorldPos, wu_to_bevy};

pub struct SimRenderPlugin;

impl Plugin for SimRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimVisuals>()
            .add_systems(OnExit(AppState::Flight), cleanup)
            .add_systems(
                Update,
                (sync_objects.run_if(resource_exists::<Combat>), world_model_state, own_shadow)
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
/// original data has no Hind model); drawn as the only helicopter model, the UH-60 (25).
const HIND_DATA_MODEL: i32 = 15;
const HELICOPTER_MODEL: i32 = 25;

/// The remake's own aircraft models (the Geran drones) are numbered from here, in
/// `models/drones.glb` (tools/models/drones.py).
const REMAKE_MODELS: i32 = 100;

/// STFLT model drawn for an aircraft type's model number.
fn aircraft_model(model: i32) -> i32 {
    if model == HIND_DATA_MODEL { HELICOPTER_MODEL } else { model }
}

/// Model file (stem) holding a model number.
fn model_file(model: i32) -> &'static str {
    if model >= REMAKE_MODELS { "drones" } else { "stflt" }
}

struct Cartridge {
    entity: Entity,
    key: (i32, i32, i32),
    alt: f32,
}

#[derive(Resource, Default)]
struct SimVisuals {
    aircraft: HashMap<usize, (Entity, (i32, bool))>,
    proj: [Option<(Entity, i32)>; N_PROJ],
    cms: [Option<Cartridge>; 4],
    /// Gun rounds: the dot at the round and its tracer line.
    bullets: Vec<(Entity, Entity)>,
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
        self.materials
            .entry(idx)
            .or_insert_with(|| {
                materials.add(PaletteMaterial {
                    params: PaletteParams {
                        word: 0xFF00 | idx as u32,
                        flags: FLAG_LINE,
                        ..default()
                    },
                    globals: globals.clone(),
                })
            })
            .clone()
    }
}

fn cleanup(mut commands: Commands, q: Query<Entity, With<SimObject>>, mut vis: ResMut<SimVisuals>) {
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
/// simulation frames. None when the slot is free.
pub fn projectile_pose(combat: &Combat, s: usize) -> Option<(bevy::math::DVec3, Quat)> {
    let m = *combat.sim.projectiles().get(s)?;
    if m.ttl == 0 {
        return None;
    }
    let p = combat.prev_proj[s];
    // Ballistic bombs at their exact point, the others at their map unit.
    let (mut x, mut y, mut z) = m.map_pos();
    if p.ttl != 0 && p.spec == m.spec && (p.x - m.x).abs() < 0x400 && (p.y - m.y).abs() < 0x400 {
        let t = combat.alpha as f64;
        let (px, py, pz) = p.map_pos();
        x = px + (x - px) * t;
        y = py + (y - py) * t;
        z = pz + (z - pz) * t;
    }
    Some((map_point(x, y, z), attitude(m.heading, m.pitch, m.roll)))
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

/// A camera-facing dot of constant screen size (a parent carrying the position).
fn spawn_dot(
    commands: &mut Commands,
    assets: &ModelAssets,
    material: Handle<PaletteMaterial>,
    offsets: &[Vec3],
) -> Entity {
    let parent = commands
        .spawn((SimObject, WorldPos::default(), Transform::default(), Visibility::Hidden))
        .id();
    for &o in offsets {
        let dot = commands
            .spawn((
                crate::dots::DotLight::bundle(o, &assets.dots, mesh_tag(LEVEL_AIRCRAFT, 0, true)),
                MeshMaterial3d(material.clone()),
            ))
            .id();
        commands.entity(parent).add_child(dot);
    }
    parent
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
        let friendly = sim.friendly_aircraft(u.spec);
        let key = (model, friendly);
        if vis.aircraft.get(&i).is_some_and(|&(_, k)| k != key) {
            if let Some((e, _)) = vis.aircraft.remove(&i) {
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
            vis.aircraft.insert(i, (e, key));
        }
        seen.insert(i);
        let (e, _) = vis.aircraft[&i];
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
                let nav = sim.aircraft_lights(i);
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
    vis.aircraft.retain(|i, (e, _)| {
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
        if vis.proj[s].is_some_and(|(_, sp)| m.ttl == 0 || sp != m.spec) {
            if let Some((e, _)) = vis.proj[s].take() {
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
            // Rocket motor glow on missiles (not on bombs / equipment), so they can be followed.
            if crate::trails::is_missile(m.spec) {
                let material = vis.material(&mut materials, &globals, TRACER);
                let dot = commands
                    .spawn((
                        crate::dots::DotLight::bundle(
                            Vec3::ZERO,
                            &assets.dots,
                            mesh_tag(LEVEL_AIRCRAFT, 0, true),
                        ),
                        MeshMaterial3d(material),
                        crate::lighting::LightEmitter {
                            color: Vec3::new(1.6, 0.9, 0.4),
                            range: 60.0,
                        },
                    ))
                    .id();
                commands.entity(e).add_child(dot);
            }
            vis.proj[s] = Some((e, m.spec));
        }
        let Some((e, _)) = vis.proj[s] else { continue };
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
        let dot = spawn_dot(&mut commands, &assets, material.clone(), &[Vec3::ZERO]);
        let line = spawn_tracer(&mut commands, material);
        vis.bullets.push((dot, line));
    }
    let fine = stealth_sim::ROUND_FINE as f64;
    for (b, &(dot, line)) in bullets.iter().zip(&vis.bullets) {
        let (x, y, alt) = b.map_pos();
        let step = (b.vx as f64 / (32.0 * fine), b.vy as f64 / (32.0 * fine), b.vz as f64 / fine);
        let back = 1.0 - alpha as f64;
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
                        tf.rotation =
                            Quat::from_rotation_arc(Vec3::Z, streak.normalize_or(Vec3::Z));
                        tf.scale = Vec3::new(1.0, 1.0, streak.length().max(0.01));
                    }
                    v.set_if_neq(Visibility::Inherited);
                } else {
                    v.set_if_neq(Visibility::Hidden);
                }
            }
        }
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
    // Slot 15 += 1152 and slot 16 += 1024 per original frame time unit, ~15 per second.
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
struct OwnShadow;

/// Colour of the original's shadow model (STFLT 17).
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
