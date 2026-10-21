//! Explosions (replacing the original's expanding balls): a mix of translucent puffs (fireball,
//! flash, smoke, dust) drawn as one camera-facing particle batch (assets/shaders/particle.wgsl),
//! and small solid meshes (sparks streaking along their flight, tumbling debris) drawn with the
//! palette material. Each impact kind of the simulation has its own recipe.
//!
//! Also turns the simulation's smoke slots (burning aircraft, the last ground kill) into rising
//! smoke puffs.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::DVec3;
use bevy::mesh::{
    Indices, MeshTag, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology,
};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError, VertexFormat,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use stealth_sim::Smoke;

use crate::combat::{Combat, CombatSystems};
use crate::palette_material::{
    FLAG_LINE, LEVEL_AIRCRAFT, PaletteMaterial, PaletteParams, mesh_tag,
};
use crate::settings::AppState;
use crate::sim_render::{SimObject, map_point};
use crate::world::{FloatingOrigin, WorldPos};

const SHADER: &str = "shaders/particle.wgsl";

/// Corner (xy), radius and rotation (per vertex).
const ATTRIBUTE_CORNER: MeshVertexAttribute =
    MeshVertexAttribute::new("ParticleCorner", 0x7A12_0001, VertexFormat::Float32x4);
/// Linear colour and alpha (per vertex).
const ATTRIBUTE_COLOUR: MeshVertexAttribute =
    MeshVertexAttribute::new("ParticleColour", 0x7A12_0002, VertexFormat::Float32x4);
/// Glow and noise seed (per vertex).
const ATTRIBUTE_LOOK: MeshVertexAttribute =
    MeshVertexAttribute::new("ParticleLook", 0x7A12_0003, VertexFormat::Float32x2);

/// Most translucent puffs alive at once (the oldest smoke goes first).
const MAX_PUFFS: usize = 1000;
/// Most solid particles alive at once.
const MAX_CHUNKS: usize = 260;
/// Gravity (m/s²).
const GRAVITY: f32 = 9.81;
/// Seconds without new sim smoke after which its emitters stop.
const SMOKE_TIMEOUT: f32 = 4.0;

pub struct ExplosionsPlugin;

impl Plugin for ExplosionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<ParticleMaterial>::default())
            .init_resource::<Explosions>()
            .init_resource::<Particles>()
            .add_systems(OnExit(AppState::Flight), cleanup)
            .add_systems(
                Update,
                (
                    spawn_blasts.run_if(resource_exists::<Combat>),
                    damage_trail.run_if(resource_exists::<Combat>),
                    simulate,
                )
                    .chain()
                    .after(CombatSystems)
                    .after(crate::sim_render::SimRenderSystems)
                    .run_if(in_state(AppState::Flight)),
            );
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct ParticleMaterial {
    #[uniform(0)]
    unused: Vec4,
    #[storage(1, read_only)]
    globals: Handle<ShaderBuffer>,
}

impl Material for ParticleMaterial {
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
            ATTRIBUTE_CORNER.at_shader_location(1),
            ATTRIBUTE_COLOUR.at_shader_location(2),
            ATTRIBUTE_LOOK.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// What a blast looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blast {
    /// Gun round hitting the player or the ground: sparks and a dust puff.
    GunHit,
    /// Missile or gun kill of an aircraft or ground target.
    Kill,
    /// Bomb or drone impact.
    Bomb,
    /// Aircraft crash, the wreck hitting the ground.
    Crash,
}

impl Blast {
    /// The simulation's explosion kind (`SimEvent::Explosion`).
    fn from_kind(kind: i32) -> Self {
        match kind {
            -1 => Blast::GunHit,
            0x10 => Blast::Kill,
            -6 => Blast::Bomb,
            _ => Blast::Crash,
        }
    }
}

/// Blasts requested by other modules this frame (the falling wreck's impact).
#[derive(Resource, Default)]
pub struct Explosions {
    pub pending: Vec<(DVec3, Blast)>,
}

