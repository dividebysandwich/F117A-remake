use bevy::color::Color;


pub const COLLISION_MASK_TERRAIN: u32 = 0b100000;
pub const COLLISION_MASK_AIRCRAFT: u32 = 0b010000;
pub const COLLISION_MASK_GROUNDVEHICLE: u32 = 0b001000;
pub const COLLISION_MASK_MISSILE: u32 = 0b000100;
pub const COLLISION_MASK_PLAYER: u32 = 0b000010;
pub const COLLISION_MASK_EFFECT: u32 = 0b000001;
#[allow(dead_code)]
pub const RENDERLAYER_WORLD: usize = 0;
#[allow(dead_code)]
pub const RENDERLAYER_COCKPIT: usize = 1;
#[allow(dead_code)]
pub const RENDERLAYER_MFD: usize = 2;
#[allow(dead_code)]
pub const RENDERLAYER_AIRCRAFT: usize = 3;
#[allow(dead_code)]
pub const RENDERLAYER_POINTLIGHTS: usize = 4;

pub const RADAR_PULSE_TIMEOUT: u64 = 300;

pub const COLOR_GREEN: Color = Color::srgb(0., 1., 0.);
pub const COLOR_YELLOW: Color = Color::srgb(1., 1., 0.);
pub const COLOR_ORANGE_RED: Color = Color::srgb(1., 0.8, 0.2);