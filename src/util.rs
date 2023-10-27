use std::time::{SystemTime, UNIX_EPOCH};

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
pub fn get_time_millis() -> Result<u64, String>{
    let current_time = SystemTime::now();

    // Calculate the time in milliseconds since the Unix epoch
    let since_epoch = current_time.duration_since(UNIX_EPOCH).expect("System date can't be that far in the past!");
    let milliseconds = since_epoch.as_secs() * 1000 + since_epoch.subsec_millis() as u64;

    Ok (milliseconds)
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