use serde::{Deserialize, Serialize};

/// Number of rows in the mission table.
pub const MISSION_ROWS: usize = 82;

/// One row of the mission table: which target, mission type and flags a mission can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionRow {
    /// Column 0: bit per theater.
    pub theater_mask: i16,
    /// Column 1: 1 cold, 2 limited, 4 conventional.
    pub roe_mask: i16,
    /// Column 2: mission type 1..=8.
    pub mission_type: i16,
    /// Column 3: target category (`ott` of the target shape).
    pub category: i16,
    /// Column 4: code flags (0x02 deadline, 0x08 not both, 0x10 area, 0x20 rubble at deadline).
    pub code: i16,
    /// Column 5: > 0 special 3D model id, < 0 special plane type (negated).
    pub special: i16,
}

/// Plane table entry: performance data per aircraft type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaneInfo {
    pub name: String,
    pub nick: String,
    pub speed: i16,
    pub range: i16,
}

/// Mission generator tables (assets/mission/tables.json) used by the generator and the briefing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartTables {
    pub missions: Vec<MissionRow>,
    /// Quantity per weapon type.
    pub weapon_qty: Vec<u8>,
    /// Threat (up, down) remap per unit type 0..15.
    pub unit_remap: Vec<(i16, i16)>,
    pub planes: Vec<PlaneInfo>,
    /// Weapon suitability `suit[weapon][class]` (16x16 signed bytes).
    pub suit: Vec<[i8; 16]>,
    /// Weapon availability index map.
    pub avail_map: [u8; 16],
    /// Weapon names.
    pub weapon_names: Vec<String>,
    /// Tile search neighbour directions (dx, dy).
    pub dir_dx: [i16; 9],
    pub dir_dy: [i16; 9],
    /// Tile-relative offset for a direction of -1, 0, +1.
    pub dir_off: [i16; 3],
    /// Grid size per LOD level.
    pub grid_size: [i16; 5],
    /// Briefing date tables (month names, days per month, start dates per theater).
    pub date_month: [u8; 9],
    pub date_day: [u8; 9],
    pub date_year: [u16; 9],
    pub month_names: Vec<String>,
    pub month_len: [u8; 14],
}

impl StartTables {
    /// Plane table entry, or a zero entry for an out-of-range type.
    pub fn plane(&self, t: i16) -> PlaneInfo {
        self.planes.get(t as usize).cloned().unwrap_or(PlaneInfo {
            name: String::new(),
            nick: String::new(),
            speed: 0,
            range: 0,
        })
    }

    pub fn weapon_name(&self, w: i16) -> &str {
        self.weapon_names.get(w as usize).map_or("", String::as_str)
    }
}
