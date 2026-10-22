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
}

impl Aircraft {
    pub fn name(self) -> &'static str {
        match self {
            Aircraft::F117 => "F-117A",
            Aircraft::F16 => "F-16",
        }
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
        }
    }

    /// Flown by the AI (the player orders it, he does not fly it).
    pub fn ai_flown(self) -> bool {
        self == MissionKind::Insertion
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Insertion {
    pub id: u32,
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
    /// A friendly helicopter or place lost in the CAP area.
    pub losses: u32,
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
    /// The escorted insertion (its result settles a pending escort).
    pub insertion: Option<u32>,
    #[serde(default)]
    pub aircraft: Aircraft,
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
    /// record goes). None: no room.
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
                self.history.retain(|r| r.player != id);
                id
            }
        };
        self.slot_mut(id).name = name.to_string();
        Some(id)
    }

    /// Renames a player (the local pilot's name).
    pub fn rename(&mut self, player: PlayerId, name: &str) {
        self.slot_mut(player).name = name.to_string();
    }

    /// A player's name.
    pub fn player_name(&self, player: PlayerId) -> Option<&str> {
        self.players.iter().find(|p| p.id == player).map(|p| p.name.as_str())
    }

    /// A player's finished sorties (oldest first).
    pub fn records(&self, player: PlayerId) -> impl Iterator<Item = &SortieRecord> {
        self.history.iter().filter(move |r| r.player == player)
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

    /// Recon intelligence on a world object (it or its place was photographed recently).
    pub fn has_intel(&self, object: ObjectId) -> bool {
        self.site(object)
            .and_then(|s| s.place)
            .and_then(|p| self.place(p))
            .is_some_and(|p| p.intel(self.clock))
    }
}

/// The original's distance approximation is not needed here: plain Euclid in map units.
pub fn dist(dx: i32, dy: i32) -> i32 {
    ((dx as f64).hypot(dy as f64)) as i32
}
