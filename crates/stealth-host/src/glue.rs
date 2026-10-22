//! Between the campaign (stealth-campaign) and the simulation (stealth-sim): what the
//! simulation shows the campaign, and the missions the campaign's sorties become.

use stealth_campaign::{
    AaaView, CampaignSetup, CampaignState, FlightView, MissionKind, PlayerId, PlayerPos, Report,
    Side, Sortie, SortieEnd, WorldView,
};
use stealth_mission::{GameData, Mission, MissionKind as GenKind, MissionSettings, Seed};
use stealth_sim::{Sim, fflags, oflags};

/// Metres per world unit.
const WU: f64 = 512_000.0 / 1_048_576.0;

pub fn side_of(blue: bool) -> Side {
    if blue { Side::Blue } else { Side::Red }
}

/// The campaign's mission: a strike mission of the theater (for its settings, briefing date,
/// time and weather) with the theater's own world and no aircraft of its own.
pub fn base_mission(setup: &CampaignSetup, data: &GameData) -> anyhow::Result<Mission> {
    let settings = MissionSettings {
        theater: setup.theater,
        roe: setup.roe,
        opponents: setup.opponents,
        kind: GenKind::Strike,
        ..Default::default()
    };
    let mut m =
        stealth_mission::generate(data, &settings, Seed::Srand((setup.seed & 0xFFFF) as u16))?;
    let td = data.theaters.get(setup.theater).ok_or_else(|| anyhow::anyhow!("no theater"))?;
    m.world = td.world.clone();
    m.drones = None;
    m.time.deadline_ticks = 0;
    m.time.has_deadline = false;
    Ok(m)
}

/// What the simulation shows the campaign.
pub fn world_view(sim: &Sim) -> WorldView {
    let aaa = sim
        .aaa_units()
        .iter()
        .enumerate()
        .map(|(k, u)| AaaView {
            x: u.x,
            y: u.y,
            side: side_of(sim.aaa_blue(k)),
            alive: !u.destroyed,
        })
        .collect();
    let flights = sim
        .flights()
        .iter()
        .enumerate()
        .filter(|(_, u)| {
            u.flags & (fflags::EXISTS | fflags::AIRBORNE) == fflags::EXISTS | fflags::AIRBORNE
                && u.flags & fflags::DESTROYED == 0
                && u.speed > 0
        })
        .filter_map(|(i, u)| {
            let tag = sim.campaign_flight(i).map_or(0, |f| f.tag);
            let blue = sim.friendly_unit(i, u.spec);
            // The base traffic is no part of the war.
            (tag != 0 || !blue).then_some(FlightView { x: u.x, y: u.y, side: side_of(blue), tag })
        })
        .collect();
    let players = (0..sim.pilot_count() as u8)
        .filter_map(|id| {
            let p = sim.pilot_view(id)?;
            Some(PlayerPos { id: PlayerId(id), x: p.mx, y: p.my, airborne: p.alt > 50 })
        })
        .collect();
    WorldView { aaa, flights, players }
}

/// Enemy SAM sites a friendly aircraft detects now: their radar painted a player (his RWR)
/// or they fire at friendly aircraft, or a friendly aircraft is within their radar's range and
/// above its horizon (its RWR picks the radar up).
pub fn spotted(state: &CampaignState, sim: &Sim) -> Vec<Report> {
    let eyes: Vec<(i32, i32, f64)> = sim
        .flights()
        .iter()
        .enumerate()
        .filter(|(_, u)| {
            u.flags & (fflags::EXISTS | fflags::AIRBORNE) == fflags::EXISTS | fflags::AIRBORNE
                && u.flags & fflags::DESTROYED == 0
                && u.speed > 0
        })
        .filter(|(i, u)| sim.friendly_unit(*i, u.spec))
        .map(|(_, u)| (u.x, u.y, u.alt.max(0) as f64 * WU))
        .collect();
    let objects = sim.objects();
    state
        .sites
        .iter()
        .filter(|s| s.kind == stealth_campaign::SiteKind::Sam && s.alive() && !s.spotted)
        .filter_map(|s| {
            let i = s.object as usize;
            let o = objects.get(i)?;
            if o.unit_type == 0 {
                return None;
            }
            let rwr = o.flags & (oflags::PAINTED | oflags::TRACKING) != 0 || sim.site_engaging(i);
            let range = (sim.radar_range(o.unit_type) * 64) as f64;
            let seen = eyes.iter().any(|&(x, y, alt)| {
                // Radar horizon (map units) of a target at `alt` metres.
                let horizon = 4120.0 * (alt.sqrt() + 3.2) / 15.625;
                ((x - o.x) as f64).hypot((y - o.y) as f64) < range.min(horizon)
            });
            (rwr || seen).then_some(Report::SiteSpotted { object: s.object })
        })
        .collect()
}

/// How a sortie ended, from the simulation's end record.
pub fn sortie_end(e: &stealth_sim::MissionEnd) -> (SortieEnd, &'static str) {
    match (e.outcome, e.landing_type) {
        (0, 2) => (SortieEnd::Ejected, "Ejected - picked up by friendly forces"),
        (0, _) => (SortieEnd::Landed, "Sortie ended on the ground"),
        (6, _) => (SortieEnd::Crashed, "Ejection failed - the pilot is recovered"),
        (7, _) => (SortieEnd::Aborted, "Sortie aborted"),
        (5, _) => (SortieEnd::Crashed, "Bad landing"),
        _ => (SortieEnd::Crashed, "Shot down / crashed - the pilot is recovered"),
    }
}

