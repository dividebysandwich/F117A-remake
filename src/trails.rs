//! Missile smoke trails and contrails. Each trail is a ribbon mesh of
//! segments along the emitter's path, rebuilt every frame; the trail shader
//! (assets/shaders/trail.wgsl) turns every segment to face the view drawing it. Segments widen
//! with age and start fading after `fade_after` seconds; a trail whose emitter is gone stays
//! until its last segment has faded.
//!
//! * Missiles (all projectiles but bombs and dropped equipment) trail smoke.
//! * Aircraft (AI and the player) above `CONTRAIL_VIEW_Z` leave two contrails.
//! * The damaged player aircraft trails smoke: grey, black when badly hit or on fire. It fades
//!   in behind the smoke puffs of [`crate::explosions`] and hangs in the air for a minute.
//! * So do the other pilots' (coop) missiles and damaged aircraft.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::DVec3;
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError, VertexFormat,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use stealth_sim::{N_PROJ, fflags};

use crate::combat::{Combat, CombatSystems, unit_position};
use crate::flight::{Aircraft, Player};
use crate::settings::AppState;
use crate::sim_render::attitude;
use crate::world::{WorldPos, wu_to_bevy};

const SHADER: &str = "shaders/trail.wgsl";

/// Contrails form above this altitude (viewZ, the sim's compressed altitude; below 0x2000 it
/// equals feet). Patrolling fighters fly at the player's viewZ + 1000 (at least 5000), so they
/// leave contrails whenever the player is above 5000; interceptors chasing a high player and
/// aircraft climbing back to base from far away do too.
pub const CONTRAIL_VIEW_Z: i32 = 6000;

/// Trail direction at a point (per vertex).
pub(crate) const ATTRIBUTE_TANGENT: MeshVertexAttribute =
    MeshVertexAttribute::new("TrailTangent", 0x7A11_0001, VertexFormat::Float32x3);
/// Signed half width (metres) and alpha (per vertex).
pub(crate) const ATTRIBUTE_SIDE: MeshVertexAttribute =
    MeshVertexAttribute::new("TrailSide", 0x7A11_0002, VertexFormat::Float32x2);

pub struct TrailsPlugin;

impl Plugin for TrailsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<TrailMaterial>::default())
            .init_resource::<Trails>()
            .add_systems(OnExit(AppState::Flight), cleanup)
            .add_systems(
                Update,
                update_trails
                    .run_if(resource_exists::<Combat>)
                    .after(CombatSystems)
                    .run_if(in_state(AppState::Flight)),
            );
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct TrailMaterial {
    /// x: palette index, y: opacity.
    #[uniform(0)]
    params: Vec4,
    #[storage(1, read_only)]
    globals: Handle<ShaderBuffer>,
}

impl Material for TrailMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            ATTRIBUTE_TANGENT.at_shader_location(1),
            ATTRIBUTE_SIDE.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Look of a kind of trail.
struct Style {
    /// Palette colour and opacity.
    colour: u8,
    opacity: f32,
    /// Half width at birth and when fully grown (metres), and the seconds it takes to grow.
    width: (f32, f32),
    grow: f32,
    /// Seconds before a segment starts fading, and how long the fade takes.
    fade_after: f32,
    fade: f32,
    /// A new segment starts every this many metres.
    spacing: f64,
    /// Alpha at the emitter (contrails form a little behind the engines).
    head_alpha: f32,
    /// Seconds a segment takes to fade in (0: born opaque).
    fade_in: f32,
}

const SMOKE: Style = Style {
    colour: 7,
    opacity: 0.75,
    width: (0.8, 6.0),
    grow: 20.0,
    fade_after: 0.0,
    fade: 20.0,
    spacing: 12.0,
    head_alpha: 1.0,
    fade_in: 0.0,
};

const CONTRAIL: Style = Style {
    colour: 15,
    opacity: 0.6,
    width: (1.0, 14.0),
    grow: 60.0,
    fade_after: 10.0,
    fade: 60.0,
    spacing: 30.0,
    head_alpha: 0.0,
    fade_in: 0.0,
};

/// Smoke of the damaged player aircraft: light and heavy damage (or fire).
const DAMAGE_SMOKE: Style = Style {
    colour: 7,
    opacity: 0.45,
    width: (1.5, 22.0),
    grow: 40.0,
    fade_after: 20.0,
    fade: 40.0,
    spacing: 20.0,
    head_alpha: 0.0,
    fade_in: 4.0,
};

const HEAVY_DAMAGE_SMOKE: Style = Style {
    colour: 8,
    opacity: 0.7,
    width: (2.5, 30.0),
    grow: 40.0,
    fade_after: 30.0,
    fade: 40.0,
    spacing: 20.0,
    head_alpha: 0.0,
    fade_in: 4.0,
};

/// Where the damaged aircraft's smoke and fire come out (aircraft frame, metres: behind the
/// middle, at the engines).
pub const DAMAGE_EMITTER: Vec3 = Vec3::new(0.0, 0.3, 8.0);

