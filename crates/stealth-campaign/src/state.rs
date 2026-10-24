//! The campaign's state: who owns what, the mission board, the insertions in progress and the
//! players' sorties. Everything here is plain data (saved as JSON, and what a coop host would
//! send its clients).

use serde::{Deserialize, Serialize};
use stealth_mission::world::{MissionWorld, flags};

use crate::rng::Rng;
use crate::tuning::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Blue,
    Red,
    Neutral,
}

impl Side {
    pub fn enemy(self) -> Side {
        match self {
            Side::Blue => Side::Red,
            Side::Red => Side::Blue,
            Side::Neutral => Side::Neutral,
        }
    }
}

/// A player of the campaign (0 = the local player; coop players get the next ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlayerId(pub u8);

/// The aircraft a sortie is flown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Aircraft {
    #[default]
    F117,
    F16,
    UH60,
    AH1Z,
}

impl Aircraft {
    pub fn name(self) -> &'static str {
        match self {
            Aircraft::F117 => "F-117A",
            Aircraft::F16 => "F-16",
            Aircraft::UH60 => "UH-60",
            Aircraft::AH1Z => "AH-1Z",
        }
    }

    pub fn helicopter(self) -> bool {
        matches!(self, Aircraft::UH60 | Aircraft::AH1Z)
    }

    /// The troop helicopter: flies insertions and picks downed pilots up.
    pub fn transport(self) -> bool {
        self == Aircraft::UH60
    }
}

/// Object index into the theater's world objects (stable for a theater).
pub type ObjectId = u16;
/// Index into [`CampaignState::places`].
pub type PlaceId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteKind {
    Airfield,
    Carrier,
    City,
    /// SAM site or radar (a ground unit with a radar type).
    Sam,
    Ship,
}

/// One world object the campaign tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Site {
    pub object: ObjectId,
    pub name: String,
    /// Map units (x east, y south).
    pub x: i32,
    pub y: i32,
    pub kind: SiteKind,
    /// Radar type (SAM sites, ships).
    pub unit_type: i16,
    /// The place it belongs to (cities and airfields; SAM sites near one).
    pub place: Option<PlaceId>,
    /// Owner of a site without a place (ships, carriers, lone SAM sites).
    pub side: Side,
    /// Campaign time it was destroyed (None: intact).
    pub destroyed_at: Option<f64>,
    /// Removed for good (the defences of a captured place).
    pub removed: bool,
    /// A friendly aircraft has detected it (its radar on an RWR, recon photos): the briefings
    /// show it while it lives.
    #[serde(default)]
    pub spotted: bool,
}

impl Site {
    pub fn alive(&self) -> bool {
        self.destroyed_at.is_none() && !self.removed
    }
}

/// A city and its airfield (same name, close together): what the sides fight over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub owner: Side,
    /// Cities and airfields of the place.
    pub members: Vec<ObjectId>,
    pub airfield: Option<ObjectId>,
    /// SAM sites and radars defending it.
    pub defences: Vec<ObjectId>,
    /// Recon intelligence valid until then (campaign time).
    pub intel_until: Option<f64>,
    /// Its guns and MANPADS teams destroyed and not replaced yet, and when the next one is
    /// replaced.
    #[serde(default)]
    pub guns_lost: u8,
    #[serde(default)]
    pub guns_repair_at: Option<f64>,
}

