//! Airfield extras around every runway of the theater (all theaters, also the desert ones): a
//! parallel taxiway with end, middle and high-speed exit connectors, an apron with hangars, an
//! operations building and a helipad, hardened aircraft shelters, a fuel farm, a radar,
//! windsocks and guidance signs. Lights: runway edges, thresholds and ends, approach lighting
//! with sequenced flashers, PAPI, taxiway edges and runway guard lights at the holding points.
//!
//! Positions are planned in runway coordinates: `u` along the runway axis, `v` across it.

use bevy::math::DVec2;
use bevy::prelude::*;

use super::content::{Frame, building, dir3, strip};
use super::geom::{Geom, PapiLight, rect};
use super::ground::{Ground, Runway};
use super::network::{Building, Kind, Rng, Roof, dist_to_segment};

/// Heights of the ground surfaces above the original's overlays (metres).
const TAXI_Y: f32 = 0.95;
const PAD_Y: f32 = 0.92;
const TAXI: u8 = 0xEC;
const PAD: u8 = 0xEB;
const MARKING: u8 = 0xC8;
const STRUCTURE: u8 = 0xE8;
/// Light colours (below 0xA0: no haze, the same in every palette of the day).
const WHITE: u8 = 0x0F;
const BLUE: u8 = 0x09;
const GREEN: u8 = 0x97;
const RED: u8 = 0x92;
const AMBER: u8 = 0x96;
const FLOOD: u8 = 0x94;
/// Approach lighting length beyond the runway end (metres) and barrette spacing.
const APPROACH: f64 = 900.0;
const BARRETTE: f64 = 30.0;
/// Sequenced flasher groups (one lit at a time, running towards the runway) and their colour.
pub const STROBE_PHASES: usize = 8;
pub const STROBE: u8 = WHITE;
/// PAPI colours below and above a unit's angle.
pub const PAPI_LOW: u8 = RED;
pub const PAPI_HIGH: u8 = WHITE;

pub struct Taxiway {
    pub pts: Vec<DVec2>,
    pub half: f64,
}

pub struct Airfield {
    pub runway: Runway,
    pub taxiways: Vec<Taxiway>,
    /// Concrete pads (apron, shelter fronts, helipad) as corner quads.
    pub pads: Vec<[DVec2; 4]>,
    pub buildings: Vec<Building>,
    /// Holding points: position on a taxiway and the direction towards the runway.
    pub holds: Vec<(DVec2, DVec2)>,
    /// Helipad centre and orientation.
    pub helipad: Option<(DVec2, DVec2)>,
    /// Apron floodlight masts.
    pub floods: Vec<DVec2>,
    /// Aircraft parking spots on the apron and the direction parked aircraft face.
    pub parking: Vec<(DVec2, DVec2)>,
    /// PAPI per runway end.
    pub papi: Vec<Papi>,
}

/// A PAPI for landing towards `-end` (the runway end it serves is at `u = end * half_len`):
/// four units from the runway outwards.
pub struct Papi {
    pub end: f64,
    pub units: [DVec2; 4],
}

/// PAPI angles from the runway outwards (minutes of arc): 3°30', 3°10', 2°50', 2°30', so a 3°
/// approach shows two white (outer) and two red (inner).
const PAPI_MINUTES: [f32; 4] = [210.0, 190.0, 170.0, 150.0];
/// Distance of the PAPI from the runway end (metres).
const PAPI_IN: f64 = 300.0;

/// What the ILS camera looks at on an approach: the runway centreline abeam the PAPI, and how
/// wide the view must be there to include the whole PAPI (ground metres, x east, y north).
#[derive(Clone, Copy, Debug)]
pub struct PapiSite {
    pub aim: DVec2,
    /// Unit direction from the PAPI towards approaching aircraft.
    pub toward: DVec2,
    /// Half width of the view at the aim point (metres).
    pub half_width: f64,
}

pub fn papi_sites(a: &Airfield) -> Vec<PapiSite> {
    let ax = Axes::new(&a.runway);
    a.papi
        .iter()
        .map(|p| {
            let aim = ax.at(p.end * (a.runway.half_len - PAPI_IN), 0.0);
            let outer = p.units.iter().map(|u| (*u - a.runway.c).dot(ax.v).abs()).fold(0.0, f64::max);
            PapiSite { aim, toward: ax.u * p.end, half_width: outer * 1.1 + 10.0 }
        })
        .collect()
}