/// Contrail emitters: behind the aircraft centre, left and right (aircraft frame, metres).
const ENGINES: [Vec3; 2] = [Vec3::new(-3.0, 0.0, 8.0), Vec3::new(3.0, 0.0, 8.0)];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    /// Projectile slot and spec.
    Missile(usize, i32),
    /// Flight unit index and engine.
    Aircraft(usize, usize),
    /// Player engine.
    Player(usize),
    /// Damaged player aircraft (heavy smoke).
    PlayerSmoke(bool),
    /// Another pilot's missile: (pilot, slot) and spec.
    RemoteMissile((u8, u8), i32),
    /// Another pilot's damaged aircraft (heavy smoke).
    RemoteSmoke(u8, bool),
}

struct Trail {
    entity: Entity,
    mesh: Handle<Mesh>,
    style: &'static Style,
    /// World position of the mesh origin.
    anchor: DVec3,
    /// Committed points: (world position, birth time).
    points: Vec<(DVec3, f32)>,
    /// The emitter's position this frame (None once detached).
    head: Option<DVec3>,
}

impl Trail {
    fn alive(&self, now: f32) -> bool {
        self.head.is_some()
            || self
                .points
                .last()
                .is_some_and(|&(_, t)| now - t < self.style.fade_after + self.style.fade)
    }

    /// Drops faded points from the tail (keeping the last faded one as the ribbon's end).
    fn prune(&mut self, now: f32) {
        let life = self.style.fade_after + self.style.fade;
        let dead = self.points.iter().take_while(|&&(_, t)| now - t >= life).count();
        if dead > 1 {
            self.points.drain(..dead - 1);
        }
    }

    /// At least one segment to draw (an empty mesh is neither uploaded nor drawn).
    fn drawable(&self) -> bool {
        self.points.len() + self.head.is_some() as usize >= 2
    }

