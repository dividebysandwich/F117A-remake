//! Draws the combat simulation's moving objects (combat.md 1.1: the per-frame draw list):
//! aircraft (STFLT models by `aircraftTypes[spec].model`), projectiles (`sams[spec].model`),
//! countermeasure cartridges, gun rounds, smoke, the falling wreck, the parachute and impact
//! flashes. Positions are interpolated between the 15 Hz simulation frames.
//!
//! Also drives the model state slots of the world (visuals.md section 5): lights (10/11) from
//! the palette, the turning antennas (15/16) and the per-object aircraft slots (8, 12, 13, 14).

use std::collections::{HashMap, HashSet};

use bevy::mesh::MeshTag;
use bevy::prelude::*;
use f117_sim::{N_PROJ, Smoke, fflags};

use crate::combat::{Combat, CombatSystems, unit_position};
use crate::flight::{Aircraft, Player, bam};
use crate::models::{ModelAssets, ModelGlobals, ModelLibrary, ModelState};
use crate::palette_material::{FLAG_LINE, LEVEL_AIRCRAFT, PaletteMaterial, PaletteParams, mesh_tag};
use crate::player::{AIRCRAFT_MODEL_SCALE, OwnModel};
use crate::settings::AppState;
use crate::world::{TileObject, WorldPos, wu_to_bevy};

pub struct SimRenderPlugin;

impl Plugin for SimRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimVisuals>()
            .add_systems(OnEnter(AppState::Flight), init_meshes)
            .add_systems(OnExit(AppState::Flight), cleanup)
            .add_systems(
                Update,
                (sync_objects.run_if(resource_exists::<Combat>), world_model_state, own_shadow)
                    .after(CombatSystems)
                    .run_if(in_state(AppState::Flight)),
            );
    }
}

/// Marks entities spawned for simulation objects.
#[derive(Component)]
pub struct SimObject;

/// Palette colours of the effects.
const FLARE: u8 = 15;
const CHAFF: u8 = 7;
const DECOY: u8 = 13;
const TRACER: u8 = 17;
const ENEMY_TRACER: u8 = 41;
const SMOKE: u8 = 8;
const CHUTE: u8 = 15;
/// STFLT model of the falling wreck (combat.md 1.1).
const WRECK_MODEL: i32 = 11;
/// Seconds without new smoke after which the old puffs are no longer drawn.
const SMOKE_TIMEOUT: f32 = 4.0;

struct Cartridge {
    entity: Entity,
    key: (i32, i32, i32),
    alt: f32,
}

struct Effect {
    entity: Entity,
    age: f32,
    life: f32,
    size: f32,
}

#[derive(Resource, Default)]
struct SimVisuals {
    aircraft: HashMap<usize, (Entity, i32)>,
    proj: [Option<(Entity, i32)>; N_PROJ],
    cms: [Option<Cartridge>; 4],
    bullets: Vec<Entity>,
    smoke: Vec<Entity>,
    smoke_state: [Smoke; 16],
    smoke_age: f32,
    wreck: Option<Entity>,
    chute: Option<Entity>,
    effects: Vec<Effect>,
    /// Solid palette-colour materials (word 0xFF00 | index).
    materials: HashMap<u8, Handle<PaletteMaterial>>,
    sphere: Option<Handle<Mesh>>,
}

impl SimVisuals {
    fn material(&mut self, materials: &mut Assets<PaletteMaterial>, globals: &Handle<bevy::render::storage::ShaderBuffer>, idx: u8) -> Handle<PaletteMaterial> {
        self.materials
            .entry(idx)
            .or_insert_with(|| {
                materials.add(PaletteMaterial {
                    params: PaletteParams { word: 0xFF00 | idx as u32, flags: FLAG_LINE, ..default() },
                    globals: globals.clone(),
                })
            })
            .clone()
    }
}

fn init_meshes(mut vis: ResMut<SimVisuals>, mut meshes: ResMut<Assets<Mesh>>) {
    vis.sphere = Some(meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()));
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

pub fn attitude(heading: i32, pitch: i32, roll: i32) -> Quat {
    Aircraft::from_euler(bam(heading as u16 as f32), bam(pitch as i16 as f32), bam(roll as i16 as f32))
}

fn spawn_model(commands: &mut Commands, assets: &ModelAssets, file: &Handle<bevy::gltf::Gltf>, model: i32) -> Option<Entity> {
    let e = assets.spawn(commands, file, &format!("stflt_{:03}", model.clamp(0, 999)), LEVEL_AIRCRAFT, false)?;
    commands.entity(e).insert((
        SimObject,
        WorldPos::default(),
        Transform::from_scale(Vec3::splat(AIRCRAFT_MODEL_SCALE as f32)),
        Visibility::Inherited,
    ));
    Some(e)
}

