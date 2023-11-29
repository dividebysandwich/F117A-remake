
pub const COLLISION_MASK_TERRAIN: u32 = 0b100000;
pub const COLLISION_MASK_AIRCRAFT: u32 = 0b010000;
pub const COLLISION_MASK_GROUNDVEHICLE: u32 = 0b001000;
pub const COLLISION_MASK_MISSILE: u32 = 0b000100;
pub const COLLISION_MASK_PLAYER: u32 = 0b000010;
pub const COLLISION_MASK_EFFECT: u32 = 0b000001;
#[allow(dead_code)]
pub const RENDERLAYER_WORLD: u8 = 0;
#[allow(dead_code)]
pub const RENDERLAYER_COCKPIT: u8 = 1;
#[allow(dead_code)]
pub const RENDERLAYER_MFD: u8 = 2;
#[allow(dead_code)]
pub const RENDERLAYER_AIRCRAFT: u8 = 3;
#[allow(dead_code)]
pub const RENDERLAYER_POINTLIGHTS: u8 = 4;

pub const RADAR_PULSE_TIMEOUT: u64 = 300;