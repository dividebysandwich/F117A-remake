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
/// Points of a planned route at most.
pub const ROUTE_POINTS: usize = 16;
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
/// Insertion helicopters cover this many map units a second on average, take-off and landing
/// included (~310 km/h; for the intercepts' arrival times).
pub const HELO_SPEED: f64 = 5.5;
/// Insertions and raids still in the air this long after take-off are called home.
pub const INSERTION_TIMEOUT: f64 = 90.0 * 60.0;
pub const RAID_TIMEOUT: f64 = 60.0 * 60.0;
/// The campaign sends an insertion on its own when a front place's threat is at most this, and
/// not more often than this.
pub const AUTO_INSERT_THREAT: i32 = 0;
pub const AUTO_INSERT_EVERY: f64 = 20.0 * 60.0;
/// Insertions fly from bases at most this far from the target (helicopter range, 300 km: every
/// theater's enemy airfields are in reach of one another's).
pub const INSERTION_RANGE: i32 = 0x4B00;
/// Weight of a live SAM site against one AAA unit in the threat estimate.
pub const SAM_THREAT: i32 = 3;
/// The enemy weighs a place where its insertions failed lately (this long) as if each failure
/// were one more SAM site, and picks among this many of the weakest.
pub const FAILED_MEMORY: f64 = 2.0 * 3600.0;
pub const ENEMY_TARGET_CHOICE: usize = 2;

/// Enemy air: a patrol takes off this often from a live airfield, at most this many fly
/// (plus one per ROE level); interceptors scramble against insertions this close to a base.
pub const PATROL_EVERY: f64 = 6.0 * 60.0;
pub const PATROLS: usize = 2;
pub const SCRAMBLE_RANGE: i32 = 0x1400;

/// Friendly air: a CAP of one or two fighters scrambles from an airfield when enemy aircraft
/// come this close to it (40 km, the bubble its fighters engage in:
/// `stealth_sim::campaign::CAP_BUBBLE`) and orbits this far out towards them (8 km); after a
/// CAP fighter is shot down that airfield sends none for this long.
pub const AI_CAP_TRIGGER: i32 = 0xA00;
pub const AI_CAP_STATION: i32 = 0x200;
pub const AI_CAP_RESPAWN: f64 = 5.0 * 60.0;
/// Patriot batteries defend friendly airfields at least this far behind the front (60 km), at
/// most this many (plus one per ROE level), standing this far from the airfield (4-6 km).
pub const PATRIOT_REAR: i32 = 0xF00;
pub const PATRIOT_BATTERIES: usize = 2;
pub const PATRIOT_OFFSET: i32 = 0x100;

/// The enemy's offensive, by the opponents' skill (0..3): an insertion against the weakest
/// friendly place on its front this often, announced this long before the helicopters lift
/// off (time to scramble an intercept).
pub const ENEMY_INSERT_EVERY: [f64; 4] = [75.0 * 60.0, 50.0 * 60.0, 35.0 * 60.0, 25.0 * 60.0];
pub const ENEMY_INSERTION_LEAD: f64 = 300.0;
/// Enemy raids on friendly airfields, by the opponents' skill: this often, this many strike
/// aircraft, announced this long before they take off, from a base at most this far from the
/// target (300 km).
pub const RAID_EVERY: [f64; 4] = [60.0 * 60.0, 40.0 * 60.0, 28.0 * 60.0, 18.0 * 60.0];
pub const RAID_AIRCRAFT: usize = 2;
/// Fighters escorting a raid: they take off this long before the strike aircraft and draw
/// the defenders' CAP.
pub const RAID_ESCORTS: usize = 2;
pub const RAID_ESCORT_LEAD: f64 = 90.0;
pub const RAID_LEAD: f64 = 120.0;
pub const RAID_RANGE: i32 = 0x4B00;
/// Finished raids are kept this long (the log and the debriefs have them).
pub const RAID_KEEP: f64 = 3600.0;