impl Place {
    pub fn intel(&self, now: f64) -> bool {
        self.intel_until.is_some_and(|t| t > now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MissionKind {
    Recon,
    Sead,
    Cap,
    Cas,
    AntiShip,
    Strike,
    Escort,
    Insertion,
    /// Stop an enemy insertion.
    Intercept,
    /// Defend an airfield against an enemy raid.
    Defend,
    /// Strike the enemy's rear: a city (its war materiel) or an airfield.
    Interdiction,
    /// Cover the rescue of a pilot down in enemy territory.
    Csar,
    /// Shoot down the enemy's AWACS while it is up.
    Awacs,
}

impl MissionKind {
    pub fn name(self) -> &'static str {
        match self {
            MissionKind::Recon => "RECON",
            MissionKind::Sead => "SEAD",
            MissionKind::Cap => "CAP",
            MissionKind::Cas => "CAS",
            MissionKind::AntiShip => "ANTI-SHIP",
            MissionKind::Strike => "STRIKE",
            MissionKind::Escort => "ESCORT",
            MissionKind::Insertion => "INSERTION",
            MissionKind::Intercept => "INTERCEPT",
            MissionKind::Defend => "DEFEND",
            MissionKind::Interdiction => "INTERDICT",
            MissionKind::Csar => "CSAR",
            MissionKind::Awacs => "AWACS",
        }
    }

    /// Flown by the AI when ordered (the player may also fly it, in the helicopter).
    pub fn ai_flown(self) -> bool {
        self == MissionKind::Insertion
    }

    /// Who may fly it: insertions only the troop helicopter, which flies nothing but them and
    /// rescues (it picks the pilot up, the others cover it). The gunship flies the rest.
    pub fn flyable_by(self, aircraft: Aircraft) -> bool {
        match self {
            MissionKind::Insertion => aircraft.transport(),
            MissionKind::Csar => true,
            _ => !aircraft.transport(),
        }
    }

    /// Why an aircraft may not fly it.
    pub fn not_flyable(self, aircraft: Aircraft) -> &'static str {
        if aircraft.transport() {
            "The UH-60 flies insertions and rescues"
        } else {
            "Insertions are flown in the UH-60, or ordered for the helicopter crews"
        }
    }

    /// Points for achieving the objective.
    pub fn points(self) -> i32 {
        match self {
            MissionKind::Recon | MissionKind::Cap => 100,
            MissionKind::Cas | MissionKind::Escort => 120,
            MissionKind::Sead
            | MissionKind::Strike
            | MissionKind::AntiShip
            | MissionKind::Intercept
            | MissionKind::Defend
            | MissionKind::Csar
            | MissionKind::Insertion => 150,
            MissionKind::Interdiction => 200,
            MissionKind::Awacs => 250,
        }
    }
}

/// What a mission asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Objective {
    /// Photograph the place (its main object).
    Recon {
        place: PlaceId,
        object: ObjectId,
    },
    /// Destroy the place's SAM sites and radars.
    Sead {
        place: PlaceId,
        sites: Vec<ObjectId>,
    },
    /// Destroy the guns and launchers around the place (AAA unit indices).
    Cas {
        place: PlaceId,
        aaa: Vec<u32>,
    },
    /// Destroy the airfield (no more enemy aircraft from it until it is repaired).
    Strike {
        place: PlaceId,
        airfield: ObjectId,
    },
    AntiShip {
        ship: ObjectId,
    },
    /// Hold the air over a point.
    Cap {
        x: i32,
        y: i32,
    },
    /// Cover an insertion.
    Escort {
        insertion: u32,
    },
    /// Send helicopters with troops to take the place.
    Insertion {
        place: PlaceId,
    },
    /// Shoot down the enemy's insertion helicopters before they land.
    Intercept {
        insertion: u32,
    },
    /// Keep the enemy's strike aircraft off the airfield.
    Defend {
        raid: u32,
    },
    /// Destroy a city of the enemy's rear (its war materiel).
    Interdict {
        place: PlaceId,
        object: ObjectId,
    },
    /// Hold the air over a downed pilot while the helicopter picks him up.
    Rescue {
        downed: u32,
    },
    /// Shoot down the AWACS (campaign tag).
    Awacs {
        tag: u32,
    },
    /// Destroy an enemy forward battery's guns (AAA unit indices).
    Battery {
        battery: u32,
        aaa: Vec<u32>,
    },
}

/// A mission on the board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionOffer {
    pub id: u32,
    pub kind: MissionKind,
    pub title: String,
    pub objective: Objective,
    /// Where it happens (map units).
    pub x: i32,
    pub y: i32,
    /// Friendly base it is best flown from.
    pub base: ObjectId,
    /// Rough danger: live SAM sites (x3) and guns around the target.
    pub threat: i32,
    /// When it was posted.
    pub posted: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum InsertionState {
    /// Ordered; the helicopters lift off at this time.
    Pending {
        launch_at: f64,
    },
    /// In the air.
    Enroute,
    Captured,
    Failed,
    /// Not flown: the place changed hands before the helicopters got there.
    CalledOff,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Insertion {
    pub id: u32,
    /// The side taking the place.
    #[serde(default = "blue")]
    pub side: Side,
    pub place: PlaceId,
    /// Base the helicopters fly from.
    pub from: ObjectId,
    /// Object they land at (the airfield, else the city).
    pub target: ObjectId,
    /// Campaign tags of the helicopters (see [`CampaignEvent::InsertionLaunched`]) and which of
    /// them are still flying.
    pub helicopters: Vec<u32>,
    pub alive: Vec<bool>,
    pub state: InsertionState,
    /// Interceptors were sent against it.
    pub scrambled: bool,
    pub ordered_by: Option<PlayerId>,
    /// When the helicopters lifted off.
    #[serde(default)]
    pub enroute_at: Option<f64>,
    /// Players flying it in their helicopters (it goes on while one does).
    #[serde(default)]
    pub pilots: Vec<PlayerId>,
}

/// How a sortie ended for the pilot (who never dies in a dynamic campaign).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortieEnd {
    Landed,
    Ejected,
    Crashed,
    Aborted,
}

