//! Attack drone (Geran) raids of the remake's Ukraine theater: the drone defense mission
//! (intercept a wave heading for a city or a nuclear plant, then bomb a launch site) and
//! drones crossing the theater during strike missions. Applied to a generated mission; the
//! original theaters never get any.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::world::{FlightUnit, TheaterData, WorldObject, flags as F};
use crate::{Base, GameData, MapPoint, Mission, OrdersLine, Target, WorldPoint};

/// Aircraft types (tables.json `planes`, sim.json `aircraft`).
pub const GERAN2: i16 = 29;
pub const GERAN3: i16 = 30;
/// Mission type of the drone defense primary.
pub const DRONE_INTERCEPT: i16 = 9;
/// Nuclear power plant shapes of the Ukraine theater.
const NPP_SHAPES: std::ops::RangeInclusive<usize> = 94..=98;
/// Weapons (sim.json `weapons`).
const AIM9: i16 = 0;
const AIM120: i16 = 1;
const GBU12: i16 = 6;
const EXTRA_FUEL: i16 = 17;

/// Drones of a mission: flight units appended after the mission's own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneWave {
    /// The mission is to stop them (drone defense); otherwise they cross a strike mission and
    /// only score a bonus.
    pub defend: bool,
    /// Object the wave flies at (defense: the protected place).
    pub target: i16,
    pub target_name: String,
    /// The target is a nuclear power plant.
    pub nuclear: bool,
    /// First drone flight unit and the number of drones.
    pub first_unit: usize,
    pub count: usize,
    /// Drones that may get through before the defense fails.
    pub allowance: u8,
}

/// Small LCG for the drone choices (the original generator's RNG is not touched).
struct R(u32);

impl R {
    fn r(&mut self, n: i32) -> i32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        if n <= 0 { 0 } else { ((self.0 >> 16) % n as u32) as i32 }
    }
}

fn dist(a: (i32, i32), b: (i32, i32)) -> f64 {
    (((a.0 - b.0) as f64).powi(2) + ((a.1 - b.1) as f64).powi(2)).sqrt()
}

fn pos(o: &WorldObject) -> (i32, i32) {
    (o.x as u16 as i32, o.y as u16 as i32)
}

/// Heading (0 north, clockwise, 16 bits) from a to b in map units (y grows south).
fn heading(a: (i32, i32), b: (i32, i32)) -> i16 {
    let ang = ((b.0 - a.0) as f64).atan2(-(b.1 - a.1) as f64);
    (ang / std::f64::consts::TAU * 65536.0).round() as i64 as i16
}

/// A drone in flight, `eta` seconds from its target, coming from `from`.
fn drone(m: &Mission, r: &mut R, ty: i16, target: i32, from: (i32, i32), eta: i32) -> FlightUnit {
    let tp = pos(&m.world.objects[target as usize]);
    let jet = ty == GERAN3;
    let speed = if jet { 350 } else { 115 };
    // Map units per second = speed / 32 (the AI flight model's integration).
    let d = dist(tp, from).max(1.0);
    let along = (speed as f64 / 32.0 * eta as f64).min(d);
    let (ux, uy) = ((from.0 - tp.0) as f64 / d, (from.1 - tp.1) as f64 / d);
    let side = (r.r(0x400) - 0x200) as f64;
    let x = (tp.0 as f64 + ux * along - uy * side).clamp(0x100 as f64, 0x7F00 as f64) as i32;
    let y = (tp.1 as f64 + uy * along + ux * side).clamp(0x100 as f64, 0x7F00 as f64) as i32;
    FlightUnit {
        waypoint: target as i16,
        x: x as i16,
        y: y as i16,
        alt: if jet { 2500 + r.r(2500) } else { 600 + r.r(900) } as i16,
        xp: x << 5,
        yp: y << 5,
        heading: heading((x, y), tp),
        plane_type: ty,
        // Exists and airborne.
        flags: 3,
        speed,
        fuel: 0x7FFF,
        ..Default::default()
    }
}

fn friendly_place(o: &WorldObject) -> bool {
    o.flags & F::FRIENDLY != 0
        && o.flags & (F::AIRFIELD | F::NEUTRAL | F::DISABLED) == 0
        && o.name != 0
}

