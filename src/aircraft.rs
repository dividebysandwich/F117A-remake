use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use std::collections::HashMap;
use lazy_static::lazy_static;
use bevy_prototype_debug_lines::DebugLines;

use crate::player::*;

#[derive(Debug, Eq, PartialEq, Hash)]
pub enum AircraftType {
    F117A,
    MIG29
}

lazy_static! {
    pub static ref MAXFORCES_ROLL : HashMap<AircraftType, f32>= {
        let mut map = HashMap::new();
        map.insert(AircraftType::F117A, 45.0);
        map.insert(AircraftType::MIG29, 47.0);
        map
    };
}

lazy_static! {
    pub static ref MAXFORCES_PITCH : HashMap<AircraftType, f32>= {
        let mut map = HashMap::new();
        map.insert(AircraftType::F117A, 45.0);
        map.insert(AircraftType::MIG29, 47.0);
        map
    };
}

lazy_static! {
    pub static ref MAXFORCES_YAW : HashMap<AircraftType, f32>= {
        let mut map = HashMap::new();
        map.insert(AircraftType::F117A, 35.0);
        map.insert(AircraftType::MIG29, 37.0);
        map
    };
}

lazy_static! {
    pub static ref MAXTHRUST : HashMap<AircraftType, f32>= {
        let mut map = HashMap::new();
        map.insert(AircraftType::F117A, 60.0);
        map.insert(AircraftType::MIG29, 60.0);
        map
    };
}


// The lift factor of the aircraft
lazy_static! {
    pub static ref LIFT : HashMap<AircraftType, f32>= {
        let mut map = HashMap::new();
        map.insert(AircraftType::F117A, 8.0);
        map.insert(AircraftType::MIG29, 8.0);
        map
    };
}



#[derive(Component)]
pub struct Aircraft {
    /// Callsign of the aircraft
    pub name: String,
    /// The type of aircraft from the AircraftType enum
    pub aircraft_type: AircraftType,
    /// Fuel amount in lbs
    pub fuel: f32,
    // Aircraft health
    pub health: f32, 
    /// Current throttle position from 0.0 .. 1.0
    pub throttle: f32, 
    /// Currently applied thrust
    pub thrust_force: f32, 
    /// World speed, calculated from physics
    pub speed: f32,
    /// Speed in knots
    pub speed_knots: f32,
    /// The calculated altitude
    pub altitude : f32,
    /// Currently applied roll force
    pub roll_force: f32, 
    /// Currently applied yaw force
    pub yaw_force: f32, 
    /// Currently applied pitch force
    pub pitch_force: f32,
}

impl Default for Aircraft {
    fn default() -> Self {
        Aircraft {
            name: String::from("Default"),
            aircraft_type: AircraftType::F117A,
            fuel: 20000.0,
            health: 100.0,
            throttle: 0.0,
            thrust_force: 0.0,
            speed: 0.0,
            speed_knots: 0.0,
            altitude: 0.0,
            roll_force: 0.0,
            yaw_force: 0.0,
            pitch_force: 0.0
        }
    }
}

