//! Fast-turning rotors drawn as a blurred disc (shader: assets/shaders/rotor.wgsl) instead of
//! their blades. The disc is a child of the rotor's articulated part, so it turns with the drawn
//! rotor angle; the shader layers the optical effects of a rotor too fast to follow: the haze of
//! the blades' sweep (denser towards the root), motion-blurred ghost blades turning with the
//! drawn angle, the blade roots, the tip path, faint bands along the span, a glint where the
//! blades catch the sun, and a disc that thickens seen edge on.
//!
//! The hub stays solid. The blades fade into the disc with the rotor speed (`RotorBlur` on a
//! model root: the own helicopter's spin-up and spin-down); other rotors are always blurred.

use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;

const SHADER: &str = "shaders/rotor.wgsl";

/// Blade chord as a fraction of the rotor radius (UH-60: 0.53 m of 8.2 m).
const CHORD: f32 = 0.065;
/// Sides of the disc polygon (its rim is softened in the shader).
const SEGMENTS: usize = 96;
/// Rotor speeds (1 = 100%) over which the blades fade into the disc.
const BLUR_SPEED: (f32, f32) = (0.2, 0.55);
/// Blur at which the blades are hidden.
const BLADES_HIDDEN: f32 = 0.5;

pub struct RotorBlurPlugin;

