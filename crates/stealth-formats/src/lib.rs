pub mod font;
pub mod terrain;
pub mod wld;

/// Theater codes in the order the game uses for file names.
pub const THEATERS: [&str; 9] = ["LB", "PG", "NC", "CE", "VN", "ME", "KU", "CU", "KO"];

/// All theater codes: the original nine, then the theaters made for this remake (their data
/// exists only in the shipped assets).
pub const ALL_THEATERS: [&str; 10] = ["LB", "PG", "NC", "CE", "VN", "ME", "KU", "CU", "KO", "UA"];
