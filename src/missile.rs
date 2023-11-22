use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::{util::*, targeting::Targetable, explosion::{spawn_explosion, ExplosionType}};

#[derive(Component)]
pub struct Missile {
    pub start_time: u64,
    pub launching_vehicle: Entity,
    pub target: Entity,
    pub target_transform: Transform,
    pub target_position: Vec3,
    pub max_turn_rate: f32,
    pub thrust: f32, 
    pub max_thrust: f32,
    pub thrust_ramp: f32,
    pub turn_rate: f32,
    pub turn_ramp: f32,
    pub gain: f32,
    pub ignition_delay: u64,
    pub proximity_fuse_distance: f32,
    pub proximity_fuse_arm_time: u64, 
    pub last_target_distance: f32,
    pub last_position: Vec3,
    pub line_of_sight: Vec3,
    pub acceleration: Vec3,
}

impl Default for Missile {
    fn default() -> Self {
         Missile {
            start_time: get_time_millis(),
            target_position: Vec3::new(0.0, 0.0, 0.0),
            max_turn_rate: 0.5,
            thrust: 0.0,
            max_thrust: 50.0,
            thrust_ramp: 1.0,
            turn_rate: 0.0,
            turn_ramp: 0.2,
            gain: 3.0,
            ignition_delay: 300,
            proximity_fuse_distance: 1.0,
            proximity_fuse_arm_time: 5000,
            last_target_distance: 9999999999.9,
            last_position: Vec3::new(0.0, 0.0, 0.0),
            line_of_sight: Vec3::new(0.0, 0.0, 0.0),
            acceleration: Vec3::new(0.0, 0.0, 0.0),
            launching_vehicle: Entity::PLACEHOLDER,
            target: Entity::PLACEHOLDER,
            target_transform: Transform::from_xyz(0.0, 0.0, 0.0)
         }
    }
}

#[allow(unused_mut)]
pub fn update_missiles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>, 
    mut missiles: Query<(Entity, &mut ExternalForce, &mut Transform, &mut Missile)>, 
    missile_targets: Query<&Transform, (With<Targetable>, Without<Missile>)>,
    all_targets: Query<(Entity, &Transform), (With<Targetable>, Without<Missile>)>,
    time: Res<Time>, 
) {
    for (missile_entity, missile_force, mut missile_transform, mut missile ) in missiles.iter_mut() {
        let target_transform = missile_targets.get(missile.target);
        match target_transform {
            Ok(t) => missile.target_transform = *t,
            Err(e) => info!("Missile targeting error: {}", e),
        }
        update_single_missile(missile_entity, &mut commands, &mut meshes, &mut materials, &asset_server, missile, time.clone(), missile_transform, missile_force, &all_targets);
        
    }

}

