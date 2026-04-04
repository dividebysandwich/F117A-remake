use bevy::prelude::*;

use crate::MainCamera;

/// Marker component for entities that should always face the camera (billboard behavior).
/// This replaces the bevy_mod_billboard crate with a simple camera-facing system.
#[derive(Component)]
pub struct Billboard;

pub struct BillboardPlugin;

impl Plugin for BillboardPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, billboard_face_camera);
    }
}

fn billboard_face_camera(
    camera: Query<&GlobalTransform, With<MainCamera>>,
    mut billboards: Query<&mut Transform, With<Billboard>>,
) {
    let Ok(cam_transform) = camera.single() else { return };
    let cam_rotation = cam_transform.compute_transform().rotation;
    for mut transform in billboards.iter_mut() {
        transform.rotation = cam_rotation;
    }
}