/// A camera-facing dot of constant screen size (a parent carrying the position).
fn spawn_dot(commands: &mut Commands, assets: &ModelAssets, material: Handle<PaletteMaterial>, offsets: &[Vec3]) -> Entity {
    let parent = commands.spawn((SimObject, WorldPos::default(), Transform::default(), Visibility::Hidden)).id();
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

fn spawn_sphere(commands: &mut Commands, mesh: Handle<Mesh>, material: Handle<PaletteMaterial>) -> Entity {
    commands
        .spawn((
            SimObject,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            MeshTag(mesh_tag(LEVEL_AIRCRAFT, 0, true)),
            WorldPos::default(),
            Transform::from_scale(Vec3::ZERO),
            Visibility::Hidden,
        ))
        .id()
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
    mut q: Query<(&mut WorldPos, &mut Transform, Option<&mut ModelState>, &mut Visibility), With<SimObject>>,
    mut mats: Query<&mut MeshMaterial3d<PaletteMaterial>, With<SimObject>>,
) {
    let vis = &mut *vis;
    let sim = &combat.sim;
    let alpha = combat.alpha;
    let file = library.load(&server, "stflt");
    let lights = assets.palette.lights_on();
    let globals = assets.palette.globals.clone();
    let Some(sphere) = vis.sphere.clone() else { return };
    let dt = time.delta_secs();

    // --- aircraft
    let mut seen = HashSet::new();
    for (i, u, model) in sim.active_aircraft() {
        if u.flags & fflags::DESTROYED != 0 || (u.wx == 0 && u.wy == 0) {
            continue;
        }
        if vis.aircraft.get(&i).is_some_and(|&(_, m)| m != model) {
            if let Some((e, _)) = vis.aircraft.remove(&i) {
                commands.entity(e).try_despawn();
            }
        }
        if !vis.aircraft.contains_key(&i) {
            let Some(e) = spawn_model(&mut commands, &assets, &file, model) else { continue };
            vis.aircraft.insert(i, (e, model));
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
                // Slots 12 (nav lights), 13 (airborne, gear up) and 14 (STFLT 1/10/15).
                let special = matches!(model, 1 | 10 | 15);
                let v = [
                    (10, lights as i16),
                    (11, lights as i16),
                    (12, (u.flags & 3 != 0) as i16),
                    (13, (!special && u.alt > 12 && u.flags & fflags::DESTROYED == 0) as i16),
                    (14, special as i16),
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
            let Some(e) = spawn_model(&mut commands, &assets, &file, model) else { continue };
            // Rocket motor glow on missiles (not on bombs / equipment), so they can be followed.
            if crate::trails::is_missile(m.spec) {
                let material = vis.material(&mut materials, &globals, TRACER);
                let dot = commands
                    .spawn((
                        crate::dots::DotLight::bundle(Vec3::ZERO, &assets.dots, mesh_tag(LEVEL_AIRCRAFT, 0, true)),
                        MeshMaterial3d(material),
                        crate::lighting::LightEmitter { color: Vec3::new(1.6, 0.9, 0.4), range: 60.0 },
                    ))
                    .id();
                commands.entity(e).add_child(dot);
            }
            vis.proj[s] = Some((e, m.spec));
        }
        let Some((e, _)) = vis.proj[s] else { continue };
        let p = combat.prev_proj[s];
        let (mut x, mut y, mut z) = (m.x as f64, m.y as f64, m.alt as f64);
        if p.ttl != 0 && p.spec == m.spec && (p.x - m.x).abs() < 0x400 && (p.y - m.y).abs() < 0x400 {
            let t = alpha as f64;
            x = p.x as f64 + (x - p.x as f64) * t;
            y = p.y as f64 + (y - p.y as f64) * t;
            z = p.alt as f64 + (z - p.alt as f64) * t;
        }
        if let Ok((mut pos, mut tf, _, _)) = q.get_mut(e) {
            pos.0 = map_point(x + 0.5, y + 0.5, z);
            tf.rotation = attitude(m.heading, m.pitch, m.roll);
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
                2 => (CHAFF, vec![Vec3::ZERO, Vec3::new(6.0, 2.0, 3.0), Vec3::new(-5.0, -3.0, 4.0), Vec3::new(2.0, 4.0, -6.0)]),
                _ => (DECOY, vec![Vec3::ZERO]),
            };
            let material = vis.materials.get(&colour).cloned().unwrap_or_else(|| {
                let h = materials.add(PaletteMaterial {
                    params: PaletteParams { word: 0xFF00 | colour as u32, flags: FLAG_LINE, ..default() },
                    globals: globals.clone(),
                });
                vis.materials.insert(colour, h.clone());
                h
            });
            let entity = spawn_dot(&mut commands, &assets, material, &offsets);
            if c.kind == 1 {
                commands.entity(entity).insert(crate::lighting::LightEmitter { color: Vec3::new(2.0, 1.8, 1.4), range: 150.0 });
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

    // --- gun rounds (tracers)
    let (bullets, n) = sim.bullets();
    while vis.bullets.len() < bullets.len() {
        let colour = if vis.bullets.len() < n { TRACER } else { ENEMY_TRACER };
        let material = vis.material(&mut materials, &globals, colour);
        let e = spawn_dot(&mut commands, &assets, material, &[Vec3::ZERO]);
        vis.bullets.push(e);
    }
    for (b, &e) in bullets.iter().zip(&vis.bullets) {
        if let Ok((mut pos, _, _, mut v)) = q.get_mut(e) {
            if b.x != 0 {
                pos.0 = map_point(b.x as f64 + 0.5, b.y as f64 + 0.5, b.alt as f64);
                v.set_if_neq(Visibility::Inherited);
            } else {
                v.set_if_neq(Visibility::Hidden);
            }
        }
    }

    // --- smoke puffs (grow with age), only while smoke is being produced
    let smoke = *sim.smoke();
    if smoke != vis.smoke_state {
        vis.smoke_state = smoke;
        vis.smoke_age = 0.0;
    } else {
        vis.smoke_age += dt;
    }
    while vis.smoke.len() < smoke.len() {
        let material = vis.material(&mut materials, &globals, SMOKE);
        let e = spawn_sphere(&mut commands, sphere.clone(), material);
        vis.smoke.push(e);
    }
    let fresh = vis.smoke_age < SMOKE_TIMEOUT;
    for (s, &e) in smoke.iter().zip(&vis.smoke) {
        if let Ok((mut pos, mut tf, _, mut v)) = q.get_mut(e) {
            if fresh && s.x != 0 && (0..16).contains(&s.age) {
                pos.0 = map_point(s.x as f64 + 0.5, s.y as f64 + 0.5, s.alt as f64);
                tf.scale = Vec3::splat(8.0 + 5.0 * ((s.age & 0xE) >> 1) as f32);
                v.set_if_neq(Visibility::Inherited);
            } else {
                v.set_if_neq(Visibility::Hidden);
            }
        }
    }

    // --- falling wreck (model 11) until it hits the ground
    let w = sim.wreck();
    if w.alt > 0 && w.x != 0 {
        if vis.wreck.is_none() {
            vis.wreck = spawn_model(&mut commands, &assets, &file, WRECK_MODEL);
        }
        if let Some(e) = vis.wreck
            && let Ok((mut pos, mut tf, _, _)) = q.get_mut(e)
        {
            pos.0 = map_point(w.x as f64 + 0.5, w.y as f64 + 0.5, w.alt as f64);
            tf.rotation = Quat::from_rotation_y(time.elapsed_secs() * 3.0) * Quat::from_rotation_x(-0.6);
        }
    } else if let Some(e) = vis.wreck.take() {
        commands.entity(e).try_despawn();
        let size = 90.0;
        let entity = spawn_sphere(&mut commands, sphere.clone(), vis.material(&mut materials, &globals, 15));
        commands.entity(entity).insert(WorldPos(map_point(w.x as f64, w.y as f64, 0.0)));
        vis.effects.push(Effect { entity, age: 0.0, life: 1.5, size });
    }

    // --- parachute
    match sim.chute() {
        Some((x, y, alt)) => {
            if vis.chute.is_none() {
                let material = vis.material(&mut materials, &globals, CHUTE);
                vis.chute = Some(spawn_dot(&mut commands, &assets, material, &[Vec3::ZERO, Vec3::new(0.0, 3.0, 0.0)]));
            }
            if let Some(e) = vis.chute
                && let Ok((mut pos, _, _, mut v)) = q.get_mut(e)
            {
                pos.0 = map_point(x as f64 + 0.5, y as f64 + 0.5, alt as f64);
                *v = Visibility::Inherited;
            }
        }
        None => {
            if let Some(e) = vis.chute.take() {
                commands.entity(e).try_despawn();
            }
        }
    }

    // --- impact flashes: expanding white -> yellow -> red balls
    for &(x, y, alt, kind) in &combat.explosions {
        let (size, life) = match kind {
            -1 => (6.0, 0.4),
            0x10 => (45.0, 1.2),
            -6 => (70.0, 1.4),
            _ => (90.0, 1.6),
        };
        let entity = spawn_sphere(&mut commands, sphere.clone(), vis.material(&mut materials, &globals, 15));
        commands.entity(entity).insert(WorldPos(map_point(x as f64 + 0.5, y as f64 + 0.5, alt.max(0) as f64)));
        vis.effects.push(Effect { entity, age: 0.0, life, size });
    }
    let (white, yellow, red) = (
        vis.material(&mut materials, &globals, 15),
        vis.material(&mut materials, &globals, 17),
        vis.material(&mut materials, &globals, 41),
    );
    vis.effects.retain_mut(|fx| {
        fx.age += dt;
        if fx.age >= fx.life {
            commands.entity(fx.entity).try_despawn();
            return false;
        }
        let t = fx.age / fx.life;
        if let Ok((_, mut tf, _, mut v)) = q.get_mut(fx.entity) {
            tf.scale = Vec3::splat(fx.size * (0.25 + 0.75 * t.sqrt()));
            v.set_if_neq(Visibility::Inherited);
        }
        // Flash light: white -> orange, fading out.
        let k = (1.0 - t).powi(2) * 3.0;
        commands.entity(fx.entity).insert((
            crate::lighting::LightEmitter { color: Vec3::new(1.0, 0.75 - 0.4 * t, 0.45 - 0.4 * t) * k, range: fx.size * 8.0 },
            bevy::light::NotShadowCaster,
        ));
        if let Ok(mut m) = mats.get_mut(fx.entity) {
            let want = if t < 0.15 { &white } else if t < 0.5 { &yellow } else { &red };
            if m.0 != *want {
                m.0 = want.clone();
            }
        }
        true
    });
}

fn sim_missile_model(combat: &Combat, spec: i32) -> i32 {
    combat.data.missile(spec).model as i32
}

/// Lights (slots 10/11) for the world and the own aircraft, the antenna angles (15/16) and the
/// own aircraft's (STFLT 24) gear (5/6), brakes (7) and bay doors (8).
#[allow(clippy::type_complexity)]
fn world_model_state(
    time: Res<Time>,
    palette: Option<Res<crate::palette::Palette>>,
    combat: Option<Res<Combat>>,
    mut globals: ResMut<ModelGlobals>,
    mut last_lights: Local<Option<bool>>,
    mut angles: Local<[f32; 2]>,
    mut gear: Local<f32>,
    mut tiles: ParamSet<(Query<(&TileObject, &mut ModelState)>, Query<(&TileObject, &mut ModelState), Added<TileObject>>)>,
    mut own: Query<&mut ModelState, (With<OwnModel>, Without<TileObject>)>,
    player: Query<&Aircraft, With<Player>>,
) {
    // Slot 15 += 1152 and slot 16 += 1024 per original frame time unit (DS:4EA0), ~15 per second.
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
    // Gear position DS:C072 counts 0 (up) .. 3 (down), one step per ~0.2 s (key 6).
    let target = if a.gear_down { 3.0 } else { 0.0 };
    let step = time.delta_secs() * 5.0;
    *gear = if *gear < target { (*gear + step).min(target) } else { (*gear - step).max(target) };
    let c072 = gear.floor() as i16;
    let brakes = (a.velocity != 0.0 && a.on_ground && a.brakes) as i16;
    for mut s in &mut own {
        let v = [(5, (c072 != 0) as i16), (6, c072 - 1), (7, brakes), (8, door), (10, lights as i16), (11, lights as i16)];
        if v.iter().any(|&(k, x)| s.0[k] != x) {
            for (k, x) in v {
                s.0[k] = x;
            }
        }
    }
}

/// The own aircraft's shadow (STFLT 17, combat.md 1.1): flat on the ground under the aircraft,
/// by day below 1000 ft.
#[derive(Component)]
struct OwnShadow;

/// STFLT model of the own shadow.
const SHADOW_MODEL: i32 = 17;

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
        if let Some(e) = spawn_model(&mut commands, &assets, &file, SHADOW_MODEL) {
            commands.entity(e).insert((OwnShadow, Visibility::Hidden));
        }
        return;
    };
    // With real shadows the visible own model casts its own (external views).
    let external = matches!(view.view, crate::camera::View::Chase | crate::camera::View::Side | crate::camera::View::Tactical);
    let show = a.altitude < 1000.0 && !assets.palette.lights_on() && !(settings.shadows && external);
    vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    if show {
        let (e, n, _) = crate::world::bevy_to_wu(pos.0);
        // A little above the ground so it does not fight with the ground overlays.
        spos.0 = wu_to_bevy(e, n, a.ground_altitude as f64 + 0.6);
        let (h, _, _) = a.euler();
        tf.rotation = Aircraft::from_euler(h, 0.0, 0.0);
    }
}
