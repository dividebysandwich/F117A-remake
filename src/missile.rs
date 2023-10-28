use bevy::prelude::*;

use crate::util::*;

#[derive(Component)]
pub struct Missile {
    start_time: u64,
    launching_vehicle: Entity,
    target: Entity,
    target_position: Vec3,
    max_turn_rate: f32,
    thrust: f32, 
    max_thrust: f32,
    thrust_ramp: f32,
    turn_rate: f32,
    turn_ramp: f32,
    gain: f32,
    ignition_delay: u64,
    proximity_fuse_distance: f32,
    proximity_fuse_arm_time: u64, 
    last_target_distance: f32,
    last_position: Vec3,
    line_of_sight: Vec3,
    acceleration: Vec3,
}

impl Default for Missile {
    fn default() -> Self {
         Missile {
            start_time: get_time_millis(),
            launching_vehicle: Some(Entity),
            target: Some(Entity),
            target_position: Vec3::new(0.0, 0.0, 0.0),
            max_turn_rate: 180.0,
            thrust: 0.0,
            max_thrust: 10.0,
            thrust_ramp: 3.0,
            turn_rate: 0.0,
            turn_ramp: 3.0,
            gain: 3.0,
            ignition_delay: 1000,
            proximity_fuse_distance: 10000.0,
            proximity_fuse_arm_time: 5000,
            last_target_distance: 9999999999.9,
            last_position: Vec3::new(0.0, 0.0, 0.0),
            line_of_sight: Vec3::new(0.0, 0.0, 0.0),
            acceleration: Vec3::new(0.0, 0.0, 0.0),
         }
    }
}

fn update_missile(mut missile: Missile, time: Res<Time>, mut query: Query<(&mut Transform, &mut RigidBody, &mut Collider)>) {
		
    let current_time = get_time_millis();
    if (current_time - missile.start_time < missile.ignition_delay) {
        return;
    }

    //We may know our target, or just the coordinates
    if (missile.target) {
        missile.target_position = missile.target.transform.position;
    }/* else if (FLIRSensor) {
        targetPosition = FLIRSensor.getCurrentLaserCoordinates();
    }*/

    //Proximity fuze if we have passed the target
    let targetDistance = Vector3.Distance(transform.position, missile.target_position);

/*     if (current_time - missile.start_time > missile.proximity_fuse_arm_time) {
        if (targetDistance > missile.last_target_distance) {
            if (missile.last_target_distance < missile.proximity_fuse_distance) {
                Damageable[] damageables = (Damageable[]) GameObject.FindObjectsOfType (typeof(Damageable));
                foreach (Damageable d in damageables) {
                    let curdist = Vector3.Distance(lastPosition, d.transform.position);
                    if (curdist < missile.proximity_fuse_distance) {
                        //TODO: Simulate less than full damage depending on actual explosion distance
                        Damage(d.transform.root.gameObject);
                    }
                }
                Detonate();
            }
            return;
        }
    }
    */
    missile.last_target_distance = targetDistance;
    missile.last_position = transform.position;

    
    // Increase thrust over time
    if (missile.thrust < missile.max_thrust) {
        // don't go over in case thrustRamp is very small
        let increase = time.delta_seconds() * missile.max_thrust / missile.thrust_ramp;
        missile.thrust = Mathf.Min(missile.thrust + increase, missile.max_thrust);
    }

    // Increase turn rate over time
    if (turnRate < maxTurnRate) {
        let increase = time.delta_seconds() * missile.max_turn_rate / missile.turn_ramp;
        missile.turn_rate = Mathf.Min(missile.turn_rate + increase, missile.max_turn_rate);
    }

    // Proportional Navigation evaluates the rate of change of the Line Of Sight (los) to our target. If the rate of change is zero,
    // the missile is on a collision course. If it is not, we apply a force to correct course.
    let prevLos = missile.line_of_sight;
    missile.line_of_sight = missile.target_position - transform.position;
    let mut dLos = missile.line_of_sight - prevLos;

    // we only want the component perpendicular to the line of sight
    dLos = dLos - Vector3.Project(dLos, missile.line_of_sight);
        
    // plain PN would be:
    // acceleration = time.delta_seconds() * missile.line_of_sight + dLos * nc;

    // Augmented PN takes acceleration into account
    missile.acceleration = time.delta_seconds() * missile.line_of_sight + dLos * gain + time.delta_seconds() * missile.acceleration * gain / 2;
    // Acceleration can't be larger than the maximum thrust
    missile.acceleration = Vector3.ClampMagnitude(missile.acceleration * missile.thrust, missile.thrust);
        
    // Accelerate towards target
    body.AddForce(acceleration, ForceMode.Acceleration);

    let targetRotation = Quaternion.LookRotation(acceleration, transform.up);
    transform.rotation = Quaternion.RotateTowards(transform.rotation, targetRotation, Time.deltaTime * turnRate);
    
    // For less accurate guidance, turn entity towards it and apply forward thrust
    //body.AddForce(transform.forward * acceleration.magnitude, ForceMode.Acceleration);
}
