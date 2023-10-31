use bevy::{prelude::*, gizmos};
use bevy::math::Vec3;
use bevy_mod_raycast::prelude::*;

use crate::{LightBillboard, MainCamera, Vehicle};

pub fn auto_scale_billboards(
    mut billboards: Query<(&mut Visibility, &GlobalTransform, &mut Transform, With<LightBillboard>)>,
    camera: Query<(&MainCamera, &GlobalTransform, &Transform, Without<LightBillboard>)>,
    raycast_query: Query<Entity, With<LightBillboard>>,
    mut raycast: Raycast,
) {
    let (_cam, c_global_transform, c_transform, _) = camera.single();

    for (mut b_visibility, b_global_transform, mut b_transform, _) in billboards.iter_mut() {
        let cam_distance = c_global_transform.translation().distance(b_global_transform.translation()) * 0.4;
//        let direction = (b_transform.translation - c_transform.translation).normalize();
//        let cam_up = c_transform.rotation * Vec3::Y;
//        let cam_right = cam_up.cross(direction).normalize();
//        let orthogonal = direction.cross(cam_right).normalize();

        let filter = |entity| !raycast_query.contains(entity);
        let early_exit_test = |_entity| true;
        let settings = RaycastSettings::default()
            .with_filter(&filter)
            .with_early_exit_test(&early_exit_test);

        let hits = raycast.cast_ray(Ray3d::new(c_global_transform.translation(), b_global_transform.translation() - c_global_transform.translation()), &settings);
        *b_visibility = Visibility::Visible;
        b_transform.scale = Vec3::new(cam_distance, cam_distance, cam_distance);
        for (is_first, intersection) in hits {
            *b_visibility = Visibility::Hidden;
        }

    }
}
