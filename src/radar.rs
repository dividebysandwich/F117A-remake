use bevy::prelude::*;

use crate::{coalition::Coalition, util::get_time_millis};

#[allow(dead_code)]
pub enum RadarEmitterType {
    PULSE,
    DOPPLER,
}

#[derive(Component)]
pub struct RadarDetectable {
    pub base_radar_cross_section: f32, // This is the basic visibility value
    pub radar_cross_section: f32, // Calculated radar visibility based on orientation, for RWR display
    pub reflected_energy: f32, // Calculated radar return energy, for RWR display
}

impl Default for RadarDetectable {
    fn default() -> Self {
         RadarDetectable {
            base_radar_cross_section: 0.2,
            radar_cross_section: 0.0,
            reflected_energy: 0.0,
         }
    }
}

#[derive(Component)]
pub struct RadarEmitter {
    pub radar_type: RadarEmitterType,
    pub radar_gain: f32, // Affects how difficult it is to hide from this radar
    pub max_detect_range_km: f32, // Maximum detection range in km
    pub scan_interval: f32, // Radar sweep interval in seconds
    pub last_scan_time: u64,
}

impl Default for RadarEmitter {
    fn default() -> Self {
         RadarEmitter {
            radar_type: RadarEmitterType::PULSE,
            radar_gain: 100.0,
            scan_interval: 3.0,
            max_detect_range_km: 100.0,
            last_scan_time: 0,
         }
    }
}

pub fn update_rcs (
    mut detectables: Query<(&mut RadarDetectable, &Transform)>,
) {
    for (mut detectable, detectable_transform) in detectables.iter_mut() {
        // Update the radar cross-section based on the pitch/roll angle of the aircraft.
        // A level flying aircraft is a stealthy aircraft
        let roll_factor = detectable_transform.rotation.x.sin().abs();
        let pitch_factor =  detectable_transform.rotation.z.sin().abs();

   		// Radar returns rise with altitude until 1000 feet, remain strong until 8000 feet, then get weaker with rising altitude (but never below 0.4f)
    	let low_altitude_curve = (detectable_transform.translation.y / 1000.0).clamp(0.0, 1.0);
   		let high_altitude_curve = 1.0 - ((detectable_transform.translation.y-8000.0).clamp(0.0, 900000.0) / 20000.0).clamp(0.4, 1.0);
	    let altitude_factor = low_altitude_curve * high_altitude_curve;

//		info!("RFactor: {} PFactor: {}", roll_factor, pitch_factor);
        detectable.radar_cross_section = (detectable.base_radar_cross_section * altitude_factor) + (roll_factor * 0.4) + (pitch_factor * 0.4);

    }
}

#[allow(unused_assignments)]
pub fn update_radar(
    mut radars: Query<(&mut RadarEmitter, &Transform, &Coalition)>,
    mut detectables: Query<(&mut RadarDetectable, &Transform, &Coalition)>,
) {
    for (mut radar_emitter, radar_transform, radar_coalition) in radars.iter_mut() {
        let milliseconds = get_time_millis();
        
        //Skip this radar if it's not time to scan yet
        if milliseconds - radar_emitter.last_scan_time < (radar_emitter.scan_interval * 1000.0) as u64 {
            continue;
        }
        radar_emitter.last_scan_time = milliseconds;
        for (mut detectable, detectable_transform, detectable_coalition) in detectables.iter_mut() {
            // Skip target if it's a friendly
            if radar_coalition.side == detectable_coalition.side {
                continue;
            }

            
		    // Calculate return signal strength based on signal strength, distance, own status, altitude and attitude
            let target_distance: f32 = (detectable_transform.translation - radar_transform.translation).length();

		    // Radar returns attenuate over distance
		    let distance_factor = (target_distance.clamp(0.0, 900000.0) / (radar_emitter.max_detect_range_km*1000.0)).clamp(0.0, 1.0);

		    let signal_strength_at_target = radar_emitter.radar_gain * distance_factor;
            
		    // if signal_strength + radar_cross_section > 1 then we are visible
		    let raw_return_signal = signal_strength_at_target + detectable.radar_cross_section;

            // Now check our orientation relative to the radar emitter, 
            // and attenuate the return signal depending on radar type and our orientation
			let mut angular_difference = (detectable_transform.translation - radar_transform.translation).angle_between(detectable_transform.forward()).to_degrees();
			if angular_difference > 90.0 {
				angular_difference = (detectable_transform.translation - radar_transform.translation).angle_between(-detectable_transform.forward()).to_degrees();
			}
			angular_difference = angular_difference / 90.0; //Make this range from 0.0 to 1.0
			
			let mut effective_gain = radar_emitter.radar_gain;

            match radar_emitter.radar_type {
                RadarEmitterType::PULSE => {
    				//For pulse radar, flying straight towards it is advisable to not be seen
				    effective_gain = radar_emitter.radar_gain * angular_difference;
                }
                RadarEmitterType::DOPPLER => {
    				//Doppler radar can't see you if you fly perpendicular to it
	    			effective_gain = radar_emitter.radar_gain * (1.0 - angular_difference);
                }
            }

            let final_return_signal = raw_return_signal * effective_gain;
            info!("Final return signal: {}", final_return_signal);
            detectable.reflected_energy = final_return_signal;

            //TODO: Update RCR/RWR indicator
            //TODO: Tracking and targeting

        }
    }

}


