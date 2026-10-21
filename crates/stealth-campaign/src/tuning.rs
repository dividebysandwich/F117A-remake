//! Every number that sets the pace of a campaign, in one place. Distances are map units
//! (15.625 m), times campaign seconds (real time while the game runs).

/// Cities and airfields of the same name this close are one place (8 km).
pub const PLACE_MERGE: i32 = 0x200;
/// A SAM site or radar belongs to the nearest place this close (12 km).
pub const SITE_ASSOCIATION: i32 = 0x300;
/// Enemy places this close to a friendly one are on the front (150 km).
pub const FRONT_RANGE: i32 = 0x2600;
/// Front places that get a full set of offers, nearest first.
pub const FRONT_PLACES: usize = 3;
/// Offers on the board at most.
pub const BOARD_SIZE: usize = 12;
/// The board is rebuilt this often (and after every capture).
pub const BOARD_REFRESH: f64 = 60.0;
/// Enemy ships this close to a friendly place are anti-ship targets (120 km).
pub const SHIP_RANGE: i32 = 0x1E00;
/// AAA within this distance of a place defends it (CAS targets, insertion threat; 6 km).
pub const AAA_RANGE: i32 = 0x180;
/// Enemy flights this close to a friendly place or an insertion call for CAP (60 km).
pub const CAP_TRIGGER: i32 = 0xF00;

/// Recon: intelligence stays valid this long; the defences' detection range with it, and the
/// player's spotting range.
pub const INTEL_SECONDS: f64 = 3600.0;
pub const INTEL_DETECTION: f32 = 0.7;
pub const INTEL_SPOTTING: f32 = 2.0;

/// The enemy repairs destroyed sites and airfields after this long.
pub const REPAIR_SITE: f64 = 45.0 * 60.0;
pub const REPAIR_AIRFIELD: f64 = 60.0 * 60.0;
/// Insertions: helicopters per insertion, minutes from the order until they lift off (time to
/// take off for an escort), landing this close to the place takes it (1.5 km).
pub const INSERTION_HELOS: usize = 4;
pub const INSERTION_LEAD: f64 = 180.0;
pub const CAPTURE_RADIUS: i32 = 0x60;
/// The campaign sends an insertion on its own when a front place's threat is at most this, and
/// not more often than this.
pub const AUTO_INSERT_THREAT: i32 = 0;
pub const AUTO_INSERT_EVERY: f64 = 20.0 * 60.0;
/// Insertions fly from bases at most this far from the target (helicopter range, 250 km).
pub const INSERTION_RANGE: i32 = 0x3E80;
/// Weight of a live SAM site against one AAA unit in the threat estimate.
pub const SAM_THREAT: i32 = 3;

/// Enemy air: a patrol takes off this often from a live airfield, at most this many fly
/// (plus one per ROE level); interceptors scramble against insertions this close to a base.
pub const PATROL_EVERY: f64 = 6.0 * 60.0;
pub const PATROLS: usize = 2;
pub const SCRAMBLE_RANGE: i32 = 0x1400;

/// Mission success: share of the defences destroyed (SEAD, CAS), time on station or kills
/// (CAP), within this radius of the CAP point (20 km).
pub const SUPPRESS_SHARE: f32 = 0.5;
pub const CAP_SECONDS: f64 = 600.0;
pub const CAP_KILLS: u32 = 2;
pub const CAP_RADIUS: i32 = 0x500;

/// Campaign log entries kept.
pub const LOG_SIZE: usize = 200;