impl Plugin for RotorBlurPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<RotorMaterial>::default())
            .add_systems(Update, (own_rotor_blur, apply_rotor_blur).chain());
    }
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct RotorParams {
    /// Blade palette index.
    colour: u32,
    blades: u32,
    /// Hub radius and blade chord (fractions of the rotor radius).
    hub: f32,
    chord: f32,
    /// 0 (blades) .. 1 (disc).
    blur: f32,
    /// The part's axis (`models::ArticulatedPart`: 0 = y, 1 = x, 2 = z).
    axis: u32,
    _pad: Vec2,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct RotorMaterial {
    #[uniform(0)]
    params: RotorParams,
    #[storage(1, read_only)]
    globals: Handle<ShaderBuffer>,
}

impl Material for RotorMaterial {
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
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        // Seen from above and below.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// A rotor found among a part's primitives (model units, in the part's frame).
pub struct RotorShape {
    pub radius: f32,
    /// Hub radius as a fraction of `radius`.
    pub hub: f32,
    /// Height of the blades along the axis.
    pub offset: f32,
    pub blades: u32,
    /// Palette colour of the blades.
    pub colour: u8,
    /// Which primitives are blades (the others are the hub).
    pub blade_prims: Vec<bool>,
}

/// Plane coordinates (u, v) and axial height of a point of a part turning about `axis`
/// (`models::ArticulatedPart`: 0 = y, 1 = x, 2 = z), with u, v ordered so the drawn rotor
/// turns towards increasing atan2(v, u).
fn plane(axis: u8, p: Vec3) -> (Vec2, f32) {
    match axis {
        0 => (Vec2::new(p.x, p.z), p.y),
        1 => (Vec2::new(p.z, p.y), p.x),
        _ => (Vec2::new(p.y, p.x), p.z),
    }
}

fn from_plane(axis: u8, uv: Vec2, h: f32) -> Vec3 {
    match axis {
        0 => Vec3::new(uv.x, h, uv.y),
        1 => Vec3::new(h, uv.y, uv.x),
        _ => Vec3::new(uv.y, uv.x, h),
    }
}

/// Recognises a rotor from its primitives' (colour, triangles): blades reach well out from the
/// axis in a few separate directions. A part sweeping all round (a rotodome) is not one.
pub fn rotor_shape(axis: u8, prims: &[(u8, Vec<[Vec3; 3]>)]) -> Option<RotorShape> {
    // Points spread over each primitive's faces (a long blade may be one thin triangle).
    const STEPS: usize = 6;
    let points: Vec<Vec<Vec3>> = prims
        .iter()
        .map(|(_, tris)| {
            let mut out = Vec::new();
            for t in tris {
                for i in 0..=STEPS {
                    for j in 0..=STEPS - i {
                        let (a, b) = (i as f32 / STEPS as f32, j as f32 / STEPS as f32);
                        out.push(t[0] + (t[1] - t[0]) * a + (t[2] - t[0]) * b);
                    }
                }
            }
            out
        })
        .collect();
    let reach = |p: &[Vec3]| p.iter().map(|&v| plane(axis, v).0.length()).fold(0.0, f32::max);
    let radius = points.iter().map(|p| reach(p)).fold(0.0, f32::max);
    if radius <= 0.0 {
        return None;
    }
    let blade_prims: Vec<bool> = points.iter().map(|p| reach(p) > 0.5 * radius).collect();
    let blade_points =
        || points.iter().zip(&blade_prims).filter(|(_, b)| **b).flat_map(|(p, _)| p.iter());
    // Directions the outer half of the blades point in, in 5 degree bins.
    const BINS: usize = 72;
    let mut bins = [false; BINS];
    for &v in blade_points() {
        let (uv, _) = plane(axis, v);
        if uv.length() > 0.5 * radius {
            let a = uv.y.atan2(uv.x).rem_euclid(std::f32::consts::TAU);
            bins[(a / std::f32::consts::TAU * BINS as f32) as usize % BINS] = true;
        }
    }
    let filled = bins.iter().filter(|&&b| b).count();
    let blades = (0..BINS).filter(|&i| bins[i] && !bins[(i + BINS - 1) % BINS]).count() as u32;
    if !(2..=8).contains(&blades) || filled > BINS / 2 {
        return None;
    }
    let (sum, n) = blade_points().fold((0.0, 0), |(s, n), &v| (s + plane(axis, v).1, n + 1));
    let hub = points
        .iter()
        .zip(&blade_prims)
        .filter(|(_, b)| !**b)
        .map(|(p, _)| reach(p))
        .fold(0.05 * radius, f32::max);
    let colour = prims
        .iter()
        .zip(&blade_prims)
        .filter(|(_, b)| **b)
        .max_by_key(|((_, t), _)| t.len())
        .map_or(15, |((c, _), _)| *c);
    Some(RotorShape {
        radius,
        hub: hub / radius,
        offset: sum / n.max(1) as f32,
        blades,
        colour,
        blade_prims,
    })
}

/// A blade mesh of a rotor, shown only while its model's rotor turns slowly.
#[derive(Component)]
pub struct RotorBlade {
    pub root: Entity,
}

/// A rotor's blurred disc.
#[derive(Component)]
pub struct RotorDisc {
    pub root: Entity,
    material: Handle<RotorMaterial>,
    blur: f32,
}

/// On a model root: how far its rotors are blurred, 0 (blades) .. 1 (disc). Without it, 1.
#[derive(Component, Clone, Copy)]
pub struct RotorBlur(pub f32);

/// Spawns the disc of a rotor as a child of its part (`parent`); returns it.
pub fn spawn_disc(
    commands: &mut Commands,
    parent: Entity,
    root: Entity,
    axis: u8,
    shape: &RotorShape,
    globals: Handle<ShaderBuffer>,
) -> Entity {
    let mut pos = Vec::with_capacity(2 * SEGMENTS);
    let mut uv = Vec::with_capacity(2 * SEGMENTS);
    let mut idx = Vec::with_capacity(6 * SEGMENTS);
    // A ring from inside the hub to just past the tips (the shader fades both edges).
    let (inner, outer) = (shape.hub * 0.5, 1.02);
    for i in 0..SEGMENTS {
        let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let d = Vec2::new(a.cos(), a.sin());
        for r in [inner, outer] {
            pos.push(from_plane(axis, d * r * shape.radius, shape.offset).to_array());
            uv.push((d * r).to_array());
        }
        let (j, k) = (2 * i as u32, 2 * ((i + 1) % SEGMENTS) as u32);
        idx.extend([j, j + 1, k + 1, j, k + 1, k]);
    }
    let mut mesh =
        Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(idx));
    let material = RotorMaterial {
        params: RotorParams {
            colour: shape.colour as u32,
            blades: shape.blades,
            hub: shape.hub,
            chord: CHORD,
            blur: 1.0,
            axis: axis as u32,
            _pad: Vec2::ZERO,
        },
        globals,
    };
    let disc = commands
        .spawn((Transform::default(), Visibility::Inherited, bevy::light::NotShadowCaster))
        .id();
    commands.entity(parent).add_child(disc);
    commands.queue(move |world: &mut World| {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let material = world.resource_mut::<Assets<RotorMaterial>>().add(material);
        if let Ok(mut e) = world.get_entity_mut(disc) {
            e.insert((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                RotorDisc { root, material, blur: 1.0 },
            ));
        }
    });
    disc
}

/// The own helicopter's rotors blur with its rotor speed.
#[allow(clippy::type_complexity)]
fn own_rotor_blur(
    mut commands: Commands,
    player: Query<&crate::flight::Aircraft, With<crate::flight::Player>>,
    flight: Option<Res<crate::flight::FlightConstants>>,
    own: Query<
        (Entity, Option<&RotorBlur>),
        Or<(With<crate::player::OwnModel>, With<crate::player::OwnRotor>)>,
    >,
) {
    if !flight.is_some_and(|k| k.heli.is_some()) {
        return;
    }
    let Ok(a) = player.single() else { return };
    let (lo, hi) = BLUR_SPEED;
    let t = ((a.rotor_speed() - lo) / (hi - lo)).clamp(0.0, 1.0);
    let blur = t * t * (3.0 - 2.0 * t);
    for (e, was) in &own {
        if was.is_none_or(|b| (b.0 - blur).abs() > 0.005) {
            // The model may be going this frame (another aircraft type).
            commands.entity(e).try_insert(RotorBlur(blur));
        }
    }
}

fn apply_rotor_blur(
    roots: Query<&RotorBlur>,
    mut discs: Query<&mut RotorDisc>,
    mut blades: Query<(&RotorBlade, &mut Visibility)>,
    mut materials: ResMut<Assets<RotorMaterial>>,
) {
    let blur = |root: Entity| roots.get(root).map_or(1.0, |b| b.0);
    for mut d in &mut discs {
        let b = blur(d.root);
        if d.blur != b {
            d.blur = b;
            if let Some(mut m) = materials.get_mut(&d.material) {
                m.params.blur = b;
            }
        }
    }
    for (blade, mut vis) in &mut blades {
        let show = blur(blade.root) < BLADES_HIDDEN;
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
}