impl SortieEnd {
    pub fn name(self) -> &'static str {
        match self {
            SortieEnd::Landed => "Landed",
            SortieEnd::Ejected => "Ejected",
            SortieEnd::Crashed => "Shot down / crashed",
            SortieEnd::Aborted => "Aborted",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub sites_destroyed: Vec<ObjectId>,
    pub aaa_destroyed: Vec<u32>,
    pub air_kills: u32,
    /// Seconds on station (CAP).
    pub on_station: f64,
    pub photo: bool,
    /// Objective achieved (stays achieved).
    pub achieved: bool,
    /// Friendly aircraft lost in the CAP area, and enemy aircraft the pilot shot down there.
    pub losses: u32,
    #[serde(default)]
    pub cap_kills: u32,
    /// Where the player's aircraft was last seen in the air.
    #[serde(default)]
    pub last: Option<(i32, i32)>,
    /// When the objective was achieved, and the package bonus earned.
    #[serde(default)]
    pub achieved_at: Option<f64>,
    #[serde(default)]
    pub bonus: i32,
    /// Seconds airborne near what the objective is about while it was going on (escorts,
    /// intercepts, defences, rescues: only who was there earns them).
    #[serde(default)]
    pub near_secs: f64,
    /// Flown in the helicopter: the player's own insertion (its id), seconds landed where it
    /// counts (unloading the troops, the downed pilot boarding), the downed pilot aboard and
    /// brought home.
    #[serde(default)]
    pub heli: bool,
    #[serde(default)]
    pub insertion: Option<u32>,
    #[serde(default)]
    pub landed_secs: f64,
    #[serde(default)]
    pub aboard: bool,
    #[serde(default)]
    pub delivered: bool,
}

impl Progress {
    /// The pilot took part: was there long enough, or shot something down.
    pub fn present(&self) -> bool {
        self.near_secs >= PRESENCE_SECONDS || self.air_kills > 0
    }
}

/// A sortie being flown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sortie {
    pub id: u32,
    pub offer: MissionOffer,
    pub base: ObjectId,
    #[serde(default)]
    pub aircraft: Aircraft,
    pub started: f64,
    pub progress: Progress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortieResult {
    Success,
    Failed,
    /// Depends on something still going on (an escorted insertion).
    Pending,
}

/// A finished sortie (the debrief and the pilot's record).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortieRecord {
    pub id: u32,
    pub player: PlayerId,
    pub kind: MissionKind,
    pub title: String,
    pub end: SortieEnd,
    pub result: SortieResult,
    pub started: f64,
    pub ended: f64,
    pub air_kills: u32,
    pub ground_kills: u32,
    /// The escorted or intercepted insertion, the raid defended against (their result
    /// settles a pending sortie).
    pub insertion: Option<u32>,
    #[serde(default)]
    pub raid: Option<u32>,
    /// The downed pilot a rescue covered.
    #[serde(default)]
    pub downed: Option<u32>,
    #[serde(default)]
    pub aircraft: Aircraft,
    /// The pilot's name (his record stays his when another takes his slot).
    #[serde(default)]
    pub pilot: String,
    /// Points earned (the objective's once it is settled, the package bonus among them).
    #[serde(default)]
    pub points: i32,
    #[serde(default)]
    pub bonus: i32,
    /// Where the objective was and when it was achieved (packages).
    #[serde(default)]
    pub place: Option<PlaceId>,
    #[serde(default)]
    pub achieved_at: Option<f64>,
}

