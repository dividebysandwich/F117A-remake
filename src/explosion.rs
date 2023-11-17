use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::{util::{get_time_millis, random_vec3, random_u64, random_f32}, player::Player, definitions::*};

pub enum ExplosionType {
    SMALL,
    MEDIUM,
    LARGE,
    HUGE,
}

#[derive(Component)]
pub struct ExplosionEffect {
    pub start_time: u64,
    pub life_time: u64,
}

pub fn spawn_explosion(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    explosion_type: ExplosionType,
    position: &Vec3,
) {
    match explosion_type {
        ExplosionType::SMALL => {
            for _ in 0..20 {
                spawn_explosion_giblet(commands, meshes, materials, position, random_f32(0.05, 0.1), random_u64(2000, 5000));
            }
        },
        ExplosionType::MEDIUM => {

        },
        ExplosionType::LARGE => {
            
        },
        ExplosionType::HUGE => {
            
        },
    };

}

fn spawn_explosion_giblet(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: &Vec3,
    size: f32,
    life_time: u64,
) {
    let explosion_handle = meshes.add(Mesh::from(shape::Cube { size: size }));
    commands
        .spawn(PbrBundle {
            mesh: explosion_handle,
            material: materials.add(StandardMaterial {
                base_color: Color::hex("#ffff33").unwrap(),
                emissive: Color::hex("#ffff33").unwrap(),
                ..default()
            }),
            ..Default::default()
        })
        .insert(ExplosionEffect {
            start_time: get_time_millis(),
            life_time: life_time,
        })
        .insert(Collider::cuboid(size, size, size))
        .insert(CollisionGroups::new(Group::from_bits_truncate(COLLISION_MASK_EFFECT), 
            Group::from_bits_truncate(
                COLLISION_MASK_TERRAIN
            )))
        .insert(RigidBody::Dynamic)
        .insert(Velocity{linvel: random_vec3(10.0), angvel: random_vec3(10.0)})
        .insert(ColliderMassProperties::Density(100.0))
        .insert(PointLightBundle {
            point_light: PointLight {
                color: Color::rgb(1.0, 1.0, 0.3),
                intensity: 100.,
                range: 10.,
                shadows_enabled: false,
                ..default()
            },
            ..default()
        })
        .insert(Transform::from_xyz(position.x, position.y, position.z));
    
}

pub fn handle_explosion_test(
    mut commands: Commands,
    player: Query<(Entity, &Transform), With<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    input: Res<Input<KeyCode>>,
) {
    if input.just_pressed(KeyCode::O) {
        for p in player.iter() {
            let position = p.1.translation;
            spawn_explosion(&mut commands, &mut meshes, &mut materials, ExplosionType::SMALL, &position);
        }
    }
}

pub fn update_explosion_effects(
    mut commands: Commands,
    mut explosion_effects: Query<(Entity, &ExplosionEffect, &mut PointLight)>,
) {
    let time = get_time_millis();
    for (entity, explosion_effect, mut point_light) in explosion_effects.iter_mut() {
        point_light.intensity = 100.0 * (1.0 - (time - explosion_effect.start_time) as f32 / explosion_effect.life_time as f32);
        if time - explosion_effect.start_time > explosion_effect.life_time {
            commands.entity(entity).despawn();
        }
    }
}
