use serde::{Deserialize, Serialize};

pub const TABLE: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldObject {
    /// Name index.
    pub name: u16,
    /// Map units (world = map << 5; map y grows southward).
    pub x: u16,
    pub y: u16,
    pub unit_type: i16,
    /// 0x100 airbase, 0x200 large, 0x400 waypoint, 0x800 disabled.
    pub target_flags: i16,
    pub occupant_type: i16,
    pub patrol_count: i16,
    pub object_idx: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub ground_unit_count: u16,
    pub world_object_count: u16,
    pub objects: Vec<WorldObject>,
    /// Per shape; low nibble = weapon suitability class.
    pub shape_class: Vec<u8>,
    pub object_types: Vec<u8>,
    pub names: Vec<String>,
}

impl World {
    pub fn name(&self, i: u16) -> &str {
        self.names.get(i as usize).map_or("", String::as_str)
    }
}
