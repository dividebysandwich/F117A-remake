use anyhow::{Context, Result};
use f117_formats::{GameDir, lzexe};
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

/// Read-only view on DGROUP, padded with zeros like the Python reference.
struct Ds(Vec<u8>);

impl Ds {
    fn b(&self, o: usize) -> u8 {
        self.0.get(o).copied().unwrap_or(0)
    }
    fn w(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.b(o), self.b(o + 1)])
    }
    fn sw(&self, o: usize) -> i16 {
        self.w(o) as i16
    }
    /// NUL terminated latin-1 string.
    fn s(&self, o: usize) -> String {
        self.0.get(o..).unwrap_or(&[]).iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
    }
}

impl StartTables {
    /// Unpack START.EXE from the game directory and read the tables.
    pub fn load(game: &GameDir) -> Result<Self> {
        let exe = game.read("START.EXE")?;
        let u = lzexe::unpack(&exe).context("unpacking START.EXE")?;
        Self::from_image(&u.image)
    }

    /// Read the tables from an unpacked START image (load module at base 0).
    pub fn from_image(image: &[u8]) -> Result<Self> {
        anyhow::ensure!(image.len() > DS_BASE + 0x2000, "START image too small ({} bytes)", image.len());
        let d = Ds(image[DS_BASE..].to_vec());
        let missions = (0..MISSION_ROWS)
            .map(|r| {
                let c = |i: usize| d.sw(0x11DA + 12 * r + 2 * i);
                MissionRow {
                    theater_mask: c(0),
                    roe_mask: c(1),
                    mission_type: c(2),
                    category: c(3),
                    code: c(4),
                    special: c(5),
                }
            })
            .collect();
        let arr9 = |base: usize| -> [i16; 9] { std::array::from_fn(|i| d.sw(base + 2 * i)) };
        Ok(Self {
            missions,
            mission_text: (0..MISSION_ROWS).map(|r| d.s(d.w(0x1F24 + 2 * r) as usize)).collect(),
            weapon_qty: (0..20).map(|i| d.b(0x1EC6 + i)).collect(),
            unit_remap: (0..16).map(|i| (d.sw(0x1EE4 + 4 * i), d.sw(0x1EE6 + 4 * i))).collect(),
            planes: (0..29)
                .map(|i| PlaneInfo {
                    name: d.s(0x1708 + 32 * i),
                    nick: d.s(0x1708 + 32 * i + 7),
                    speed: d.sw(0x171A + 32 * i),
                    range: d.sw(0x171C + 32 * i),
                })
                .collect(),
            suit: (0..16).map(|w| std::array::from_fn(|c| d.b(0x1D86 + 16 * w + c) as i8)).collect(),
            avail_map: std::array::from_fn(|i| d.b(0x1C4 + i)),
            weapon_names: (0..19).map(|i| d.s(d.w(0xA409 + 2 * i) as usize)).collect(),
            roe_text: (0..3).map(|r| d.s(d.w(0x9781 + 2 * r) as usize)).collect(),
            dir_dx: arr9(0x105A),
            dir_dy: arr9(0x106C),
            dir_off: [d.sw(0x1052), d.sw(0x1054), d.sw(0x1056)],
            grid_size: std::array::from_fn(|i| d.sw(0x107E + 2 * i)),
            date_month: std::array::from_fn(|i| d.b(0x882 + i)),
            date_day: std::array::from_fn(|i| d.b(0x88C + i)),
            date_year: std::array::from_fn(|i| d.w(0x896 + 2 * i)),
            month_names: (0..13).map(|i| d.s(d.w(0x8A8 + 2 * i) as usize)).collect(),
            month_len: std::array::from_fn(|i| d.b(0x8C2 + i)),
            dot: d.s(0x949),
        })
    }

    /// Plane table entry, or a zero entry for an out-of-range type.
    pub fn plane(&self, t: i16) -> PlaneInfo {
        self.planes.get(t as usize).cloned().unwrap_or(PlaneInfo { name: String::new(), nick: String::new(), speed: 0, range: 0 })
    }

    pub fn weapon_name(&self, w: i16) -> &str {
        self.weapon_names.get(w as usize).map_or("", String::as_str)
    }
}