#[derive(Clone, Copy, PartialEq)]
enum PuffLook {
    /// Fireball: white -> yellow -> orange -> dark red, glowing.
    Fire,
    /// Short bright flash.
    Flash,
    /// Gun round impact: a short bright yellow flash.
    GunFlash,
    /// Smoke of the given darkness (0 black .. 1 light grey).
    Smoke(f32),
    /// Smoke trailing from a damaged aircraft: like `Smoke`, without the fire's glow.
    Trail(f32),
    /// Brown dust thrown up by ground impacts.
    Dust,
}

struct Puff {
    pos: DVec3,
    vel: Vec3,
    age: f32,
    life: f32,
    /// Radius at birth and at the end.
    size: (f32, f32),
    look: PuffLook,
    rot: f32,
    spin: f32,
    seed: f32,
    /// Velocity damping per second and upward acceleration (m/s²).
    drag: f32,
    lift: f32,
    /// Peak opacity.
    opacity: f32,
}

impl Puff {
    fn colour(&self, t: f32) -> (Vec3, f32, f32) {
        // Fade in quickly, out over the last part of the life.
        let fade_in = (self.age / 0.06).min(1.0);
        match self.look {
            PuffLook::Fire => {
                let stops = [
                    (0.0, Vec3::new(4.0, 3.6, 2.6)),
                    (0.15, Vec3::new(3.2, 2.0, 0.5)),
                    (0.4, Vec3::new(1.8, 0.6, 0.08)),
                    (0.7, Vec3::new(0.5, 0.12, 0.03)),
                    (1.0, Vec3::new(0.08, 0.06, 0.05)),
                ];
                let c = gradient(&stops, t);
                let a = self.opacity * fade_in * (1.0 - smooth(0.55, 1.0, t));
                (c, a, 1.0 - smooth(0.5, 0.95, t))
            }
            PuffLook::Flash => (Vec3::new(6.0, 5.5, 4.5), self.opacity * (1.0 - t).powi(2), 1.0),
            PuffLook::GunFlash => (Vec3::new(2.6, 2.1, 0.2), self.opacity * (1.0 - t * t), 1.0),
            PuffLook::Smoke(shade) => {
                let g = 0.04 + 0.32 * shade;
                // Young smoke is lit from inside by the fire.
                let warm = (1.0 - t * 8.0).max(0.0);
                let c = Vec3::splat(g) + Vec3::new(0.6, 0.25, 0.05) * warm;
                (c, self.opacity * fade_in * (1.0 - smooth(0.35, 1.0, t)), warm * 0.6)
            }
            PuffLook::Trail(shade) => (
                Vec3::splat(0.04 + 0.32 * shade),
                self.opacity * fade_in * (1.0 - smooth(0.3, 1.0, t)),
                0.0,
            ),
            PuffLook::Dust => (
                Vec3::new(0.30, 0.24, 0.17),
                self.opacity * fade_in * (1.0 - smooth(0.3, 1.0, t)),
                0.0,
            ),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ChunkKind {
    Spark,
    /// A gun round's spark: yellow, cooling to red.
    GunSpark,
    Debris,
}

struct Chunk {
    entity: Entity,
    kind: ChunkKind,
    pos: DVec3,
    vel: Vec3,
    rot: Quat,
    spin: Vec3,
    age: f32,
    life: f32,
    size: f32,
    /// Seconds until the next smoke puff (burning debris), negative: none.
    smoke: f32,
    /// Palette colour index drawn last (sparks cool down).
    colour: u8,
}

#[derive(Resource, Default)]
struct Particles {
    puffs: Vec<Puff>,
    chunks: Vec<Chunk>,
    /// Hidden solid particles ready for reuse.
    free: Vec<(Entity, ChunkKind)>,
    batch: Option<(Entity, Handle<Mesh>)>,
    material: Option<Handle<ParticleMaterial>>,
    meshes: Option<(Handle<Mesh>, Handle<Mesh>)>,
    colours: Vec<(u8, Handle<PaletteMaterial>)>,
    /// Sim smoke: last slots seen and seconds since they changed; per-slot emitter clocks.
    smoke_state: [Smoke; 16],
    smoke_age: f32,
    smoke_clock: [f32; 16],
    /// Flash lights: entity, age, life, strength.
    lights: Vec<(Entity, f32, f32, f32)>,
    /// The damaged player aircraft's trail: last emitter point and the puffs owed (smoke, fire).
    trail: Option<DVec3>,
    trail_owed: (f32, f32),
    rng: u32,
}

impl Particles {
    fn rand(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / 16_777_216.0
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.rand()
    }

    /// A random direction, flattened towards the upper half by `up` (0 sphere .. 1 hemisphere).
    fn dir(&mut self, up: f32) -> Vec3 {
        let z = self.range(-1.0 + up, 1.0);
        let a = self.range(0.0, std::f32::consts::TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), z, r * a.sin())
    }

    #[allow(clippy::too_many_arguments)]
    fn puff(
        &mut self,
        pos: DVec3,
        vel: Vec3,
        life: f32,
        size: (f32, f32),
        look: PuffLook,
        drag: f32,
        lift: f32,
        opacity: f32,
    ) {
        if self.puffs.len() >= MAX_PUFFS {
            // Drop the oldest smoke (relative to its life) first.
            if let Some(i) = (0..self.puffs.len()).max_by(|&a, &b| {
                let (pa, pb) = (&self.puffs[a], &self.puffs[b]);
                (pa.age / pa.life).total_cmp(&(pb.age / pb.life))
            }) {
                self.puffs.swap_remove(i);
            }
        }
        let rot = self.range(0.0, std::f32::consts::TAU);
        let spin = self.range(-0.6, 0.6);
        let seed = self.rand();
        self.puffs.push(Puff {
            pos,
            vel,
            age: 0.0,
            life,
            size,
            look,
            rot,
            spin,
            seed,
            drag,
            lift,
            opacity,
        });
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn gradient(stops: &[(f32, Vec3)], t: f32) -> Vec3 {
    for w in stops.windows(2) {
        if t <= w[1].0 {
            return w[0].1.lerp(w[1].1, (t - w[0].0) / (w[1].0 - w[0].0));
        }
    }
    stops[stops.len() - 1].1
}

fn cleanup(mut commands: Commands, mut p: ResMut<Particles>, mut ex: ResMut<Explosions>) {
    for c in &p.chunks {
        commands.entity(c.entity).try_despawn();
    }
    for &(e, _) in &p.free {
        commands.entity(e).try_despawn();
    }
    for &(e, ..) in &p.lights {
        commands.entity(e).try_despawn();
    }
    if let Some((e, _)) = p.batch {
        commands.entity(e).try_despawn();
    }
    // Keep the shared assets.
    let (material, meshes, colours) =
        (p.material.take(), p.meshes.take(), std::mem::take(&mut p.colours));
    *p = Particles { material, meshes, colours, ..default() };
    ex.pending.clear();
}

/// Starts this frame's blasts and the sim smoke.
#[allow(clippy::too_many_arguments)]
fn spawn_blasts(
    mut commands: Commands,
    combat: Res<Combat>,
    mut ex: ResMut<Explosions>,
    mut p: ResMut<Particles>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    palette: Res<crate::palette::Palette>,
) {
    let p = &mut *p;
    if p.rng == 0 {
        p.rng = 0x9E37_79B9 ^ (time.elapsed_secs() * 1000.0) as u32;
    }
    if p.meshes.is_none() {
        // Spark: a thin bar along +Z; debris: a small irregular chunk.
        let spark = meshes.add(Cuboid::new(0.12, 0.12, 1.0));
        let debris = meshes.add(Tetrahedron::default());
        p.meshes = Some((spark, debris));
    }
    let mut blasts: Vec<(DVec3, Blast, bool)> =
        ex.pending.drain(..).map(|(pos, b)| (pos, b, true)).collect();
    for &(x, y, alt, kind) in &combat.explosions {
        let ground = alt <= 0;
        blasts.push((
            map_point(x as f64 + 0.5, y as f64 + 0.5, alt.max(0) as f64),
            Blast::from_kind(kind),
            ground,
        ));
    }
    for &(x, y) in &combat.round_impacts {
        blasts.push((map_point(x, y, 0.0), Blast::GunHit, true));
    }
    for (pos, blast, ground) in blasts {
        spawn_blast(&mut commands, p, &mut materials, &palette.globals, pos, blast, ground);
    }

    // Sim smoke slots: each live slot emits a rising dark puff every so often.
    let dt = time.delta_secs();
    let smoke = *combat.sim.smoke();
    if smoke != p.smoke_state {
        p.smoke_state = smoke;
        p.smoke_age = 0.0;
    } else {
        p.smoke_age += dt;
    }
    if p.smoke_age < SMOKE_TIMEOUT {
        for (i, s) in smoke.iter().enumerate() {
            if s.x == 0 || !(0..16).contains(&s.age) {
                continue;
            }
            p.smoke_clock[i] -= dt;
            if p.smoke_clock[i] > 0.0 {
                continue;
            }
            p.smoke_clock[i] = p.range(0.25, 0.45);
            let pos = map_point(s.x as f64 + 0.5, s.y as f64 + 0.5, s.alt as f64)
                + (p.dir(0.0) * 4.0).as_dvec3();
            let shade = p.range(0.05, 0.35);
            let r = 6.0 + 1.5 * ((s.age & 0xE) >> 1) as f32;
            let life = p.range(5.0, 8.0);
            p.puff(pos, Vec3::Y * 2.0, life, (r, r * 3.0), PuffLook::Smoke(shade), 0.3, 1.5, 0.75);
        }
    }
}

/// Where the damaged aircraft's smoke and fire come out (aircraft frame, metres: behind the
/// middle, at the engines).
/// Metres flown per smoke puff and per flame puff.
const SMOKE_SPACING: f32 = 4.0;
const FIRE_SPACING: f32 = 2.5;

/// The player's aircraft trails smoke once it is damaged, and flames while it is on fire. The
/// smoke puffs billow for a few seconds and fade into the long smoke ribbon
/// ([`crate::trails`]) that carries the trail on for kilometres.
fn damage_trail(
    combat: Res<Combat>,
    mut p: ResMut<Particles>,
    time: Res<Time>,
    player: Query<(&crate::flight::Aircraft, &WorldPos), With<crate::flight::Player>>,
) {
    let p = &mut *p;
    let dt = time.delta_secs();
    let d = combat.sim.damage_effects();
    let Ok((a, pos)) = player.single() else { return };
    if d.bits == 0 || dt <= 0.0 || a.ended.is_some() {
        p.trail = None;
        p.trail_owed = (0.0, 0.0);
        return;
    }
    let at = pos.0 + (a.orientation * crate::trails::DAMAGE_EMITTER).as_dvec3();
    // A jump (a respawn, the floating origin) starts a new trail.
    let last = p.trail.filter(|l| l.distance(at) < 400.0).unwrap_or(at);
    p.trail = Some(at);
    let seg = at - last;
    let vel = (seg / dt as f64).as_vec3();
    let len = seg.length() as f32;
    let fire = d.bits & stealth_sim::dmg::FIRE != 0;
    let hits = d.hits.max(1) as f32;
    // Severity 0 (one hit) .. 1 (eight or more).
    let sev = ((hits - 1.0) / 7.0).clamp(0.0, 1.0);
    let heavy = if fire { 1.0 } else { sev };

    // Smoke: drifts in the air where it was left, growing and fading; light grey and thin at
    // first, darker and thicker with more damage or a fire.
    p.trail_owed.0 += len / SMOKE_SPACING + dt * 4.0;
    let n = p.trail_owed.0.floor();
    p.trail_owed.0 -= n;
    for i in 0..n as usize {
        let k = (i as f64 + p.rand() as f64) / n as f64;
        let o = last + seg * k + (p.dir(0.0) * 0.6).as_dvec3();
        let shade = (0.75 - 0.7 * heavy) * p.range(0.7, 1.1);
        let life = p.range(4.0, 6.0);
        let r = 1.2 + 1.3 * heavy;
        let v = vel * 0.1 + p.dir(0.0) * 1.5;
        let opacity = 0.25 + 0.45 * heavy;
        p.puff(o, v, life, (r, r * 5.0), PuffLook::Trail(shade), 0.8, 0.3, opacity);
    }

    // Flames: short lived glowing puffs streaming back from the engines.
    if fire {
        p.trail_owed.1 += len / FIRE_SPACING + dt * 30.0;
        let n = p.trail_owed.1.floor();
        p.trail_owed.1 -= n;
        for i in 0..n as usize {
            let k = (i as f64 + p.rand() as f64) / n as f64;
            let o = last + seg * k + (p.dir(0.0) * 0.4).as_dvec3();
            // Most of the aircraft's speed, so the flame stays close behind it.
            let v = vel * p.range(0.75, 0.9) + p.dir(0.0) * 1.5;
            let life = p.range(0.15, 0.3);
            let s = p.range(0.7, 1.2);
            p.puff(o, v, life, (s, s * 1.8), PuffLook::Fire, 1.5, 1.0, 0.85);
        }
    }
}

fn colour_material(
    p: &mut Particles,
    materials: &mut Assets<PaletteMaterial>,
    globals: &Handle<ShaderBuffer>,
    idx: u8,
) -> Handle<PaletteMaterial> {
    if let Some((_, m)) = p.colours.iter().find(|(c, _)| *c == idx) {
        return m.clone();
    }
    let m = materials.add(PaletteMaterial {
        params: PaletteParams { word: 0xFF00 | idx as u32, flags: FLAG_LINE, ..default() },
        globals: globals.clone(),
    });
    p.colours.push((idx, m.clone()));
    m
}

/// Palette colours: sparks cool from white through yellow to red, gun sparks from yellow;
/// debris is dark.
const SPARK_COLOURS: [u8; 3] = [15, 17, 41];
const GUN_SPARK_COLOURS: [u8; 3] = [17, 17, 41];
const DEBRIS_COLOURS: [u8; 2] = [5, 8];

#[allow(clippy::too_many_arguments)]
fn spawn_chunk(
    commands: &mut Commands,
    p: &mut Particles,
    materials: &mut Assets<PaletteMaterial>,
    globals: &Handle<ShaderBuffer>,
    kind: ChunkKind,
    pos: DVec3,
    vel: Vec3,
    life: f32,
    size: f32,
    smoke: bool,
) {
    if p.chunks.len() >= MAX_CHUNKS {
        return;
    }
    let colour = match kind {
        ChunkKind::Spark => SPARK_COLOURS[0],
        ChunkKind::GunSpark => GUN_SPARK_COLOURS[0],
        ChunkKind::Debris => DEBRIS_COLOURS[(p.rand() * 2.0) as usize % 2],
    };
    let material = colour_material(p, materials, globals, colour);
    let entity = match p.free.iter().position(|&(_, k)| k == kind) {
        Some(i) => {
            let (e, _) = p.free.swap_remove(i);
            commands.entity(e).insert(MeshMaterial3d(material));
            e
        }
        None => {
            let (spark, debris) = p.meshes.clone().unwrap();
            let mesh = if kind == ChunkKind::Debris { debris } else { spark };
            commands
                .spawn((
                    SimObject,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    MeshTag(mesh_tag(LEVEL_AIRCRAFT, 0, true)),
                    WorldPos(pos),
                    Transform::from_scale(Vec3::ZERO),
                    Visibility::Hidden,
                    bevy::light::NotShadowCaster,
                ))
                .id()
        }
    };
    let rot = Quat::from_scaled_axis(p.dir(0.0) * p.range(0.0, 3.0));
    let spin = p.dir(0.0) * p.range(2.0, 10.0);
    let smoke = if smoke { p.range(0.0, 0.1) } else { -1.0 };
    p.chunks.push(Chunk { entity, kind, pos, vel, rot, spin, age: 0.0, life, size, smoke, colour });
}

/// One blast's particles. `ground`: on the ground (debris and dust thrown up, not around).
#[allow(clippy::too_many_arguments)]
fn spawn_blast(
    commands: &mut Commands,
    p: &mut Particles,
    materials: &mut Assets<PaletteMaterial>,
    globals: &Handle<ShaderBuffer>,
    pos: DVec3,
    blast: Blast,
    ground: bool,
) {
    // (fireball radius, fireball puffs, sparks, debris, smoke puffs, flash light strength)
    let (r, fire, sparks, debris, smoke, light) = match blast {
        Blast::GunHit => (0.0, 0, 5, 0, 1, 0.0),
        Blast::Kill => (14.0, 9, 28, 14, 7, 2.5),
        Blast::Bomb => (22.0, 12, 34, 22, 10, 3.0),
        Blast::Crash => (30.0, 16, 40, 30, 14, 3.5),
    };
    let up = if ground { 0.7 } else { 0.0 };
    // Ground blasts sit on the ground (their middle a little above it).
    let centre = if ground { pos + DVec3::Y * (r as f64 * 0.4) } else { pos };

    if blast == Blast::GunHit {
        // A small bright yellow flash with yellow sparks.
        let at = pos + DVec3::Y * 0.5;
        for _ in 0..sparks {
            let v = p.dir(if ground { 0.5 } else { 0.0 }) * p.range(15.0, 45.0);
            let life = p.range(0.2, 0.45);
            spawn_chunk(
                commands,
                p,
                materials,
                globals,
                ChunkKind::GunSpark,
                at,
                v,
                life,
                1.2,
                false,
            );
        }
        let look = if ground { PuffLook::Dust } else { PuffLook::Smoke(0.6) };
        let life = p.range(1.2, 2.0);
        p.puff(pos, Vec3::Y * 1.5, life, (1.0, 4.0), look, 1.0, 0.5, 0.7);
        p.puff(at, Vec3::ZERO, 0.18, (6.0, 9.0), PuffLook::GunFlash, 0.0, 0.0, 1.0);
        return;
    }

    // Flash and fireball: overlapping glowing puffs pushed out from the middle.
    p.puff(centre, Vec3::ZERO, 0.18, (r * 1.2, r * 2.2), PuffLook::Flash, 0.0, 0.0, 0.9);
    for _ in 0..fire {
        let d = p.dir(up);
        let o = d * p.range(0.0, r * 0.5);
        let v = d * p.range(r * 0.6, r * 1.6);
        let life = p.range(0.7, 1.3) * (r / 20.0).sqrt();
        let s = p.range(0.45, 0.75) * r;
        p.puff(centre + o.as_dvec3(), v, life, (s * 0.4, s * 1.1), PuffLook::Fire, 3.5, 3.0, 0.95);
    }
    // Sparks: fast, thin, falling streaks.
    for _ in 0..sparks {
        let v = p.dir(up * 0.6) * p.range(r * 2.0, r * 5.0);
        let life = p.range(0.5, 1.4);
        let size = p.range(0.8, 1.6);
        spawn_chunk(
            commands,
            p,
            materials,
            globals,
            ChunkKind::Spark,
            centre,
            v,
            life,
            size,
            false,
        );
    }
    // Debris: tumbling chunks thrown out on ballistic paths, some trailing smoke.
    for _ in 0..debris {
        let v =
            p.dir(up) * p.range(r * 0.8, r * 2.8) + Vec3::Y * if ground { r * 0.8 } else { 0.0 };
        let life = p.range(3.0, 6.0);
        let smoking = p.rand() < 0.35;
        let size = p.range(0.4, 1.5) * (r / 14.0).sqrt();
        spawn_chunk(
            commands,
            p,
            materials,
            globals,
            ChunkKind::Debris,
            centre,
            v,
            life,
            size,
            smoking,
        );
    }
    // Smoke left behind: dark, slowly rising and spreading.
    for i in 0..smoke {
        let d = p.dir(up);
        let o = d * p.range(0.0, r * 0.7);
        let shade = p.range(0.0, 0.25);
        let life = p.range(6.0, 11.0);
        let s = p.range(0.5, 0.8) * r;
        let v = d * r * 0.25 + Vec3::Y * p.range(1.0, 4.0);
        // Born a little later than the fireball so it shows as the fire dies.
        p.puff(
            centre + o.as_dvec3(),
            v,
            life,
            (s * 0.6, s * 2.2),
            PuffLook::Smoke(shade),
            0.5,
            1.2,
            0.85,
        );
        let last = p.puffs.len() - 1;
        p.puffs[last].age = -0.15 - 0.03 * i as f32;
    }
    if ground {
        // Dust ring rolling out along the ground.
        for _ in 0..10 {
            let a = p.range(0.0, std::f32::consts::TAU);
            let d = Vec3::new(a.cos(), 0.05, a.sin());
            let s = r * p.range(0.35, 0.55);
            let life = p.range(3.0, 5.0);
            p.puff(
                pos + DVec3::Y * (s as f64 * 0.4),
                d * r * 1.8,
                life,
                (s * 0.5, s * 1.6),
                PuffLook::Dust,
                1.4,
                0.3,
                0.7,
            );
        }
    }
    if light > 0.0 {
        let e = commands
            .spawn((SimObject, WorldPos(centre), Transform::default(), Visibility::Inherited))
            .id();
        p.lights.push((e, 0.0, 1.4, light));
    }
}

/// Moves the particles, rebuilds the puff batch and places the solid ones.
#[allow(clippy::too_many_arguments)]
fn simulate(
    mut commands: Commands,
    mut p: ResMut<Particles>,
    time: Res<Time>,
    origin: Res<FloatingOrigin>,
    palette: Option<Res<crate::palette::Palette>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut puff_materials: ResMut<Assets<ParticleMaterial>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    cams: Query<&GlobalTransform, With<crate::camera::EyeCamera>>,
    mut q: Query<(&mut WorldPos, &mut Transform, &mut Visibility)>,
) {
    let Some(palette) = palette else { return };
    let p = &mut *p;
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    // --- solid particles
    let mut spawn_smoke = Vec::new();
    let mut i = 0;
    while i < p.chunks.len() {
        let c = &mut p.chunks[i];
        c.age += dt;
        if c.age >= c.life {
            let c = p.chunks.swap_remove(i);
            if let Ok((_, _, mut v)) = q.get_mut(c.entity) {
                v.set_if_neq(Visibility::Hidden);
            }
            p.free.push((c.entity, c.kind));
            continue;
        }
        let drag = if c.kind == ChunkKind::Debris { 0.25 } else { 1.2 };
        c.vel.y -= GRAVITY * dt;
        c.vel *= (-drag * dt).exp();
        c.pos += (c.vel * dt).as_dvec3();
        let floor = 0.0;
        if c.pos.y < floor {
            c.pos.y = floor;
            if c.kind == ChunkKind::Debris {
                c.vel = Vec3::new(c.vel.x * 0.4, (-c.vel.y * 0.3).max(0.0), c.vel.z * 0.4);
                c.spin *= 0.5;
                if c.vel.length() < 2.0 {
                    c.vel = Vec3::ZERO;
                    c.spin = Vec3::ZERO;
                }
            } else {
                c.age = c.life;
            }
        }
        c.rot = (Quat::from_scaled_axis(c.spin * dt) * c.rot).normalize();
        if c.smoke >= 0.0 && c.age < c.life * 0.7 {
            c.smoke -= dt;
            if c.smoke < 0.0 {
                c.smoke = 0.06;
                spawn_smoke.push((c.pos, c.size));
            }
        }
        let t = c.age / c.life;
        let (rot, scale) = match c.kind {
            ChunkKind::Spark | ChunkKind::GunSpark => {
                let speed = c.vel.length();
                let dir = c.vel.try_normalize().unwrap_or(Vec3::Y);
                let w = c.size * 0.6 * (1.0 - t * 0.5);
                (
                    Quat::from_rotation_arc(Vec3::Z, dir),
                    Vec3::new(w, w, (speed * 0.05).clamp(0.4, 6.0) * c.size),
                )
            }
            // Debris shrinks away at the end of its life.
            ChunkKind::Debris => (c.rot, Vec3::splat(c.size * (1.0 - smooth(0.8, 1.0, t)))),
        };
        if let Ok((mut pos, mut tf, mut v)) = q.get_mut(c.entity) {
            pos.0 = c.pos;
            tf.rotation = rot;
            tf.scale = scale;
            v.set_if_neq(Visibility::Inherited);
        }
        if c.kind != ChunkKind::Debris {
            let colours =
                if c.kind == ChunkKind::Spark { SPARK_COLOURS } else { GUN_SPARK_COLOURS };
            let want = colours[((t * 3.0) as usize).min(2)];
            if want != c.colour {
                c.colour = want;
                let e = c.entity;
                let m = colour_material(p, &mut materials, &palette.globals, want);
                commands.entity(e).insert(MeshMaterial3d(m));
            }
        }
        i += 1;
    }
    for (pos, size) in spawn_smoke {
        let s = size * 1.5;
        let life = p.range(2.0, 3.5);
        p.puff(pos, Vec3::Y, life, (s, s * 4.0), PuffLook::Smoke(0.1), 0.5, 1.0, 0.6);
    }

    // --- flash lights
    p.lights.retain_mut(|(e, age, life, k)| {
        *age += dt;
        if *age >= *life {
            commands.entity(*e).try_despawn();
            return false;
        }
        let t = *age / *life;
        let s = (1.0 - t).powi(2) * *k;
        commands.entity(*e).insert(crate::lighting::LightEmitter {
            color: Vec3::new(1.0, 0.75 - 0.4 * t, 0.45 - 0.4 * t) * s,
            range: 250.0,
        });
        true
    });

    // --- translucent puffs
    p.puffs.retain_mut(|f| {
        f.age += dt;
        if f.age < 0.0 {
            return true;
        }
        if f.age >= f.life {
            return false;
        }
        f.vel *= (-f.drag * dt).exp();
        f.vel.y += f.lift * dt;
        f.pos += (f.vel * dt).as_dvec3();
        f.rot += f.spin * dt;
        true
    });

    if p.material.is_none() {
        p.material = Some(
            puff_materials
                .add(ParticleMaterial { unused: Vec4::ZERO, globals: palette.globals.clone() }),
        );
    }
    let eye =
        cams.single().map(|g| origin.origin + g.translation().as_dvec3()).unwrap_or(origin.origin);
    let anchor = eye;
    // Back to front.
    let mut order: Vec<(f64, usize)> = p
        .puffs
        .iter()
        .enumerate()
        .filter(|(_, f)| f.age >= 0.0)
        .map(|(i, f)| (-(f.pos - eye).length_squared(), i))
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));

    let n = order.len();
    // The batch mesh is created with its first particles (an empty mesh upsets the render
    // world's mesh allocator) and left alone while there is nothing to draw.
    let mut drawn = false;
    if n > 0 {
        let mut pos = Vec::with_capacity(4 * n);
        let mut corner = Vec::with_capacity(4 * n);
        let mut colour = Vec::with_capacity(4 * n);
        let mut look = Vec::with_capacity(4 * n);
        let mut idx = Vec::with_capacity(6 * n);
        for &(_, i) in &order {
            let f = &p.puffs[i];
            let t = f.age / f.life;
            let size = f.size.0 + (f.size.1 - f.size.0) * (1.0 - (1.0 - t).powi(3));
            let (rgb, a, glow) = f.colour(t);
            let local = (f.pos - anchor).as_vec3();
            let k = pos.len() as u32;
            for (cx, cy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                pos.push(local);
                corner.push([cx, cy, size, f.rot]);
                colour.push([rgb.x, rgb.y, rgb.z, a]);
                look.push([glow, f.seed]);
            }
            idx.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
        }
        let fill = |m: &mut Mesh| {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            m.insert_attribute(ATTRIBUTE_CORNER, corner);
            m.insert_attribute(ATTRIBUTE_COLOUR, colour);
            m.insert_attribute(ATTRIBUTE_LOOK, look);
            m.insert_indices(Indices::U32(idx));
        };
        match &p.batch {
            Some((_, mesh)) => drawn = meshes.get_mut(mesh).map(|mut m| fill(&mut m)).is_some(),
            None => {
                let mut m =
                    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
                fill(&mut m);
                let mesh = meshes.add(m);
                let e = commands
                    .spawn((
                        SimObject,
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(p.material.clone().unwrap()),
                        WorldPos(anchor),
                        Transform::default(),
                        Visibility::Inherited,
                        NoFrustumCulling,
                        bevy::light::NotShadowCaster,
                    ))
                    .id();
                p.batch = Some((e, mesh));
                return;
            }
        }
    }
    let Some((batch, _)) = p.batch else { return };
    if let Ok((mut pos, _, mut v)) = q.get_mut(batch) {
        pos.0 = anchor;
        v.set_if_neq(if drawn { Visibility::Inherited } else { Visibility::Hidden });
    }
}
