use serde::{Deserialize, Serialize};

/// DGROUP offset in the unpacked START image.
pub const DS_BASE: usize = 0xA950;

/// Number of rows in the mission table (DS:11DA).
pub const MISSION_ROWS: usize = 82;

/// One row of the mission table (spec §5.4).
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

/// Plane table entry (DS:1708, 32-byte records, spec §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaneInfo {
    pub name: String,
    pub nick: String,
    pub speed: i16,
    pub range: i16,
}

/// All START.EXE tables used by the generator and the briefing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartTables {
    pub missions: Vec<MissionRow>,
    /// Briefing paragraph per mission row (pointer table DS:1F24).
    pub mission_text: Vec<String>,
    /// Quantity per weapon type (DS:1EC6).
    pub weapon_qty: Vec<u8>,
    /// Threat (up, down) remap per unit type 0..15 (DS:1EE4).
    pub unit_remap: Vec<(i16, i16)>,
    pub planes: Vec<PlaneInfo>,
    /// Weapon suitability `suit[weapon][class]` (DS:1D86, 16x16 signed bytes).
    pub suit: Vec<[i8; 16]>,
    /// Weapon availability index map (DS:01C4).
    pub avail_map: [u8; 16],
    /// Weapon names (pointer table DS:A409).
    pub weapon_names: Vec<String>,
    /// ROE paragraphs (pointer table DS:9781).
    pub roe_text: Vec<String>,
    /// Tile search neighbour directions (DS:105A / DS:106C).
    pub dir_dx: [i16; 9],
    pub dir_dy: [i16; 9],
    /// Tile-relative offset for a direction of -1, 0, +1 (DS:1052).
    pub dir_off: [i16; 3],
    /// Grid size per LOD level (DS:107E).
    pub grid_size: [i16; 5],
    /// Briefing date tables (DS:0882 / 088C / 0896 / 08A8 / 08C2).
    pub date_month: [u8; 9],
    pub date_day: [u8; 9],
    pub date_year: [u16; 9],
    pub month_names: Vec<String>,
    pub month_len: [u8; 14],
    /// The "." appended to objectives (DS:0949).
    pub dot: String,
}

impl StartTables {

    /// Plane table entry, or a zero entry for an out-of-range type.
    pub fn plane(&self, t: i16) -> PlaneInfo {
        self.planes.get(t as usize).cloned().unwrap_or(PlaneInfo { name: String::new(), nick: String::new(), speed: 0, range: 0 })
    }

    pub fn weapon_name(&self, w: i16) -> &str {
        self.weapon_names.get(w as usize).map_or("", String::as_str)
    }
}
