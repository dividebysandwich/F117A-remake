//! Fullbright "dot" lights (runway lights, city lights, beacons). In the original these are
//! single pixels (zero-length lines) drawn in a solid palette colour. Here each is a quad
//! that the palette shader expands around the dot's position to a constant size in the
//! pixels of whichever view draws it (main view, XR eyes, tracking camera), drawn with the
//! line material (solid colour, distance haze as in the original), so the lights read the same
//! at any resolution, distance or zoom.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshTag, PrimitiveTopology};
use bevy::prelude::*;

use crate::palette_material::TAG_DOT;

pub struct DotsPlugin;

impl Plugin for DotsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_dot_assets);
    }
}

/// XR: angular size of a dot in radians (the original pixel's, a little enlarged); the
/// shader caps the per-view pixel size to it (`Globals.dots.x`).
pub const XR_ANGLE: f32 = 0.6 * 1.5 / (crate::cockpit::VSCALE * crate::cockpit::FOCAL);

#[derive(Resource)]
pub struct DotAssets {
    quad: Handle<Mesh>,
}

#[derive(Component)]
pub struct DotLight;

impl DotLight {
    /// Quad, transform and mesh tag (`tag` with the dot bit set); the caller adds the material.
    pub fn bundle(pos: Vec3, assets: &DotAssets, tag: u32) -> impl Bundle {
        (
            DotLight,
            Mesh3d(assets.quad.clone()),
            MeshTag(tag | TAG_DOT),
            Transform::from_translation(pos),
            bevy::light::NotShadowCaster,
        )
    }
}

fn setup_dot_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(DotAssets { quad: meshes.add(dot_batch(&[Vec3::ZERO])) });
}

/// Four vertices per dot at its centre; the shader spreads them by the quad corner (UV).
/// Every dot mesh must be built this way: the shader takes each vertex's position as the dot
/// centre, so a real quad would keep its world size and grow up close.
pub fn dot_batch(points: &[Vec3]) -> Mesh {
    let pos: Vec<Vec3> = points.iter().flat_map(|&p| [p; 4]).collect();
    let corners: Vec<[f32; 2]> = points.iter().flat_map(|_| [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]]).collect();
    let idx: Vec<u32> = (0..points.len() as u32).flat_map(|i| [0, 1, 3, 0, 3, 2].map(|k| 4 * i + k)).collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, corners)
        .with_inserted_indices(Indices::U32(idx))
}