/// Runway coordinates -> ground metres.
struct Axes {
    c: DVec2,
    u: DVec2,
    v: DVec2,
}

impl Axes {
    fn new(rw: &Runway) -> Self {
        Self { c: rw.c, u: rw.axis, v: rw.axis.perp() }
    }

    fn at(&self, u: f64, v: f64) -> DVec2 {
        self.c + self.u * u + self.v * v
    }
}

/// Plans the extras of one runway, keeping clear of water and the theater's raised objects.
pub fn plan(rw: &Runway, ground: &Ground, seed: u64) -> Airfield {
    let mut rng = Rng::new(seed);
    let ax = Axes::new(rw);
    let (l, w) = (rw.half_len, rw.half_width);
    let half = if w > 50.0 { 12.0 } else { 9.0 };
    let mut a = Airfield { runway: *rw, taxiways: vec![], pads: vec![], buildings: vec![], holds: vec![], helipad: None, floods: vec![], parking: vec![], papi: vec![] };
    let free_line = |p: DVec2, q: DVec2, r: f64| {
        let n = (p.distance(q) / 20.0).ceil().max(1.0) as usize;
        (0..=n).all(|i| ground.is_dry_clear(p.lerp(q, i as f64 / n as f64), r))
    };
    let end = l - 30.0;
    // Parallel taxiway: the nearest offset and side with a clear corridor.
    let Some((s, vt)) = [80.0, 110.0, 140.0, 180.0, 230.0].iter().flat_map(|&g| [(1.0, w + g), (-1.0, w + g)]).find(|&(s, vt)| {
        free_line(ax.at(-end, s * vt), ax.at(end, s * vt), half + 6.0)
            && free_line(ax.at(-end, s * w), ax.at(-end, s * vt), half)
            && free_line(ax.at(end, s * w), ax.at(end, s * vt), half)
    }) else {
        // No taxiway: the PAPI on the left of the landing direction, 25 m out.
        for end in [-1.0, 1.0] {
            let units = std::array::from_fn(|i| ax.at(end * (l - PAPI_IN), -end * (w + 25.0 + i as f64 * 15.0)));
            a.papi.push(Papi { end, units });
        }
        return a;
    };
    let mut main = vec![ax.at(-end, s * (w - 5.0)), ax.at(-end, s * vt)];
    let n = (2.0 * end / 150.0).ceil() as usize;
    main.extend((1..n).map(|i| ax.at(-end + 2.0 * end * i as f64 / n as f64, s * vt)));
    main.extend([ax.at(end, s * vt), ax.at(end, s * (w - 5.0))]);
    a.taxiways.push(Taxiway { pts: main, half });
    a.holds.push((ax.at(-end, s * (w + 45.0)), -ax.v * s));
    a.holds.push((ax.at(end, s * (w + 45.0)), -ax.v * s));
    // Middle connector and high-speed exits for both landing directions.
    if free_line(ax.at(0.0, s * w), ax.at(0.0, s * vt), half) {
        a.taxiways.push(Taxiway { pts: vec![ax.at(0.0, s * (w - 5.0)), ax.at(0.0, s * vt)], half });
        a.holds.push((ax.at(0.0, s * (w + 45.0)), -ax.v * s));
    }
    for dir in [1.0, -1.0] {
        let u0 = dir * l * 0.22;
        let u1 = u0 + dir * (vt - w) * 1.8;
        let (p, q) = (ax.at(u0, s * (w - 5.0)), ax.at(u1, s * vt));
        if u1.abs() < end && free_line(p, q, half) {
            a.taxiways.push(Taxiway { pts: vec![p, q], half });
            let t = 45.0 / (vt - w + 5.0);
            a.holds.push((p.lerp(q, t), (p - q).normalize()));
        }
    }

    // Apron with hangars and an operations building behind the taxiway.
    let depth = 130.0;
    let v0 = vt + half - 2.0;
    let apron_ok = |uc: f64, len: f64| {
        let mut ok = true;
        let mut u = uc - len * 0.5;
        while u <= uc + len * 0.5 && ok {
            let mut v = v0 + 10.0;
            while v <= v0 + depth + 70.0 && ok {
                ok = ground.is_dry_clear(ax.at(u, s * v), 6.0);
                v += 30.0;
            }
            u += 30.0;
        }
        ok
    };
    let len = (l * 0.45).clamp(300.0, 480.0);
    let apron = [0.0, -0.3, 0.3, -0.55, 0.55].iter().map(|f| f * l).find(|&uc| apron_ok(uc, len));
    if let Some(uc) = apron {
        let (u0, u1) = (uc - len * 0.5, uc + len * 0.5);
        a.pads.push([ax.at(u0, s * v0), ax.at(u1, s * v0), ax.at(u1, s * (v0 + depth)), ax.at(u0, s * (v0 + depth))]);
        let hangars = rng.range(2.0, 3.99) as usize;
        let hw = 48.0;
        for i in 0..hangars {
            let u = u0 + 40.0 + i as f64 * (hw + 14.0);
            a.buildings.push(Building {
                p: ax.at(u + hw * 0.5, s * (v0 + depth + 24.0)),
                dir: ax.u,
                kind: Kind::Shed,
                l: hw as f32,
                w: 42.0,
                h: rng.range(11.0, 15.0) as f32,
                roof: Roof::Gable,
                wall: rng.pick(&[0xE4, 0xF4, 0xE5]),
                roof_colour: rng.pick(&[0xE8, 0xF6, 0xE9]),
                seed: rng.u64(),
            });
        }
        a.buildings.push(Building {
            p: ax.at(u1 - 45.0, s * (v0 + depth + 22.0)),
            dir: ax.u,
            kind: Kind::Apartment,
            l: 55.0,
            w: 16.0,
            h: 9.0,
            roof: Roof::Flat,
            wall: rng.pick(&[0xE2, 0xC2, 0xE3]),
            roof_colour: 0xE7,
            seed: rng.u64(),
        });
        a.helipad = Some((ax.at(u1 - 30.0, s * (v0 + depth - 30.0)), ax.u));
        a.floods = vec![ax.at(u0 + 5.0, s * (v0 + depth - 5.0)), ax.at(uc, s * (v0 + depth - 5.0)), ax.at(u1 - 70.0, s * (v0 + depth - 5.0))];
        // A row across the middle of the apron, clear of the helipad, facing the runway.
        a.parking = (0..4).map(|k| (ax.at(u0 + 50.0 + k as f64 * 45.0, s * (v0 + depth * 0.5)), -ax.v * s)).collect();
    }

    // Hardened aircraft shelters near both runway ends, doors towards the taxiway.
    for dir in [1.0, -1.0] {
        for i in 0..3 {
            let u = dir * (end - 120.0 - i as f64 * 70.0);
            let door = ax.at(u, s * (vt + 65.0));
            let centre = ax.at(u, s * (vt + 65.0 + 18.0));
            if apron.is_some_and(|uc| (u - uc).abs() < len * 0.5 + 60.0) || !ground.is_clear(centre, 26.0) || !free_line(ax.at(u, s * vt), door, half) {
                continue;
            }
            a.taxiways.push(Taxiway { pts: vec![ax.at(u, s * vt), door], half: 8.0 });
            a.pads.push([ax.at(u - 16.0, s * (vt + 48.0)), ax.at(u + 16.0, s * (vt + 48.0)), ax.at(u + 16.0, s * (vt + 66.0)), ax.at(u - 16.0, s * (vt + 66.0))]);
            a.buildings.push(Building {
                p: centre,
                dir: -ax.v * s,
                kind: Kind::Shelter,
                l: 36.0,
                w: 24.0,
                h: 9.0,
                roof: Roof::Flat,
                wall: 0xE4,
                roof_colour: 0xEA,
                seed: rng.u64(),
            });
        }
    }

    // Fuel farm, radar and windsocks.
    let fuel = ax.at(-end * 0.55, s * (vt + 230.0));
    if ground.is_clear(fuel, 45.0) {
        for (du, dv) in [(-26.0, 0.0), (0.0, 0.0), (26.0, 0.0), (-13.0, 26.0), (13.0, 26.0)] {
            a.buildings.push(Building {
                p: fuel + ax.u * du + ax.v * s * dv,
                dir: ax.u,
                kind: Kind::Tank,
                l: 18.0,
                w: 18.0,
                h: 9.0,
                roof: Roof::Flat,
                wall: 0xE2,
                roof_colour: 0xE4,
                seed: rng.u64(),
            });
        }
    }
    for v in [-s * (w + 260.0), s * (vt + 300.0)] {
        let p = ax.at(l * 0.4, v);
        if ground.is_clear(p, 15.0) {
            a.buildings.push(Building { p, dir: ax.u, kind: Kind::Radar, l: 12.0, w: 12.0, h: 26.0, roof: Roof::Flat, wall: 0xE4, roof_colour: 0xE1, seed: rng.u64() });
            break;
        }
    }
    for u in [-(l - 250.0), l - 250.0] {
        let p = ax.at(u, -s * (w + 45.0));
        if ground.is_dry_clear(p, 5.0) {
            a.buildings.push(Building { p, dir: ax.v.rotate(DVec2::from_angle(rng.range(-0.8, 0.8))), kind: Kind::Windsock, l: 1.0, w: 1.0, h: 7.0, roof: Roof::Flat, wall: 0xCB, roof_colour: 0xCB, seed: rng.u64() });
        }
    }
    // PAPI between the runway and the parallel taxiway, clear of the connectors: the units
    // spread over the strip (at most 15 m apart), moved along the runway if a connector or an
    // object is in the way.
    let (inner, outer) = (w + 8.0, vt - half - 8.0);
    let spacing = ((outer - inner) / 3.0).min(15.0);
    for end in [-1.0, 1.0] {
        let units_at = |u: f64| -> [DVec2; 4] { std::array::from_fn(|i| ax.at(u, s * (inner + i as f64 * spacing))) };
        let clear = |units: &[DVec2; 4]| {
            units.iter().all(|&p| {
                ground.is_dry_clear(p, 4.0) && !a.taxiways.iter().any(|t| t.pts.windows(2).any(|w| dist_to_segment(p, w[0], w[1]) < t.half + 10.0))
            })
        };
        let units = [0.0, 40.0, -40.0, 80.0, -80.0, 120.0, -120.0]
            .iter()
            .map(|d| units_at(end * (l - PAPI_IN + d)))
            .find(|u| clear(u))
            .unwrap_or_else(|| units_at(end * (l - PAPI_IN)));
        a.papi.push(Papi { end, units });
    }

    // Guidance signs beside the holding points.
    for &(p, d) in &a.holds.clone() {
        a.buildings.push(Building { p: p + d.perp() * (half + 7.0), dir: d.perp(), kind: Kind::Sign, l: 3.2, w: 0.5, h: 1.6, roof: Roof::Flat, wall: 0xC8, roof_colour: 0xEF, seed: 0 });
    }
    a
}