/// Stores for a kind of mission (weapon table indices, the generator's bays).
pub fn stores(kind: MissionKind) -> [i16; 4] {
    match kind {
        MissionKind::Recon => [16, 0, 2, 0],
        MissionKind::Sead => [2, 2, 5, 0],
        MissionKind::Cas => [5, 7, 5, 0],
        MissionKind::Strike => [6, 8, 6, 0],
        MissionKind::AntiShip => [4, 3, 5, 0],
        MissionKind::Cap | MissionKind::Escort | MissionKind::Insertion => [1, 0, 1, 0],
    }
}

/// The objective a sortie puts in the simulation's primary slot (mission type 1 photo, 2
/// destroy; object).
pub fn sim_objective(sortie: &Sortie) -> Option<(i32, usize)> {
    use stealth_campaign::Objective;
    match sortie.offer.objective {
        Objective::Recon { object, .. } => Some((1, object as usize)),
        Objective::Strike { airfield, .. } => Some((2, airfield as usize)),
        Objective::AntiShip { ship } => Some((2, ship as usize)),
        Objective::Sead { ref sites, .. } => sites.first().map(|&o| (2, o as usize)),
        _ => None,
    }
}

/// The sortie as a mission (briefing, kneeboard, route, stores): from the base to the target
/// and back.
pub fn sortie_mission(base: &Mission, state: &CampaignState, sortie: &Sortie) -> Mission {
    use stealth_mission::{Base, Bay, MapPoint, OrdersLine};
    let mut m = base.clone();
    let o = &sortie.offer;
    let b = state.site(sortie.base);
    let (bx, by) = b.map_or((0x4000, 0x4000), |s| (s.x, s.y));
    let home = Base {
        object_index: sortie.base as i16,
        name: b.map(|s| s.name.clone()).unwrap_or_default(),
        position: MapPoint { x: bx, y: by },
        carrier: b.is_some_and(|s| s.kind == stealth_campaign::SiteKind::Carrier),
        neutral: false,
        distance: 0,
    };
    m.takeoff = home.clone();
    m.landing = home;
    let target = MapPoint { x: o.x, y: o.y };
    let mid = MapPoint { x: (bx + o.x) / 2, y: (by + o.y) / 2 };
    m.waypoints = [mid, target, target, MapPoint { x: bx, y: by }];
    m.primary.place_name = o.title.clone();
    m.primary.position = target;
    m.primary.mission_type = if o.kind == MissionKind::Recon { 1 } else { 2 };
    m.primary.type_name = o.kind.name().into();
    m.secondary.position = target;
    m.secondary.mission_type = 0;
    let what = describe(o.kind);
    m.briefing.title = o.title.clone();
    m.briefing.primary_objective = what.to_string();
    m.briefing.secondary_objective = String::new();
    m.orders.primary = o.title.to_ascii_uppercase();
    m.orders.secondary = String::new();
    m.orders.return_to = format!("RETURN TO {}", m.landing.name.to_ascii_uppercase());
    m.orders.lines = vec![
        OrdersLine { text: o.title.clone(), color: 15 },
        OrdersLine { text: what.into(), color: 11 },
        OrdersLine { text: m.orders.return_to.clone(), color: 11 },
    ];
    m.loadout.bays =
        stores(o.kind).iter().map(|&w| Bay { weapon: w, name: String::new(), count: 1 }).collect();
    m.loadout.usable_bays =
        if m.settings.plane == stealth_mission::Plane::Lockheed { 2 } else { 4 };
    m
}

/// What a kind of mission asks the pilot for.
pub fn describe(kind: MissionKind) -> &'static str {
    match kind {
        MissionKind::Recon => {
            "Photograph the target: its defences are then known (spotted from twice the range, shorter radar range) for an hour"
        }
        MissionKind::Sead => "Destroy the SAM sites and radars defending the place",
        MissionKind::Cap => {
            "Hold the air over the point for 10 minutes or shoot down two enemy aircraft"
        }
        MissionKind::Cas => "Destroy the guns and MANPADS teams around the place",
        MissionKind::AntiShip => "Sink the enemy ship",
        MissionKind::Strike => {
            "Destroy the airfield: no enemy aircraft take off from it until it is repaired"
        }
        MissionKind::Escort => "Cover the insertion helicopters until they land",
        MissionKind::Insertion => {
            "Helicopters fly troops in from the nearest friendly base; one landing takes the place"
        }
    }
}

/// The friendly side's base nearest the front (where a player waits between sorties).
pub fn default_base(state: &CampaignState) -> Option<usize> {
    let front = state.front(Side::Blue);
    let (x, y) =
        front.first().and_then(|&p| state.place(p)).map_or((0x4000, 0x4000), |p| (p.x, p.y));
    state.nearest_base(Side::Blue, x, y).map(|b| b as usize)
}
