//! Fullbright "dot" lights (runway lights, city lights, beacons). In the original these are
//! single pixels (zero-length lines) drawn in a solid palette colour. Here each is a quad
//! that the palette shader expands around the dot's position to a constant size in the
//! pixels of whichever view draws it (main view, XR eyes, tracking camera), drawn with the
//! line material (solid colour, distance haze as in the original), so the lights read the same
//! at any resolution, distance or zoom.

use bevy::mesh::MeshTag;
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
    commands.insert_resource(DotAssets { quad: meshes.add(Rectangle::new(1.0, 1.0)) });
}