//pub fn update_aircraft_forces(mut external_forces: Query<&mut ExternalForce, With<Aircraft>>, mut velocities: Query<&mut Velocity, With<Aircraft>>,  mut transform: Query<&mut Transform, With<Aircraft>>, mut aircraft: Query<&mut Aircraft, With<Aircraft>>) {
pub fn update_aircraft_forces(mut query: Query<(&mut ExternalForce, &Velocity, &Transform, &mut Aircraft)>, time: Res<Time>, mut debug_lines: ResMut<DebugLines>,) {
    for (mut external_force, velocity, transform, mut aircraft) in query.iter_mut() {
    
        aircraft.altitude = transform.translation.y * 10.0;
        aircraft.speed = velocity.linvel.length();
        aircraft.speed_knots = aircraft.speed * 10.0;

        if aircraft.thrust_force < aircraft.throttle * MAXTHRUST.get(&aircraft.aircraft_type).unwrap() {
            aircraft.thrust_force += 20.0 * time.delta_seconds();
        } else if aircraft.thrust_force > aircraft.throttle * MAXTHRUST.get(&aircraft.aircraft_type).unwrap() {
            aircraft.thrust_force -= 20.0 * time.delta_seconds();
        }
        let mut lift_force = *LIFT.get(&aircraft.aircraft_type).unwrap() * aircraft.speed;
        if lift_force > 105.0 {
            lift_force = 105.0;
        }
        let object_rotation = transform.rotation;
        let gravity_force = 100.0;
        lift_force = lift_force - gravity_force;
//        info!("Thrustforce: {} Speed: {} Lift: {}", aircraft.thrust_force, aircraft.speed, lift_force);
    
        let force_vector = Vec3::new(aircraft.thrust_force, lift_force / 10.0, 0.0);
        let rotated_force_vector = Quat::mul_vec3(object_rotation, force_vector) + Vec3::new(0.0, lift_force, 0.0);
        external_force.force = rotated_force_vector;
        let torque_vector = Vec3::new(aircraft.roll_force, aircraft.yaw_force, aircraft.pitch_force);
        let rotated_torque_vector = Quat::mul_vec3(object_rotation, torque_vector);
        external_force.torque = rotated_torque_vector;

        debug_lines.line_colored(transform.translation,transform.translation + (rotated_force_vector / 100.0),0.0, Color::RED);
    }
}

pub fn update_player_aircraft_controls(mut aircrafts: Query<&mut Aircraft, (With<Aircraft>, With<Player>)>, input: Res<Input<KeyCode>>, time: Res<Time>) {
    for mut aircraft in aircrafts.iter_mut() {
        // Throttle
//        info!("Throttle: {}", aircraft.throttle);
        if input.pressed(KeyCode::W) {
            if aircraft.throttle < 1.0 {
                aircraft.throttle += 0.4 * time.delta_seconds();
            }
        }
        if input.pressed(KeyCode::S) {
            if aircraft.throttle > 0.0 {
                aircraft.throttle -= 0.4 * time.delta_seconds();
            }
        }
        //Pitch
        if input.pressed(KeyCode::Up) {
            if aircraft.pitch_force > -*MAXFORCES_PITCH.get(&aircraft.aircraft_type).unwrap() {
                aircraft.pitch_force -= 8.0 * time.delta_seconds();
            }
        } else if input.pressed(KeyCode::Down) {
            if aircraft.pitch_force < *MAXFORCES_PITCH.get(&aircraft.aircraft_type).unwrap() {
                aircraft.pitch_force += 8.0 * time.delta_seconds();
            }
        } else {
            //TODO: Slew to 0 instead of hard reset
            aircraft.pitch_force = 0.0;
        }
        //Roll
        if input.pressed(KeyCode::Left) {
            if aircraft.roll_force > -*MAXFORCES_ROLL.get(&aircraft.aircraft_type).unwrap() {
                aircraft.roll_force -= 8.0 * time.delta_seconds();
            }
        } else if input.pressed(KeyCode::Right) {
            if aircraft.roll_force < *MAXFORCES_ROLL.get(&aircraft.aircraft_type).unwrap() {
                aircraft.roll_force += 8.0 * time.delta_seconds();
            }
        } else {
            //TODO: Slew to 0 instead of hard reset
            aircraft.roll_force = 0.0;
        }
        //Yaw
        if input.pressed(KeyCode::D) {
            if aircraft.yaw_force > -*MAXFORCES_YAW.get(&aircraft.aircraft_type).unwrap() {
                aircraft.yaw_force -= 8.0 * time.delta_seconds();
            }
        } else if input.pressed(KeyCode::A) {
            if aircraft.yaw_force < *MAXFORCES_YAW.get(&aircraft.aircraft_type).unwrap() {
                aircraft.yaw_force += 8.0 * time.delta_seconds();
            }
        } else {
            //TODO: Slew to 0 instead of hard reset
            aircraft.yaw_force = 0.0;
        }


    }
}
