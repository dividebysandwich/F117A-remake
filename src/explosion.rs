use std::thread::spawn;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use rand::Rng;

use crate::{util::get_time_millis, player::Player, definitions::*};

pub enum ExplosionType {
    SMALL,
    MEDIUM,
    LARGE,
    HUGE,
}

#[derive(Component)]
pub struct ExplosionEffect {
    pub start_time: u64,
}

fn random_vec3(range:f32) -> Vec3 {
    let mut rng = rand::thread_rng();
    Vec3::new(
        rng.gen_range(-range..range),
        rng.gen_range(-range..range),
        rng.gen_range(-range..range),
    )
}

pub fn spawn_explosion(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    explosion_type: ExplosionType,
    position: Vec3,
) {
    info!("spawning explosion");
    let transform = Transform::from_xyz(position.x, position.y, position.z);
    match explosion_type {
        ExplosionType::SMALL => {
            for _ in 0..40 {
                spawn_explosion_giblet(commands, meshes, materials, position, 0.1);
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

pub fn spawn_explosion_giblet(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    size: f32
) {
    let transform = Transform::from_xyz(position.x, position.y, position.z);
    let explosion_handle = meshes.add(Mesh::from(shape::Cube { size: size }));
    info!("spawning explosion giblet");

    commands
        .spawn(PbrBundle {
            mesh: explosion_handle,
            material: materials.add(StandardMaterial {
                base_color: Color::hex("#ffff33").unwrap(),
                emissive: Color::hex("#ffff33").unwrap(),
                ..default()
            }),
            transform: transform,
            ..Default::default()
        })
        .insert(ExplosionEffect {
            start_time: get_time_millis(),
        })
        .insert(Collider::cuboid(size, size, size))
        .insert(CollisionGroups::new(Group::from_bits_truncate(COLLISION_MASK_EFFECT), 
            Group::from_bits_truncate(
                COLLISION_MASK_TERRAIN
            )))
        .insert(RigidBody::Dynamic)
        .insert(Velocity{linvel: random_vec3(10.0), angvel: random_vec3(10.0)})
        .insert(ColliderMassProperties::Density(100.0));
    
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
            spawn_explosion(&mut commands, &mut meshes, &mut materials, ExplosionType::SMALL, position);
        }
    }
}
