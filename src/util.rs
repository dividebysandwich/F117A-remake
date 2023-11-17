use std::time::SystemTime;
use rand::Rng;

use bevy::math::Vec3;

///This function returns the current time in milliseconds
/// 
///Usage: 
///let mut _current_time : u64 = 0;
///match get_time_millis() {
///    Ok(t) => _current_time = t,
///    Err(e) => {
///        println!("Error: {}", e);
///        process::exit(0);
///    }
///}
#[allow(dead_code)]
pub fn get_time_millis() -> u64 {

    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => return n.as_secs() * 1000 + n.subsec_millis() as u64,
        Err(_) => return 0 as u64,
    }
}

///This returns an monotonously increasing serial number
#[allow(dead_code)]
pub fn get_serial_number() -> u64 {
    // Define a static variable to store the state
    static mut COUNTER: u64 = 0;

    // Increment the counter by 1
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

pub fn random_vec3(range:f32) -> Vec3 {
    let mut rng = rand::thread_rng();
    Vec3::new(
        rng.gen_range(-range..range),
        rng.gen_range(-range..range),
        rng.gen_range(-range..range),
    )
}

pub fn random_u64(min: u64, range:u64) -> u64 {
    let mut rng = rand::thread_rng();
    rng.gen_range(min..range)
}

pub fn random_f32(min: f32, range:f32) -> f32 {
    let mut rng = rand::thread_rng();
    rng.gen_range(min..range)
}