/// What a route point is: a navigation point a player placed (or the mission's midpoint), or
/// one of the mission's fixed points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteKind {
    Nav,
    Primary,
    Secondary,
    Land,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePoint {
    /// Map units (x east, y south).
    pub x: i32,
    pub y: i32,
    pub kind: RouteKind,
}

/// The players' shared plan for an offer of the board (coop): the base they fly from and the
/// route, who is planning it and who is flying it. The route is locked while anyone flies it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionPlan {
    pub offer: u32,
    pub base: ObjectId,
    /// Empty: the mission's own route from the base.
    pub route: Vec<RoutePoint>,
    /// In the briefing.
    pub crew: Vec<PlayerId>,
    pub flying: Vec<PlayerId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSlot {
    pub id: PlayerId,
    pub name: String,
    pub sortie: Option<Sortie>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub time: f64,
    pub text: String,
}

/// How a new campaign is set up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignSetup {
    pub theater: usize,
    pub theater_code: String,
    /// 0 cold war .. 2 conventional war (more and better enemy defences and patrols).
    pub roe: u8,
    /// Opponents' skill 0..3.
    pub opponents: u8,
    pub seed: u32,
    /// Game time runs this many times faster than the campaign clock for the sky (24: one day
    /// per real hour).
    pub sky_ratio: f32,
    /// Hour of the day at the start.
    pub start_hour: f32,
    /// The weather changes during the campaign (else a clear sky).
    #[serde(default = "yes")]
    pub dynamic_weather: bool,
}

fn yes() -> bool {
    true
}

/// The rules the campaign is fought under (kept with it: the same whoever serves it later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CampaignRules {
    pub realistic_damage: bool,
    pub realistic_guidance: bool,
    pub no_crashes: bool,
    pub continuous_time: bool,
    pub realistic_weapons: bool,
    #[serde(default)]
    pub friendly_fire: bool,
}

fn blue() -> Side {
    Side::Blue
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RaidState {
    /// Ordered; the strike aircraft take off at this time.
    Pending {
        launch_at: f64,
    },
    Enroute,
    /// The target was hit.
    Hit,
    /// Every strike aircraft was shot down or turned back before it got there.
    Repelled,
    /// Not flown: the target changed hands or was destroyed before.
    CalledOff,
}

/// Strike aircraft of a side on their way to bomb an airfield of the other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Raid {
    pub id: u32,
    pub side: Side,
    pub place: PlaceId,
    /// The airfield they bomb, the base they fly from.
    pub target: ObjectId,
    pub from: ObjectId,
    /// Campaign tags of the aircraft and which of them still fly to the target.
    pub aircraft: Vec<u32>,
    pub alive: Vec<bool>,
    pub state: RaidState,
    /// When it ended (campaign time).
    pub ended: Option<f64>,
    /// Its escort took off (ahead of it).
    #[serde(default)]
    pub escorted: bool,
    /// Attack drones (they dive into the target) instead of strike aircraft.
    #[serde(default)]
    pub drones: bool,
    /// When the strike aircraft took off.
    #[serde(default)]
    pub enroute_at: Option<f64>,
}

impl Raid {
    pub fn open(&self) -> bool {
        matches!(self.state, RaidState::Pending { .. } | RaidState::Enroute)
    }
}

/// A side's means of war: its materiel and the AI's aircraft on the ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forces {
    pub materiel: f64,
    pub fighters: u32,
    pub helicopters: u32,
    /// Where the materiel came from and went since the start.
    #[serde(default)]
    pub ledger: Ledger,
}

/// A side's materiel since the start of the campaign: earned, spent (by what on), lost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub earned: f64,
    pub repairs: f64,
    pub operations: f64,
    pub aircraft: f64,
    pub lost: f64,
}

/// What the AI spends materiel on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spend {
    /// Sites, airfields and guns put back in service.
    Repairs,
    /// Sorties, raids and insertions.
    Operations,
    /// New fighters and helicopters.
    Aircraft,
}

impl Forces {
    pub fn start(fighters: u32) -> Self {
        Self {
            materiel: MATERIEL_START,
            fighters,
            helicopters: HELICOPTERS_START,
            ledger: Ledger::default(),
        }
    }