/// Guns and MANPADS teams of a place: one destroyed comes back this long after the last one
/// came back (or was lost).
pub const REPAIR_GUN: f64 = 20.0 * 60.0;

/// The campaign is over: the result stays on the screen this long before a dedicated server
/// starts the next one.
pub const END_SCREEN_SECONDS: f64 = 120.0;

/// Mission success: share of the defences destroyed (SEAD, CAS), time on station or kills
/// (CAP), within this radius of the CAP point (20 km).
pub const SUPPRESS_SHARE: f32 = 0.5;
pub const CAP_SECONDS: f64 = 600.0;
pub const CAP_KILLS: u32 = 2;
pub const CAP_RADIUS: i32 = 0x500;
/// Escorts, intercepts, airfield defences and rescues need the pilot there: this long airborne
/// within this radius (20 km) of the helicopters, raiders or pilot while it goes on (an air
/// kill counts too).
pub const PRESENCE_SECONDS: f64 = 120.0;
pub const PRESENCE_RADIUS: i32 = 0x500;

/// Campaign log entries kept.
pub const LOG_SIZE: usize = 200;

/// War materiel: what a side starts with and holds at most; what it gets an hour (a base
/// amount, for every place it holds, for every intact city of its places).
pub const MATERIEL_START: f64 = 150.0;
pub const MATERIEL_MAX: f64 = 400.0;
pub const INCOME_BASE: f64 = 20.0;
pub const INCOME_PLACE: f64 = 2.0;
pub const INCOME_CITY: f64 = 3.0;
/// A destroyed city costs its side this much at once (stores and factories gone).
pub const CITY_LOSS: f64 = 20.0;
/// What the AI pays: repairs by kind of site, a gun or MANPADS team, a fighter sortie (patrol,
/// CAP), a raid, an insertion the campaign orders itself (the players' orders are free).
pub const REPAIR_COST_AIRFIELD: f64 = 25.0;
pub const REPAIR_COST_SAM: f64 = 15.0;
pub const REPAIR_COST_CITY: f64 = 10.0;
pub const REPAIR_COST_SHIP: f64 = 20.0;
pub const GUN_COST: f64 = 2.0;
pub const SORTIE_COST: f64 = 2.0;
pub const RAID_COST: f64 = 15.0;
pub const INSERTION_COST: f64 = 20.0;

/// The AI's aircraft on the ground: each side starts with these fighters (the enemy more by
/// its skill) and helicopters, holds at most twice as many, and buys some every hour while
/// its materiel lasts. Taking an airfield brings its captor some of each.
pub const FIGHTERS_START: u32 = 16;
pub const ENEMY_FIGHTERS_PER_SKILL: u32 = 4;
pub const HELICOPTERS_START: u32 = 12;
pub const RESUPPLY_EVERY: f64 = 3600.0;
pub const RESUPPLY_FIGHTERS: u32 = 6;
pub const RESUPPLY_HELICOPTERS: u32 = 4;
/// The enemy buys this many an hour by the opponents' skill (the friendly side the above).
pub const ENEMY_RESUPPLY_FIGHTERS: [u32; 4] = [4, 6, 8, 10];
pub const ENEMY_RESUPPLY_HELICOPTERS: [u32; 4] = [3, 4, 6, 8];
/// Materiel above this is spent on aircraft (up to the limits) at the hourly purchase too.
pub const MATERIEL_RESERVE: f64 = 200.0;
pub const FIGHTER_COST: f64 = 15.0;
pub const HELICOPTER_COST: f64 = 4.0;
pub const CAPTURED_FIGHTERS: u32 = 2;
pub const CAPTURED_HELICOPTERS: u32 = 2;
/// Helicopters a player's insertion order always gets, whatever is left (the command keeps a
/// reserve for the players).
pub const RESERVE_HELICOPTERS: usize = 2;

