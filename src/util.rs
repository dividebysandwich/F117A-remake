use std::time::SystemTime;

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