    /// An older save's.
    fn initial() -> Self {
        Self::start(FIGHTERS_START)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownedState {
    Waiting,
    /// In a player's helicopter on the way home.
    Aboard,
    Rescued,
    Captured,
}

/// A player who ejected over enemy territory, waiting for a helicopter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Downed {
    pub id: u32,
    pub player: PlayerId,
    pub name: String,
    /// Where he came down (map units), and until when he can hold out.
    pub x: i32,
    pub y: i32,
    pub until: f64,
    pub state: DownedState,
}

/// How the campaign ended.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub winner: Side,
    /// Campaign time it ended.
    pub at: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignState {
    pub setup: CampaignSetup,
    /// Campaign seconds (real time while the game runs).
    pub clock: f64,
    pub sites: Vec<Site>,
    pub places: Vec<Place>,
    pub board: Vec<MissionOffer>,
    /// The players' plans for offers of the board.
    #[serde(default)]
    pub plans: Vec<MissionPlan>,
    pub insertions: Vec<Insertion>,
    pub players: Vec<PlayerSlot>,
    pub history: Vec<SortieRecord>,
    pub log: Vec<LogEntry>,
    pub next_id: u32,
    pub rng: Rng,
    /// When the board was last rebuilt (f64::MIN: never; JSON has no infinity), the last
    /// enemy patrol and automatic insertion.
    pub board_at: f64,
    pub patrol_at: f64,
    pub auto_insert_at: f64,
    /// Campaign tags of the fighters in the air (side, launch time).
    pub patrols: Vec<(u32, Side, f64)>,
    /// Friendly CAPs in the air, and the airfields that send none until then (campaign time).
    #[serde(default)]
    pub caps: Vec<CapFlight>,
    #[serde(default)]
    pub cap_lockout: Vec<(ObjectId, f64)>,
    /// The enemy's raids, and when its last offensive insertion and raid were ordered.
    #[serde(default)]
    pub raids: Vec<Raid>,
    #[serde(default)]
    pub enemy_insert_at: f64,
    #[serde(default)]
    pub raid_at: f64,
    /// The war is over (None: still going on).
    #[serde(default)]
    pub outcome: Option<Outcome>,
    /// Each side's materiel and aircraft, and when they last bought aircraft.
    #[serde(default = "Forces::initial")]
    pub blue: Forces,
    #[serde(default = "Forces::initial")]
    pub red: Forces,
    #[serde(default)]
    pub resupply_at: f64,
    /// Income is settled up to then.
    #[serde(default)]
    pub income_at: f64,
    /// Pilots down in enemy territory (and how their rescue went).
    #[serde(default)]
    pub downed: Vec<Downed>,
    /// The rules it is fought under (None: an older save, the server's).
    #[serde(default)]
    pub rules: Option<CampaignRules>,
    /// The enemy's AWACS in the air, when the last one went up, and the ones shot down.
    #[serde(default)]
    pub awacs: Option<Awacs>,
    #[serde(default)]
    pub awacs_at: f64,
    #[serde(default)]
    pub awacs_down: Vec<u32>,
    /// When the last drone wave was ordered.
    #[serde(default)]
    pub drones_at: f64,
    /// The enemy's forward batteries, and when it last looked into pushing one forward.
    #[serde(default)]
    pub batteries: Vec<Battery>,
    #[serde(default = "never")]
    pub battery_at: f64,
    #[serde(skip)]
    pub terrain: Terrain,
}

fn never() -> f64 {
    f64::MIN
}

/// Guns the enemy pushed forward, a short helicopter flight from the players' bases (when no
/// other target is).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Battery {
    pub id: u32,
    /// Map units (x east, y south).
    pub x: i32,
    pub y: i32,
    /// The place it is named after (the nearest).
    pub name: String,
    /// Its guns are in the simulation (it is gone once none of them is left).
    #[serde(default)]
    pub seen: bool,
}

/// The theater's land and water, for putting things on land: the host's (a coop client's
/// state has none), not saved and not compared.
#[derive(Clone, Default)]
pub struct Terrain(pub Option<std::sync::Arc<stealth_mission::TheaterData>>);

impl Terrain {
    /// Water at a map point; None without the theater's terrain.
    pub fn is_water(&self, x: i32, y: i32) -> Option<bool> {
        self.0.as_ref().map(|t| t.is_water(x, y))
    }
}