/// Turns a generated air mission into a drone defense mission.
pub(crate) fn defend(m: &mut Mission, data: &GameData, th: &TheaterData) -> Result<()> {
    ensure!(!th.launch_sites.is_empty(), "no drone defense missions in {}", m.theater_name);
    let mut r = R(m.rng.holdrand_exit ^ 0x5EED_D20E);
    let diff = m.settings.opponents as i32;
    let roe = m.settings.roe as i32;
    let w = &m.world;
    // The place: a nuclear plant now and then, else a city.
    let places: Vec<i32> =
        (0..w.objects.len() as i32).filter(|&i| friendly_place(&w.objects[i as usize])).collect();
    ensure!(!places.is_empty(), "no friendly places in {}", m.theater_name);
    let npps: Vec<i32> = places
        .iter()
        .copied()
        .filter(|&i| NPP_SHAPES.contains(&w.objects[i as usize].shape()))
        .collect();
    let target = if !npps.is_empty() && r.r(4) == 0 {
        npps[r.r(npps.len() as i32) as usize]
    } else {
        places[r.r(places.len() as i32) as usize]
    };
    let to = w.objects[target as usize];
    let tp = pos(&to);
    // The two nearest launch sites send the wave; the nearest is the secondary target.
    let mut sites: Vec<i32> = th
        .launch_sites
        .iter()
        .map(|&i| i as i32)
        .filter(|&i| (i as usize) < w.objects.len())
        .collect();
    sites.sort_by(|&a, &b| {
        dist(pos(&w.objects[a as usize]), tp).total_cmp(&dist(pos(&w.objects[b as usize]), tp))
    });
    sites.truncate(2);
    let site = sites[0];
    // Takeoff and landing: the friendly airfield nearest the place.
    let base = (0..w.objects.len() as i32)
        .filter(|&i| {
            w.objects[i as usize].flags
                & (F::AIRFIELD | F::FRIENDLY | F::NEUTRAL | F::CARRIER | F::DISABLED)
                == F::AIRFIELD | F::FRIENDLY
        })
        .min_by(|&a, &b| {
            dist(pos(&w.objects[a as usize]), tp).total_cmp(&dist(pos(&w.objects[b as usize]), tp))
        })
        .ok_or_else(|| anyhow::anyhow!("no friendly airfield in {}", m.theater_name))?;
    let bo = w.objects[base as usize];
    let so = w.objects[site as usize];

    // The wave.
    let count = (4 + 2 * diff + roe + r.r(3)).clamp(4, 12) as usize;
    let jets = count * (diff + roe) as usize / 10;
    let first_unit = m.world.flights.len();
    let mut eta = 9 * 60 + r.r(180);
    for k in 0..count {
        let ty = if k < jets { GERAN3 } else { GERAN2 };
        let from = pos(&m.world.objects[sites[k % sites.len()] as usize]);
        let f = drone(m, &mut r, ty, target, from, eta);
        m.world.flights.push(f);
        eta += 30 + r.r(45);
    }
    let allowance = (count as i32 * (4 - diff) / 10) as u8;

    // The special aircraft (flight unit 0) of the generated air mission stays home.
    m.world.flights[0] = th.world.flights[0];
    m.special_aircraft = None;
    m.escort = -1;

    let names = |o: &WorldObject| m.world.name(o.name as i32).to_string();
    let (target_name, site_name, base_name) = (names(&to), names(&so), names(&bo));
    let nuclear = NPP_SHAPES.contains(&to.shape());
    let ott = |o: &WorldObject| m.world.ott(o.shape()) as i8;
    let class = |o: &WorldObject| m.world.buf7.get(o.shape()).copied().unwrap_or(0) & 0xF;
    let launch_names: Vec<String> =
        sites.iter().map(|&s| names(&m.world.objects[s as usize])).collect();
    let what = if nuclear { "the nuclear power plant" } else { "the city" };
    let primary_summary = format!(
        "Early warning reports a wave of {count} Geran attack drones launched from {} and heading for {what} {target_name}. \
         Fighter cover is committed elsewhere: you are the only aircraft able to reach them in time.",
        launch_names.join(" and ")
    );
    let primary_objective = format!(
        "Intercept the drones before they reach {target_name}. {}",
        match allowance {
            0 => "None of them may get through.".to_string(),
            1 => "No more than one may get through.".to_string(),
            n => format!("No more than {n} may get through."),
        }
    );
    let secondary_summary =
        format!("The drones are launched and stored at a depot near {site_name}.");
    let secondary_objective = format!("Destroy the drone launch site at {site_name}.");
    m.primary = Target {
        mission_type: DRONE_INTERCEPT,
        type_name: "intercept attack drones".into(),
        code: 0,
        has_deadline: false,
        area_target: false,
        rubble_at_deadline: false,
        special_model: None,
        object_index: target as i16,
        base_index: base as i16,
        place_name: target_name.clone(),
        shape: to.shape() as u8,
        shape_name: m.world.name(to.shape() as i32).into(),
        placed_by_generator: false,
        position: MapPoint { x: tp.0, y: tp.1 },
        category: ott(&to),
        weapon_class: class(&to),
        dist: 0,
        intelligence: primary_summary.clone(),
        objective: primary_objective.clone(),
        ..m.primary.clone()
    };
    m.secondary = Target {
        mission_type: 2,
        type_name: "destroy".into(),
        code: 0,
        has_deadline: false,
        area_target: false,
        rubble_at_deadline: false,
        special_model: None,
        object_index: site as i16,
        base_index: base as i16,
        place_name: site_name.clone(),
        shape: so.shape() as u8,
        shape_name: m.world.name(so.shape() as i32).into(),
        placed_by_generator: false,
        position: MapPoint { x: pos(&so).0, y: pos(&so).1 },
        category: ott(&so),
        weapon_class: class(&so),
        dist: 0,
        intelligence: secondary_summary.clone(),
        objective: secondary_objective.clone(),
        ..m.secondary.clone()
    };
    let bp = pos(&bo);
    let new_base = Base {
        object_index: base as i16,
        name: base_name.clone(),
        position: MapPoint { x: bp.0, y: bp.1 },
        carrier: false,
        neutral: false,
        distance: dist(bp, tp) as i16,
    };
    let (old_takeoff, old_landing) = (m.takeoff.name.clone(), m.landing.name.clone());
    m.takeoff = new_base.clone();
    m.landing = Base { distance: dist(bp, pos(&so)) as i16, ..new_base };
    m.exit_code = 0x0C;
    m.start_position = WorldPoint { x: bp.0 << 5, y: ((0x8000 - bp.1) << 5) - 0x708 };
    // Route: out toward the launch sites to meet the wave, the place, the launch site, home.
    let sp = pos(&so);
    let meet = (tp.0 + (sp.0 - tp.0) / 3, tp.1 + (sp.1 - tp.1) / 3);
    m.waypoints = [
        MapPoint { x: meet.0, y: meet.1 },
        MapPoint { x: tp.0, y: tp.1 },
        MapPoint { x: sp.0, y: sp.1 },
        MapPoint { x: bp.0, y: bp.1 },
    ];
    m.route_length =
        ((dist(bp, meet) + dist(meet, tp) + dist(tp, sp) + dist(sp, bp)) / 64.0) as i16;
    m.time.has_deadline = false;
    let old_fuel = m.fuel_lbs.to_string();
    m.fuel_lbs = 12 * m.route_length;
    m.briefing.fuel_line = m.briefing.fuel_line.replace(&old_fuel, &m.fuel_lbs.to_string());

    // Air-to-air missiles and a pair of Paveways for the launch site (the Lockheed: the gun
    // and the bombs only).
    let x = &data.tables;
    let unavailable = |w: i16| m.weapon_unavailable.get(w as usize).is_some_and(|&u| u != 0);
    let aim = if unavailable(AIM120) { AIM9 } else { AIM120 };
    let bays: Vec<i16> = if m.loadout.usable_bays == 2 {
        vec![GBU12, EXTRA_FUEL, 0, 0]
    } else {
        vec![aim, AIM9, GBU12, AIM9]
    };
    for (b, w) in m.loadout.bays.iter_mut().zip(bays) {
        b.weapon = w;
        b.name = x.weapon_name(w).into();
        b.count = x.weapon_qty.get(w as usize).copied().unwrap_or(0) as u8;
    }

    // Briefing and orders.
    let b = &mut m.briefing;
    b.title = "Air Defense".into();
    b.primary_summary = primary_summary;
    b.primary_objective = primary_objective;
    b.secondary_summary = secondary_summary;
    b.secondary_objective = secondary_objective;
    for l in [&mut b.base_date_line, &mut b.depart_line] {
        *l = l.replace(&old_takeoff, &base_name);
    }
    b.land_line = b.land_line.replace(&old_landing, &base_name);
    b.armament = m.loadout.bays[..m.loadout.usable_bays].iter().map(|b| b.name.clone()).collect();
    let l = &data.briefings.labels;
    let mut lines = vec![
        b.header.clone(),
        b.title.clone(),
        b.pilot_line.clone(),
        b.base_date_line.clone(),
        l.primary.clone(),
        b.primary_summary.clone(),
        l.objective.clone(),
        b.primary_objective.clone(),
        l.secondary.clone(),
        b.secondary_summary.clone(),
        l.objective.clone(),
        b.secondary_objective.clone(),
        l.flight_plan.clone(),
        b.depart_line.clone(),
        b.land_line.clone(),
        l.roe.clone(),
        b.roe_text.clone(),
        l.armament.clone(),
    ];
    lines.extend(b.armament.iter().cloned());
    lines.push(b.fuel_line.clone());
    lines.retain(|l| !l.is_empty());
    b.lines = lines;
    let o = &mut m.orders;
    o.lines = vec![
        OrdersLine { text: "Stop drones over".into(), color: 0x11 },
        OrdersLine { text: target_name.clone(), color: 0x11 },
        OrdersLine { text: "Destroy launch site".into(), color: 0x29 },
        OrdersLine { text: "at ".into(), color: 0x29 },
        OrdersLine { text: site_name.clone(), color: 0x29 },
        OrdersLine { text: "Return to".into(), color: 0xA },
        OrdersLine { text: base_name.clone(), color: 0xA },
    ];
    o.primary = format!("STOP GERAN DRONES HEADING FOR {}", target_name.to_uppercase());
    o.secondary = format!("DESTROY DRONE LAUNCH SITE AT {}", site_name.to_uppercase());
    o.return_to = format!("RETURN TO {}", base_name.to_uppercase());

    m.drones = Some(DroneWave {
        defend: true,
        target: target as i16,
        target_name,
        nuclear,
        first_unit,
        count,
        allowance,
    });
    Ok(())
}