fn update_single_missile(
    missile_entity: Entity,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    asset_server: &Res<AssetServer>, 
    mut missile: Mut<Missile>, 
    time: Time, 
    mut missile_transform: Mut<Transform>,
    mut missile_force: Mut<ExternalForce>,
    all_targets: &Query<(Entity, &Transform), (With<Targetable>, Without<Missile>)>,
) {
		
    let current_time = get_time_millis();
    if current_time - missile.start_time < missile.ignition_delay {
        return;
    }

    //We may know our target, or just the coordinates
//    if missile.target != Entity::PLACEHOLDER {
        missile.target_position = missile.target_transform.translation;
//    }/* else if (FLIRSensor) {
//        targetPosition = FLIRSensor.getCurrentLaserCoordinates();
//    }*/

    //Proximity fuze if we have passed the target
    let target_distance = (missile.target_transform.translation - missile_transform.translation).length();

     if current_time - missile.start_time > missile.proximity_fuse_arm_time {
        if target_distance > missile.last_target_distance {
            if missile.last_target_distance < missile.proximity_fuse_distance {
                for (prox_target_entity, prox_target_transform) in all_targets.iter() {
                    let prox_target_distance = (prox_target_transform.translation - missile_transform.translation).length();
                    //Damage all targets within proximity fuse distance
                    if prox_target_distance < missile.proximity_fuse_distance {
                        info!("Missile proximity detonation");
                        commands.entity(prox_target_entity).despawn_recursive();
                        let position = prox_target_transform.translation;
                        spawn_explosion(commands, meshes, materials, ExplosionType::SMALL, &position);
                    }
                }
                commands.spawn(AudioBundle {
                    source: asset_server.load("sounds/xplgmn2.ogg"),
                    ..default()
                });    
                commands.entity(missile_entity).despawn_recursive();
            }
        }
    }
    
    missile.last_target_distance = target_distance;
    missile.last_position = missile_transform.translation;

    
    // Increase thrust over time
    if missile.thrust < missile.max_thrust {
        // don't go over in case thrustRamp is very small
        let increase = time.delta_seconds() * missile.max_thrust / missile.thrust_ramp;
        missile.thrust = (missile.thrust + increase).min(missile.max_thrust);
    }

    // Increase turn rate over time
    if missile.turn_rate < missile.max_turn_rate {
        let increase = time.delta_seconds() * missile.max_turn_rate / missile.turn_ramp;
        missile.turn_rate = (missile.turn_rate + increase).min(missile.max_turn_rate);
    }

    // Proportional Navigation evaluates the rate of change of the Line Of Sight (los) to our target. If the rate of change is zero,
    // the missile is on a collision course. If it is not, we apply a force to correct course.
    let prev_los = missile.line_of_sight;
    missile.line_of_sight = missile.target_position - missile_transform.translation;
    let mut d_los = missile.line_of_sight - prev_los;

    // we only want the component perpendicular to the line of sight
    d_los = d_los - d_los.project_onto(missile.line_of_sight);
        
    // plain PN would be:
    // acceleration = time.delta_seconds() * missile.line_of_sight + dLos * nc;

    // Augmented PN takes acceleration into account
    missile.acceleration = time.delta_seconds() * missile.line_of_sight + d_los * missile.gain + time.delta_seconds() * missile.acceleration * missile.gain / 2.0;
    // Acceleration can't be larger than the maximum thrust
    missile.acceleration = (missile.acceleration * missile.thrust).clamp_length_max(missile.thrust);
        
    // Accelerate towards target
    missile_force.force = missile.acceleration;

//    info!("Missile thrust: {}", missile.thrust);
//    info!("Missile turn_rate: {}", missile.turn_rate);

    // Turn towards target
    let mut target_transform:Transform = Transform::default();
    target_transform = target_transform.looking_to(missile.acceleration.normalize(), Vec3::Y);
    missile_transform.rotation = missile_transform.rotation.lerp(target_transform.rotation, time.delta_seconds() * missile.turn_rate);

    // Simplified version of the above that just makes the missile look at the target:
    //    missile_transform.look_to(missile.acceleration.normalize(), Vec3::Y);

}


/* A system that displays the events. */
#[allow(unused_mut)]
pub fn handle_collision_events(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>, 
    mut collision_events: EventReader<CollisionEvent>,
    mut missiles: Query<(Entity, &mut ExternalForce, &mut Transform, &mut Collider, &mut Missile)>, 
    all_targets: Query<(Entity, &Transform), (With<Targetable>, Without<Missile>)>,
) {
    for collision_event in collision_events.read() {
        println!("Received collision event: {:?}", collision_event);
        match collision_event {
            CollisionEvent::Started(entity1, entity2, _) => {
                handle_collision_entity(&missiles, entity1, &all_targets, &mut commands, &mut meshes, &mut materials, &asset_server);
                handle_collision_entity(&missiles, entity2, &all_targets, &mut commands, &mut meshes, &mut materials, &asset_server);
            },
            CollisionEvent::Stopped(_, _, _) => {
                // Do nothing
            }
        }    }
}

fn handle_collision_entity(
    missiles: &Query<'_, '_, (Entity, &mut ExternalForce, &mut Transform, &mut Collider, &mut Missile)>, 
    entity: &Entity, 
    all_targets: &Query<'_, '_, (Entity, &Transform), (With<Targetable>, Without<Missile>)>, 
    commands: &mut Commands<'_, '_>, 
    meshes: &mut ResMut<'_, Assets<Mesh>>, 
    materials: &mut ResMut<'_, Assets<StandardMaterial>>,
    asset_server: &Res<AssetServer>, 
) {
    if missiles.get_component::<Missile>(*entity).is_ok() {
        let missile_transform_result = missiles.get(*entity);
        match missile_transform_result {
            Ok(t) => { 
                info!("Missile contact detonation");
                let missile_transform = *t.2;
                let missile = t.4;
                for (prox_target_entity, prox_target_transform) in all_targets.iter() {
                    let prox_target_distance = (prox_target_transform.translation - missile_transform.translation).length();
                    //Damage all targets within proximity fuse distance
                    if prox_target_distance < missile.proximity_fuse_distance {
                        commands.entity(prox_target_entity).despawn_recursive();
                        let position = prox_target_transform.translation;
                        spawn_explosion(commands, meshes, materials, ExplosionType::SMALL, &position);
                    }
                }
                    },
            Err(e) => info!("Collision handling error: {}", e),
        }
        //TODO: Missile explosion effect in case of terrain hit
        commands.spawn(AudioBundle {
            source: asset_server.load("sounds/xplgmn2.ogg"),
            ..default()
        });    

        commands.entity(*entity).despawn_recursive();
        }
}