impl PartialEq for Terrain {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Terrain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "Terrain" } else { "Terrain(none)" })
    }
}

/// An AWACS orbiting a point until it is called home.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Awacs {
    pub tag: u32,
    pub side: Side,
    pub x: i32,
    pub y: i32,
    pub until: f64,
}

/// What a campaign fighter takes off for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FighterTask {
    /// Patrols and goes for the other side's aircraft.
    #[default]
    Patrol,
    /// Defends its airfield, then lands.
    Cap,
}

/// Friendly fighters defending an airfield.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapFlight {
    pub base: ObjectId,
    /// Campaign tags of its fighters still in the air.
    pub tags: Vec<u32>,
    pub launched: f64,
}

impl CampaignState {
    /// A new campaign on a theater's world (its friendly and neutral flags give the sides).
    pub fn new(setup: CampaignSetup, world: &MissionWorld) -> Self {
        let gu = world.ground_unit_count as usize;
        let mut sites: Vec<Site> = Vec::new();
        for (i, o) in world.objects.iter().enumerate().skip(3) {
            let (x, y) = (o.x as u16 as i32, o.y as u16 as i32);
            if (x, y) == (0, 0) {
                continue;
            }
            let f = o.flags;
            let kind = if f & flags::CARRIER != 0 {
                SiteKind::Carrier
            } else if f & flags::SHIP != 0 {
                SiteKind::Ship
            } else if f & flags::AIRFIELD != 0 {
                SiteKind::Airfield
            } else if i < gu && o.unit_type > 0 {
                SiteKind::Sam
            } else if i < gu {
                continue;
            } else {
                SiteKind::City
            };
            let side = if f & flags::FRIENDLY != 0 {
                Side::Blue
            } else if f & flags::NEUTRAL != 0 {
                Side::Neutral
            } else {
                Side::Red
            };
            sites.push(Site {
                object: i as ObjectId,
                name: world.name(o.name as i32).to_string(),
                x,
                y,
                kind,
                unit_type: o.unit_type,
                place: None,
                side,
                destroyed_at: None,
                removed: false,
                spotted: false,
            });
        }
        // Places: cities and airfields, merged by name and distance.
        let mut places: Vec<Place> = Vec::new();
        for s in sites.iter_mut() {
            if !matches!(s.kind, SiteKind::City | SiteKind::Airfield) {
                continue;
            }
            let found = places.iter().position(|p| {
                p.owner == s.side
                    && !s.name.is_empty()
                    && p.name.eq_ignore_ascii_case(&s.name)
                    && dist(p.x - s.x, p.y - s.y) < PLACE_MERGE
            });
            let p = match found {
                Some(p) => p,
                None => {
                    places.push(Place {
                        name: if s.name.is_empty() {
                            format!("Site {}", s.object)
                        } else {
                            s.name.clone()
                        },
                        x: s.x,
                        y: s.y,
                        owner: s.side,
                        members: Vec::new(),
                        airfield: None,
                        defences: Vec::new(),
                        intel_until: None,
                        guns_lost: 0,
                        guns_repair_at: None,
                    });
                    places.len() - 1
                }
            };
            places[p].members.push(s.object);
            if s.kind == SiteKind::Airfield && places[p].airfield.is_none() {
                places[p].airfield = Some(s.object);
                // The airfield is the place's centre (the insertions land there).
                places[p].x = s.x;
                places[p].y = s.y;
            }
            s.place = Some(p as PlaceId);
        }
        // SAM sites defend the nearest place of their side.
        for s in sites.iter_mut().filter(|s| s.kind == SiteKind::Sam) {
            let near = places
                .iter()
                .enumerate()
                .filter(|(_, p)| p.owner == s.side)
                .map(|(i, p)| (i, dist(p.x - s.x, p.y - s.y)))
                .filter(|&(_, d)| d < SITE_ASSOCIATION)
                .min_by_key(|&(_, d)| d);
            if let Some((p, _)) = near {
                s.place = Some(p as PlaceId);
                places[p].defences.push(s.object);
            }
        }
        let skill = setup.opponents.min(3) as u32;
        let mut c = Self {
            rng: Rng::new(setup.seed ^ 0xCA4D_5EED),
            setup,
            clock: 0.0,
            sites,
            places,
            board: Vec::new(),
            plans: Vec::new(),
            insertions: Vec::new(),
            players: vec![PlayerSlot { id: PlayerId(0), name: "Player".into(), sortie: None }],
            history: Vec::new(),
            log: Vec::new(),
            next_id: 1,
            board_at: f64::MIN,
            patrol_at: 0.0,
            auto_insert_at: 0.0,
            patrols: Vec::new(),
            caps: Vec::new(),
            cap_lockout: Vec::new(),
            raids: Vec::new(),
            enemy_insert_at: 0.0,
            raid_at: 0.0,
            outcome: None,
            blue: Forces::start(FIGHTERS_START),
            red: Forces::start(FIGHTERS_START + ENEMY_FIGHTERS_PER_SKILL * skill),
            resupply_at: 0.0,
            income_at: 0.0,
            downed: Vec::new(),
            rules: None,
            awacs: None,
            awacs_at: 0.0,
            awacs_down: Vec::new(),
            drones_at: 0.0,
            batteries: Vec::new(),
            battery_at: f64::MIN,
            terrain: Terrain::default(),
        };
        c.note("Campaign started");
        c
    }