/// Strike missions in a theater with launch sites: a few drones cross toward friendly cities
/// during the flight (targets of opportunity).
pub(crate) fn crossing(m: &mut Mission, th: &TheaterData) {
    let mut r = R(m.rng.holdrand_exit ^ 0xC20_55ED);
    let w = &m.world;
    let places: Vec<i32> =
        (0..w.objects.len() as i32).filter(|&i| friendly_place(&w.objects[i as usize])).collect();
    let sites: Vec<i32> = th
        .launch_sites
        .iter()
        .map(|&i| i as i32)
        .filter(|&i| (i as usize) < w.objects.len())
        .collect();
    if places.is_empty() || sites.is_empty() {
        return;
    }
    let target = places[r.r(places.len() as i32) as usize];
    let from = pos(&w.objects[sites[r.r(sites.len() as i32) as usize] as usize]);
    let count = 1 + r.r(3) as usize;
    let first_unit = m.world.flights.len();
    let mut eta = 8 * 60 + r.r(15 * 60);
    for k in 0..count {
        let ty = if k == 0 && r.r(3) == 0 { GERAN3 } else { GERAN2 };
        let f = drone(m, &mut r, ty, target, from, eta);
        m.world.flights.push(f);
        eta += 20 + r.r(40);
    }
    let to = m.world.objects[target as usize];
    m.drones = Some(DroneWave {
        defend: false,
        target: target as i16,
        target_name: m.world.name(to.name as i32).into(),
        nuclear: NPP_SHAPES.contains(&to.shape()),
        first_unit,
        count,
        allowance: count as u8,
    });
}