/// Areas to keep clear of towns, roads and trees (axis-aligned bounds, metres).
pub fn footprint(a: &Airfield) -> Vec<Rect> {
    let rw = &a.runway;
    let ax = Axes::new(rw);
    let bounds = |pts: &[DVec2], m: f64| {
        let (mut min, mut max) = (pts[0], pts[0]);
        for p in pts {
            min = min.min(*p);
            max = max.max(*p);
        }
        Rect::from_corners((min - m).as_vec2(), (max + m).as_vec2())
    };
    let mut out = vec![bounds(&[ax.at(-rw.half_len - APPROACH - 40.0, -60.0), ax.at(rw.half_len + APPROACH + 40.0, 60.0)], 0.0)];
    out.push(bounds(&[ax.at(-rw.half_len - 60.0, -rw.half_width - 80.0), ax.at(rw.half_len + 60.0, rw.half_width + 80.0)], 0.0));
    for t in &a.taxiways {
        for s in t.pts.windows(2) {
            out.push(bounds(s, t.half + 15.0));
        }
    }
    out.extend(a.pads.iter().map(|p| bounds(p, 15.0)));
    out.extend(a.buildings.iter().map(|b| bounds(&[b.p], b.radius() + 10.0)));
    out
}

/// The whole airfield's geometry around the runway centre.
pub fn build(a: &Airfield) -> Geom {
    let rw = &a.runway;
    let ax = Axes::new(rw);
    let f = Frame::around(rw.c);
    let mut g = Geom::default();
    let (l, w) = (rw.half_len, rw.half_width);
    let on_runway = |p: DVec2| {
        let d = p - rw.c;
        d.dot(ax.u).abs() < l + 5.0 && d.dot(ax.v).abs() < w + 3.0
    };

    // Taxiways: surface, yellow centre line, blue edge lights.
    for (ti, t) in a.taxiways.iter().enumerate() {
        for s in 0..t.pts.len() - 1 {
            strip(&mut g, &f, &t.pts, s, t.half, TAXI_Y, TAXI);
            let (p, q) = (t.pts[s], t.pts[s + 1]);
            if !on_runway(p) || !on_runway(q) {
                g.line(MARKING, f.at(p, TAXI_Y + 0.05), f.at(q, TAXI_Y + 0.05));
            }
            let dir = (q - p).normalize_or_zero();
            let len = p.distance(q);
            let mut k = 0.0;
            while k <= len {
                for side in [-1.0, 1.0] {
                    let lp = p + dir * k + dir.perp() * side * (t.half + 1.0);
                    let inside_other = a.taxiways.iter().enumerate().any(|(oi, o)| {
                        oi != ti && o.pts.windows(2).any(|ow| dist_to_segment(lp, ow[0], ow[1]) < o.half - 0.5)
                    });
                    if !on_runway(lp) && !inside_other && !a.pads.iter().any(|pad| inside_quad(pad, lp)) {
                        g.lamp(BLUE, f.at(lp, TAXI_Y + 0.4));
                    }
                }
                k += 30.0;
            }
        }
    }
    for pad in &a.pads {
        g.ground(PAD, &pad.map(|p| f.at(p, PAD_Y)));
    }

    // Holding points: hold lines across the taxiway and runway guard lights.
    for &(p, d) in &a.holds {
        let n = d.perp();
        for (off, y) in [(0.0, 0.0), (2.0, 0.0)] {
            let c = p + d * off;
            g.line(MARKING, f.at(c - n * 11.0, TAXI_Y + 0.06 + y), f.at(c + n * 11.0, TAXI_Y + 0.06 + y));
        }
        for side in [-1.0, 1.0] {
            let base = p + n * side * 15.0;
            g.line(STRUCTURE, f.at(base, 0.0), f.at(base, 1.0));
            g.lamp(AMBER, f.at(base, 1.0) + dir3(n) * 0.3);
            g.lamp(AMBER, f.at(base, 1.0) - dir3(n) * 0.3);
        }
    }

    // Runway edge lights, thresholds and ends.
    let mut u = -l;
    while u <= l + 0.1 {
        for v in [-(w + 1.5), w + 1.5] {
            g.lamp(WHITE, f.at(ax.at(u, v), 0.6));
        }
        u += 60.0;
    }
    for end in [-1.0, 1.0] {
        let mut v = -w;
        while v <= w + 0.1 {
            g.lamp(GREEN, f.at(ax.at(end * (l + 3.0), v), 0.5));
            g.lamp(RED, f.at(ax.at(end * (l - 3.0), v), 0.5));
            v += 6.0;
        }
        approach(&mut g, &f, &ax, end, l, w);
    }
    // PAPI units: housings and the lights facing the approach.
    for papi in &a.papi {
        let toward = dir3(ax.u) * papi.end as f32;
        for (p, minutes) in papi.units.iter().zip(PAPI_MINUTES) {
            g.block(0xE6, 0xE8, &rect(f.at(*p, 0.0), Vec2::new(ax.u.x as f32, -ax.u.y as f32), 2.0, 3.0), 0.0, 1.4);
            g.papi.push(PapiLight { pos: f.at(*p, 1.5) + toward * 1.1, toward, angle: (minutes / 60.0).to_radians() });
        }
    }

    // Helipad: square pad, white H and touchdown circle, green perimeter lights.
    if let Some((c, d)) = a.helipad {
        let (dx, dy) = (d, d.perp());
        let corner = |x: f64, y: f64| c + dx * x + dy * y;
        g.ground(PAD, &[corner(-14.0, -14.0), corner(14.0, -14.0), corner(14.0, 14.0), corner(-14.0, 14.0)].map(|p| f.at(p, PAD_Y + 0.03)));
        let y = PAD_Y + 0.08;
        for (p, q) in [((-4.0, -5.0), (4.0, -5.0)), ((-4.0, 5.0), (4.0, 5.0)), ((0.0, -5.0), (0.0, 5.0))] {
            g.line(WHITE, f.at(corner(p.0, p.1), y), f.at(corner(q.0, q.1), y));
        }
        for i in 0..16 {
            let (a0, a1) = (i as f64 / 16.0 * std::f64::consts::TAU, (i + 1) as f64 / 16.0 * std::f64::consts::TAU);
            g.line(WHITE, f.at(c + DVec2::from_angle(a0) * 10.0, y), f.at(c + DVec2::from_angle(a1) * 10.0, y));
        }
        for i in 0..8 {
            let p = c + DVec2::from_angle(i as f64 / 8.0 * std::f64::consts::TAU) * 14.5;
            g.lamp(GREEN, f.at(p, PAD_Y + 0.4));
        }
    }
    for &p in &a.floods {
        g.line(STRUCTURE, f.at(p, 0.0), f.at(p, 22.0));
        g.lamp(FLOOD, f.at(p, 22.3));
        g.lamp(RED, f.at(p, 23.0));
    }
    for b in &a.buildings {
        building(b, &f, &mut g);
    }
    g
}