/// Deep strike: enemy places off the front within this range of a friendly base (350 km) get
/// strike offers on their cities and airfields, this many of them.
pub const DEEP_RANGE: i32 = 0x5780;
pub const DEEP_TARGETS: usize = 2;

/// A pilot ejected over enemy territory waits this long for a rescue; it takes a player this
/// long airborne within this radius of him (5 km) to cover the helicopter in. Without one the
/// campaign's own helicopter gets him out at this chance; a captured pilot costs the friendly
/// side this much materiel and a fighter.
pub const RESCUE_WINDOW: f64 = 20.0 * 60.0;
pub const RESCUE_SECONDS: f64 = 120.0;
pub const RESCUE_RADIUS: i32 = 0x140;
pub const RESCUE_AI_CHANCE: i32 = 40;
pub const CAPTURE_COST: f64 = 30.0;

/// Flown in the helicopter: an insertion's troops get out in this many seconds landed within
/// `CAPTURE_RADIUS` of the target; a downed pilot boards in this many seconds landed within
/// this radius of him (about 200 m), and is home landed this near a friendly base.
pub const UNLOAD_SECONDS: f64 = 15.0;
pub const BOARD_SECONDS: f64 = 30.0;
pub const BOARD_RADIUS: i32 = 13;
pub const HOME_RADIUS: i32 = 0x40;

/// Points: each aircraft and ground kill, an aircraft brought home (on top of the objective's,
/// see `MissionKind::points`).
pub const POINTS_AIR_KILL: i32 = 50;
pub const POINTS_GROUND_KILL: i32 = 20;
pub const POINTS_LANDED: i32 = 25;
/// A package: objectives of two or more kinds achieved at one place within this time; each
/// sortie of it earns this bonus (once).
pub const PACKAGE_WINDOW: f64 = 15.0 * 60.0;
pub const PACKAGE_BONUS: i32 = 100;
/// AI SEAD support a player asks for: what it costs the friendly side, and how soon its strike
/// aircraft take off.
pub const SUPPORT_COST: f64 = 15.0;
pub const SUPPORT_LEAD: f64 = 60.0;

/// The enemy's AWACS: it goes up this often (when none is up) for this long, orbiting this far
/// behind its front (80 km); it costs this much to fly, and this much to lose.
pub const AWACS_EVERY: f64 = 90.0 * 60.0;
pub const AWACS_ENDURANCE: f64 = 60.0 * 60.0;
pub const AWACS_STANDOFF: i32 = 0x1400;
pub const AWACS_COST: f64 = 20.0;
pub const AWACS_LOSS: f64 = 60.0;

/// Enemy forward batteries: when no target on the board is within this of a player base
/// (50 km, a short helicopter flight), the enemy pushes a battery of guns forward this far from
/// the base nearest its front (15 to 35 km), on land and clear of every place, site and player
/// base (8 km). It looks into it this often, keeps this many at most, and pays this much for
/// one; the battery's guns stand within this radius of it (1.5 km).
pub const BATTERY_REACH: i32 = 0xC80;
pub const BATTERY_NEAR: i32 = 0x3C0;
pub const BATTERY_FAR: i32 = 0x8C0;
pub const BATTERY_CLEAR: i32 = 0x200;
pub const BATTERY_EVERY: f64 = 10.0 * 60.0;
pub const BATTERIES: usize = 2;
pub const BATTERY_COST: f64 = 6.0;
pub const BATTERY_RADIUS: i32 = 0x60;

/// Drone waves (the Ukraine theater): this often by the opponents' skill, this many drones,
/// announced this long before launch, at a target this far from their base at most (300 km),
/// at this cost.
pub const DRONES_EVERY: [f64; 4] = [80.0 * 60.0, 55.0 * 60.0, 40.0 * 60.0, 30.0 * 60.0];
pub const DRONE_WAVE: usize = 6;
pub const DRONE_LEAD: f64 = 180.0;
pub const DRONE_RANGE: i32 = 0x4B00;
pub const DRONE_COST: f64 = 12.0;
