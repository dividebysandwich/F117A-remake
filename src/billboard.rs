use std::cmp;

use bevy::math::Vec3;
use bevy::prelude::{Query, Transform, With, Without, GlobalTransform, info};

use crate::{LightBillboard, MainCamera};

pub fn auto_scale_billboards(
    mut billboards: Query<(&GlobalTransform, &mut Transform, With<LightBillboard>)>,
    camera: Query<(&MainCamera, &GlobalTransform, Without<LightBillboard>)>,
) {
    let (_cam, c_transform, _) = camera.single();

    for (mut b_global_transform, mut b_transform, _) in billboards.iter_mut() {
        let cam_distance = (c_transform.translation().distance(b_global_transform.translation()) * 0.5);
//        let direction = (b_transform.translation - c_transform.translation).normalize();
//        let cam_up = c_transform.rotation * Vec3::Y;
//        let cam_right = cam_up.cross(direction).normalize();
//        let orthogonal = direction.cross(cam_right).normalize();
        b_transform.scale = Vec3::new(cam_distance, cam_distance, cam_distance);
    }
}