    pub(crate) fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub(crate) fn note(&mut self, text: impl Into<String>) {
        self.log.push(LogEntry { time: self.clock, text: text.into() });
        if self.log.len() > LOG_SIZE {
            let extra = self.log.len() - LOG_SIZE;
            self.log.drain(..extra);
        }
    }

    pub fn site(&self, object: ObjectId) -> Option<&Site> {
        self.sites.iter().find(|s| s.object == object)
    }

    pub(crate) fn site_mut(&mut self, object: ObjectId) -> Option<&mut Site> {
        self.sites.iter_mut().find(|s| s.object == object)
    }

    pub fn place(&self, p: PlaceId) -> Option<&Place> {
        self.places.get(p as usize)
    }

    /// Who owns a world object now (its place's owner, else its own side); None when the
    /// campaign does not track it.
    pub fn owner(&self, object: ObjectId) -> Option<Side> {
        let s = self.site(object)?;
        Some(match (s.kind, s.place) {
            (SiteKind::City | SiteKind::Airfield, Some(p)) => self.places[p as usize].owner,
            _ => s.side,
        })
    }

    /// Bases a side flies from: its intact airfields and its carriers.
    pub fn bases(&self, side: Side) -> Vec<ObjectId> {
        self.sites
            .iter()
            .filter(|s| s.alive() && matches!(s.kind, SiteKind::Airfield | SiteKind::Carrier))
            .filter(|s| self.owner(s.object) == Some(side))
            .map(|s| s.object)
            .collect()
    }

    /// Bases the players fly from: every friendly airfield and carrier, bombed or not (a raid
    /// grounds the AI's aircraft there, never the players).
    pub fn player_bases(&self) -> Vec<ObjectId> {
        self.sites
            .iter()
            .filter(|s| !s.removed && matches!(s.kind, SiteKind::Airfield | SiteKind::Carrier))
            .filter(|s| self.owner(s.object) == Some(Side::Blue))
            .map(|s| s.object)
            .collect()
    }

    /// The players' base nearest to a point.
    pub fn nearest_player_base(&self, x: i32, y: i32) -> Option<ObjectId> {
        self.player_bases().into_iter().min_by_key(|&b| {
            let s = self.site(b).expect("base is a site");
            dist(s.x - x, s.y - y)
        })
    }

    /// The side's base nearest to a point (None when it has none).
    pub fn nearest_base(&self, side: Side, x: i32, y: i32) -> Option<ObjectId> {
        self.bases(side).into_iter().min_by_key(|&b| {
            let s = self.site(b).expect("base is a site");
            dist(s.x - x, s.y - y)
        })
    }

    /// The players' plan for an offer.
    pub fn plan(&self, offer: u32) -> Option<&MissionPlan> {
        self.plans.iter().find(|p| p.offer == offer)
    }

    /// The sortie a player is flying.
    pub fn sortie(&self, player: PlayerId) -> Option<&Sortie> {
        self.players.iter().find(|p| p.id == player).and_then(|p| p.sortie.as_ref())
    }