fn inside_quad(q: &[DVec2; 4], p: DVec2) -> bool {
    (0..4).all(|i| (q[(i + 1) % 4] - q[i]).perp_dot(p - q[i]) >= 0.0) || (0..4).all(|i| (q[(i + 1) % 4] - q[i]).perp_dot(p - q[i]) <= 0.0)
}

/// Approach lighting beyond one runway end: centreline barrettes every 30 m on stands, a
/// crossbar at 300 m, red side rows over the first 270 m and sequenced flashers.
fn approach(g: &mut Geom, f: &Frame, ax: &Axes, end: f64, l: f64, w: f64) {
    let spread = (w * 0.12).clamp(3.0, 6.0);
    let mut d = BARRETTE;
    let mut k = 0;
    while d <= APPROACH + 0.1 {
        let u = end * (l + d);
        let y = 0.6 + (d / APPROACH) as f32 * 4.0;
        let c = ax.at(u, 0.0);
        g.line(STRUCTURE, f.at(c, 0.0), f.at(c, y - 0.2));
        g.line(STRUCTURE, f.at(ax.at(u, -spread), y - 0.2), f.at(ax.at(u, spread), y - 0.2));
        for i in -2..=2 {
            g.lamp(WHITE, f.at(ax.at(u, i as f64 * spread * 0.5), y));
        }
        if (d - 300.0).abs() < 1.0 {
            // Crossbar.
            let span = (w * 0.45).clamp(15.0, 30.0);
            g.line(STRUCTURE, f.at(ax.at(u, -span), y - 0.2), f.at(ax.at(u, span), y - 0.2));
            let mut v = spread * 1.5;
            while v <= span {
                g.lamp(WHITE, f.at(ax.at(u, v), y));
                g.lamp(WHITE, f.at(ax.at(u, -v), y));
                v += 2.5;
            }
        }
        if d < 280.0 {
            for side in [-1.0, 1.0] {
                for i in 0..3 {
                    g.lamp(RED, f.at(ax.at(u, side * (spread * 2.5 + i as f64 * 1.5)), y));
                }
            }
        }
        if d >= 300.0 {
            // Flashers run from the far end towards the runway, a group per phase.
            let phase = STROBE_PHASES - 1 - (k * STROBE_PHASES / ((APPROACH - 300.0) / BARRETTE + 1.0) as usize).min(STROBE_PHASES - 1);
            g.strobe(phase, f.at(c, y + 0.6));
            k += 1;
        }
        d += BARRETTE;
    }
}
