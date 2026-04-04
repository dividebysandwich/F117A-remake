use bevy::prelude::*;

use crate::MainCamera;

/// Marker component for entities that should always face the camera (billboard behavior).
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
    mut billboards: Query<(&mut Transform, &GlobalTransform), With<Billboard>>,
) {
    let Ok(cam_global) = camera.single() else { return };
    let cam_pos = cam_global.translation();

    for (mut transform, billboard_global) in billboards.iter_mut() {
        let billboard_pos = billboard_global.translation();
        let to_camera = cam_pos - billboard_pos;
        if to_camera.length_squared() < 0.001 {
            continue;
        }
        // Point the billboard's +Z (face normal) toward the camera.
        // look_to makes local -Z point along the given direction,
        // so we pass the direction AWAY from camera to make +Z face toward it.
        transform.look_to(-to_camera.normalize(), Vec3::Y);
    }
}