    /// A player joining under `name`: his slot (and record) if he played before, else a new
    /// one below `max`; with none left, the slot of a player not `busy` and not flying (his
    /// record stays his, under his name). None: no room.
    pub fn join(
        &mut self,
        name: &str,
        max: usize,
        busy: impl Fn(PlayerId) -> bool,
    ) -> Option<PlayerId> {
        if let Some(p) = self.players.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
            return Some(p.id);
        }
        let max = max.min(u8::MAX as usize + 1);
        let free =
            (0..max).map(|k| PlayerId(k as u8)).find(|id| self.players.iter().all(|p| p.id != *id));
        let id = match free {
            Some(id) => id,
            None => {
                let id = self
                    .players
                    .iter()
                    .filter(|p| (p.id.0 as usize) < max && p.sortie.is_none() && !busy(p.id))
                    .map(|p| p.id)
                    .next()?;
                // The record stays with the pilot who had the slot.
                let old = self.slot_mut(id).name.clone();
                for r in self.history.iter_mut().filter(|r| r.player == id && r.pilot.is_empty()) {
                    r.pilot = old.clone();
                }
                id
            }
        };
        self.slot_mut(id).name = name.to_string();
        Some(id)
    }

    /// Renames a player (the local pilot's name).
    pub fn rename(&mut self, player: PlayerId, name: &str) {
        let old = self.slot_mut(player).name.clone();
        for r in self.history.iter_mut().filter(|r| r.player == player) {
            if r.pilot.is_empty() || r.pilot.eq_ignore_ascii_case(&old) {
                r.pilot = name.to_string();
            }
        }
        self.slot_mut(player).name = name.to_string();
    }

    /// A player's name.
    pub fn player_name(&self, player: PlayerId) -> Option<&str> {
        self.players.iter().find(|p| p.id == player).map(|p| p.name.as_str())
    }

    /// A player's finished sorties (oldest first).
    pub fn records(&self, player: PlayerId) -> impl Iterator<Item = &SortieRecord> {
        let name = self.player_name(player).unwrap_or("").to_string();
        self.history.iter().filter(move |r| {
            r.player == player && (r.pilot.is_empty() || r.pilot.eq_ignore_ascii_case(&name))
        })
    }

    pub(crate) fn slot_mut(&mut self, player: PlayerId) -> &mut PlayerSlot {
        if let Some(i) = self.players.iter().position(|p| p.id == player) {
            return &mut self.players[i];
        }
        self.players.push(PlayerSlot {
            id: player,
            name: format!("Player {}", player.0 + 1),
            sortie: None,
        });
        self.players.last_mut().expect("just pushed")
    }

    /// Hours of the day for the sky (the sky's clock runs `sky_ratio` times faster).
    pub fn sky_hours(&self) -> f64 {
        self.setup.start_hour as f64 + self.clock * self.setup.sky_ratio as f64 / 3600.0
    }

    /// Places a side holds / all capturable places (for the war map's score line).
    pub fn held(&self, side: Side) -> usize {
        self.places.iter().filter(|p| p.owner == side).count()
    }

    pub fn insertion(&self, id: u32) -> Option<&Insertion> {
        self.insertions.iter().find(|i| i.id == id)
    }

    /// A side's materiel and aircraft (the neutrals have none: the enemy's).
    pub fn forces(&self, side: Side) -> &Forces {
        if side == Side::Blue { &self.blue } else { &self.red }
    }

    pub(crate) fn forces_mut(&mut self, side: Side) -> &mut Forces {
        if side == Side::Blue { &mut self.blue } else { &mut self.red }
    }

    pub fn downed_pilot(&self, id: u32) -> Option<&Downed> {
        self.downed.iter().find(|d| d.id == id)
    }

    pub fn raid(&self, id: u32) -> Option<&Raid> {
        self.raids.iter().find(|r| r.id == id)
    }

    /// Seconds the end of the campaign has been on the screen (None: the war goes on).
    pub fn ended_for(&self) -> Option<f64> {
        self.outcome.map(|o| (self.clock - o.at).max(0.0))
    }

    /// Recon intelligence on a world object (it or its place was photographed recently).
    pub fn has_intel(&self, object: ObjectId) -> bool {
        self.site(object)
            .and_then(|s| s.place)
            .and_then(|p| self.place(p))
            .is_some_and(|p| p.intel(self.clock))
    }
}

/// Plain Euclidean distance in map units (no integer approximation needed here).
pub fn dist(dx: i32, dy: i32) -> i32 {
    ((dx as f64).hypot(dy as f64)) as i32
}