    fn rebuild(&self, mesh: &mut Mesh, now: f32) {
        let s = self.style;
        let mut pts: Vec<(DVec3, f32, f32)> = self
            .points
            .iter()
            .map(|&(p, t)| {
                let age = now - t;
                let mut alpha = 1.0 - ((age - s.fade_after) / s.fade).clamp(0.0, 1.0);
                if s.fade_in > 0.0 {
                    alpha *= (age / s.fade_in).clamp(0.0, 1.0);
                }
                let w = s.width.0 + (s.width.1 - s.width.0) * (age / s.grow).clamp(0.0, 1.0).sqrt();
                (p, w, alpha)
            })
            .collect();
        if let Some(h) = self.head {
            pts.push((h, s.width.0, s.head_alpha));
        }
        let n = pts.len();
        let mut pos = Vec::with_capacity(2 * n);
        let mut tan = Vec::with_capacity(2 * n);
        let mut side = Vec::with_capacity(2 * n);
        let mut idx = Vec::with_capacity(6 * n.saturating_sub(1));
        for i in 0..n {
            let (p, w, a) = pts[i];
            let d = (pts[(i + 1).min(n - 1)].0 - pts[i.saturating_sub(1)].0).as_vec3();
            let t = d.try_normalize().unwrap_or(Vec3::Z);
            let local = (p - self.anchor).as_vec3();
            pos.extend([local, local]);
            tan.extend([t, t]);
            side.extend([[-w, a], [w, a]]);
        }
        for i in 0..n as u32 - 1 {
            let k = 2 * i;
            idx.extend([k, k + 1, k + 2, k + 1, k + 3, k + 2]);
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(ATTRIBUTE_TANGENT, tan);
        mesh.insert_attribute(ATTRIBUTE_SIDE, side);
        mesh.insert_indices(Indices::U32(idx));
    }
}

#[derive(Resource, Default)]
struct Trails {
    /// Trails with a live emitter.
    active: HashMap<Source, Trail>,
    /// Trails left behind, fading out.
    detached: Vec<Trail>,
    /// By colour and opacity.
    materials: Vec<((u8, u32), Handle<TrailMaterial>)>,
}

/// Missiles trail smoke; bombs (28..30) and dropped equipment (38) do not.
pub fn is_missile(spec: i32) -> bool {
    !matches!(spec, 28..=30 | 38)
}

fn cleanup(mut commands: Commands, mut trails: ResMut<Trails>) {
    for t in trails.active.values().chain(&trails.detached) {
        commands.entity(t.entity).try_despawn();
    }
    *trails = Trails::default();
}

#[allow(clippy::too_many_arguments)]
fn update_trails(
    mut commands: Commands,
    combat: Res<Combat>,
    mut trails: ResMut<Trails>,
    time: Res<Time>,
    palette: Res<crate::palette::Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TrailMaterial>>,
    player: Query<(&Aircraft, &WorldPos), With<Player>>,
    mut visibility: Query<&mut Visibility>,
) {
    let trails = &mut *trails;
    let sim = &combat.sim;
    let alpha = combat.alpha;
    let now = time.elapsed_secs();

    // Emitter positions this frame.
    let mut emitters: Vec<(Source, &'static Style, DVec3)> = Vec::new();
    let projs = sim.projectiles();
    for s in 0..N_PROJ {
        let m = projs[s];
        // Smoke while the motor burns, from where the missile is drawn.
        if m.ttl == 0 || !is_missile(m.spec) || !m.motor_visible() {
            continue;
        }
        if let Some((pos, _)) = crate::sim_render::projectile_pose(&combat, s) {
            emitters.push((Source::Missile(s, m.spec), &SMOKE, pos));
        }
    }
    for (i, u, _) in sim.active_aircraft() {
        if u.flags & fflags::DESTROYED != 0 || (u.wx == 0 && u.wy == 0) || u.alt < CONTRAIL_VIEW_Z {
            continue;
        }
        let prev = combat.prev_flights.get(i);
        let (east, north, alt) = unit_position(prev, &u, alpha);
        let rot = attitude(u.heading, u.pitch, u.bank);
        for (k, &o) in ENGINES.iter().enumerate() {
            emitters.push((
                Source::Aircraft(i, k),
                &CONTRAIL,
                wu_to_bevy(east, north, alt) + (rot * o).as_dvec3(),
            ));
        }
    }
    if let Ok((a, pos)) = player.single()
        && !a.on_ground
        && sim.player().view_z >= CONTRAIL_VIEW_Z
    {
        for (k, &o) in ENGINES.iter().enumerate() {
            emitters.push((Source::Player(k), &CONTRAIL, pos.0 + (a.orientation * o).as_dvec3()));
        }
    }
    let d = sim.damage_effects();
    if let Ok((a, pos)) = player.single()
        && d.bits != 0
        && a.ended.is_none()
    {
        let heavy = d.bits & stealth_sim::dmg::FIRE != 0 || d.hits >= 6;
        emitters.push((
            Source::PlayerSmoke(heavy),
            if heavy { &HEAVY_DAMAGE_SMOKE } else { &DAMAGE_SMOKE },
            pos.0 + (a.orientation * DAMAGE_EMITTER).as_dvec3(),
        ));
    }

    // The other pilots' missiles and damaged aircraft.
    let me = sim.active_pilot();
    for p in sim.remote_pilots().iter().filter(|p| p.id != me) {
        for &(slot, m) in &p.missiles {
            if is_missile(m.spec) && m.motor_visible() {
                let key = (p.id, slot);
                let (pos, _) = crate::sim_render::remote_missile_pose(&combat, key, &m);
                emitters.push((Source::RemoteMissile(key, m.spec), &SMOKE, pos));
            }
        }
        if p.smoke > 0 && p.chute.is_none() {
            let heavy = p.smoke >= 2;
            let (pos, rot) = crate::sim_render::remote_pose(&combat, p);
            emitters.push((
                Source::RemoteSmoke(p.id, heavy),
                if heavy { &HEAVY_DAMAGE_SMOKE } else { &DAMAGE_SMOKE },
                pos + (rot * DAMAGE_EMITTER).as_dvec3(),
            ));
        }
    }

    // Detach the trails whose emitter is gone.
    let live: Vec<Source> = emitters.iter().map(|e| e.0).collect();
    let gone: Vec<Source> = trails.active.keys().filter(|k| !live.contains(k)).copied().collect();
    for k in gone {
        let mut t = trails.active.remove(&k).unwrap();
        if let Some(h) = t.head.take() {
            t.points.push((h, now));
        }
        trails.detached.push(t);
    }

    for (src, style, p) in emitters {
        if !trails.active.contains_key(&src) {
            let key = (style.colour, style.opacity.to_bits());
            let material = match trails.materials.iter().find(|(k, _)| *k == key) {
                Some((_, m)) => m.clone(),
                None => {
                    let m = materials.add(TrailMaterial {
                        params: Vec4::new(style.colour as f32, style.opacity, 0.0, 0.0),
                        globals: palette.globals.clone(),
                    });
                    trails.materials.push((key, m.clone()));
                    m
                }
            };
            let mesh = meshes
                .add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let entity = commands
                .spawn((
                    crate::sim_render::SimObject,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    WorldPos(p),
                    Transform::default(),
                    // Shown once the ribbon has a segment (empty meshes upset some GPU drivers).
                    Visibility::Hidden,
                    NoFrustumCulling,
                    bevy::light::NotShadowCaster,
                ))
                .id();
            trails.active.insert(
                src,
                Trail { entity, mesh, style, anchor: p, points: vec![(p, now)], head: None },
            );
        }
        let t = trails.active.get_mut(&src).unwrap();
        if t.points.last().is_none_or(|&(q, _)| (p - q).length() >= style.spacing) {
            t.points.push((p, now));
        }
        t.head = Some(p);
    }

    trails.detached.retain(|t| {
        let keep = t.alive(now);
        if !keep {
            commands.entity(t.entity).try_despawn();
        }
        keep
    });
    for t in trails.active.values_mut().chain(trails.detached.iter_mut()) {
        t.prune(now);
        let drawn = t.drawable()
            && meshes.get_mut(&t.mesh).map(|mut mesh| t.rebuild(&mut mesh, now)).is_some();
        if let Ok(mut v) = visibility.get_mut(t.entity) {
            v.set_if_neq(if drawn { Visibility::Inherited } else { Visibility::Hidden });
        }
    }
}
