//! Geometry of one scenery block: roads with street lamps, buildings, pylons and wires (main
//! tier), and trees (detail tier near the viewpoint, far tier beyond). Everything is generated
//! from the network and per-position hashes, so a block looks the same whenever it is streamed
//! in, and both tree tiers place the same trees.

use std::f32::consts::TAU;

use bevy::math::DVec2;
use bevy::prelude::*;

use super::Scenery;
use super::geom::{Geom, rect, ring};
use super::network::{Building, Kind, Network, Rng, RoadKind, Roof, hash, noise};
use super::style::Tree;

/// Block size (metres).
pub const BLOCK: f64 = 1000.0;
/// Height of road surfaces above the ground overlays (metres).
const ROAD_Y: f32 = 0.9;
const ROAD: u8 = 0xEC;
const LAMP: u8 = 0x94;
const WINDOW: u8 = 0x95;
const BEACON: u8 = 0x92;
const STEEL: u8 = 0xE7;
const WIRE: u8 = 0xEE;
const WOOD: u8 = 0xCE;
const LAMP_SPACING: f64 = 50.0;
/// Tree grid spacing (metres).
const TREE_GRID: f64 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tier {
    /// Roads, lamps, buildings, power lines.
    Main,
    /// Trees.
    Detail,
    /// Distant trees: one small 3-sided cone or double cone per tree (3-6 triangles instead
    /// of 10-30), no bushes or trunks.
    Far,
}

/// Block-local frame: ground metres -> Bevy (x east, y up, z south) around the block centre.
pub(super) struct Frame {
    o: DVec2,
    min: DVec2,
}

impl Frame {
    fn new(bx: i32, by: i32) -> Self {
        let min = DVec2::new(bx as f64, by as f64) * BLOCK;
        Self { o: min + DVec2::splat(BLOCK * 0.5), min }
    }

    /// A frame around any origin (whole-airfield meshes).
    pub(super) fn around(o: DVec2) -> Self {
        Self { o, min: o - DVec2::splat(BLOCK * 0.5) }
    }

    pub(super) fn at(&self, p: DVec2, y: f32) -> Vec3 {
        Vec3::new((p.x - self.o.x) as f32, y, -(p.y - self.o.y) as f32)
    }

    fn contains(&self, p: DVec2) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.x < self.min.x + BLOCK && p.y < self.min.y + BLOCK
    }
}

/// Ground direction -> Bevy xz direction.
fn dir2(d: DVec2) -> Vec2 {
    Vec2::new(d.x as f32, -d.y as f32)
}

/// Bevy horizontal unit vector of a ground direction.
pub(super) fn dir3(d: DVec2) -> Vec3 {
    Vec3::new(d.x as f32, 0.0, -d.y as f32)
}

pub fn build(sc: &Scenery, bx: i32, by: i32, tier: Tier) -> Geom {
    let f = Frame::new(bx, by);
    let mut g = Geom::default();
    match tier {
        Tier::Main => {
            roads(&sc.net, &f, &mut g);
            let (min, max) = (f.min, f.min + DVec2::splat(BLOCK));
            for i in sc.net.building_index.around(min, max) {
                let b = &sc.net.buildings[i as usize];
                if f.contains(b.p) {
                    building(b, &f, &mut g);
                }
            }
            power(&sc.net, &f, &mut g);
        }
        Tier::Detail => trees(sc, &f, &mut g, false),
        Tier::Far => trees(sc, &f, &mut g, true),
    }
    g
}

fn roads(net: &Network, f: &Frame, g: &mut Geom) {
    let (min, max) = (f.min, f.min + DVec2::splat(BLOCK));
    let mut lamps: Vec<DVec2> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in net.segment_index.around(min, max) {
        if !seen.insert(id) {
            continue;
        }
        let (ri, s) = net.segments[id as usize];
        let road = &net.roads[ri as usize];
        let s = s as usize;
        let (a, b) = (road.pts[s], road.pts[s + 1]);
        if !f.contains((a + b) * 0.5) {
            continue;
        }
        let half = road.width * 0.5;
        strip(g, f, &road.pts, s, half, ROAD_Y, ROAD);
        // Centre line: keeps distant roads visible as in the original's line roads.
        g.line(ROAD, f.at(a, ROAD_Y + 0.05), f.at(b, ROAD_Y + 0.05));

        let (s0, s1) = (road.arc[s], road.arc[s + 1]);
        let mid = (a + b) * 0.5;
        let lit = match road.kind {
            RoadKind::Street => true,
            _ => net.town_at(mid, 1.0).is_some() || net.towns.iter().any(|t| t.c.distance(mid) < t.r + 350.0),
        };
        if !lit {
            continue;
        }
        let dir = (b - a).normalize_or_zero();
        let mut k = (s0 / LAMP_SPACING).ceil();
        while k * LAMP_SPACING < s1 {
            let t = (k * LAMP_SPACING - s0) / (s1 - s0);
            // Alternate sides on wide roads.
            let sd = if road.kind == RoadKind::Street || (k as i64) % 2 == 0 { 1.0 } else { -1.0 };
            let foot = a.lerp(b, t) + dir.perp() * sd * (half + 1.5);
            k += 1.0;
            if lamps.iter().any(|l| l.distance(foot) < 14.0) {
                continue;
            }
            lamps.push(foot);
            let top = f.at(foot, 8.5);
            let head = top - dir3(dir.perp()) * sd as f32 * 1.8;
            g.line(STEEL, f.at(foot, 0.0), top);
            g.line(STEEL, top, head);
            g.lamp(LAMP, head - Vec3::Y * 0.2);
        }
    }
}

/// Ground surface of segment `s` of a polyline, `half` wide on each side, with mitred ends so
/// consecutive segments join without gaps.
pub(super) fn strip(g: &mut Geom, f: &Frame, pts: &[DVec2], s: usize, half: f64, y: f32, colour: u8) {
    let side = |i: usize| -> DVec2 {
        let prev = if i > 0 { (pts[i] - pts[i - 1]).normalize_or_zero() } else { DVec2::ZERO };
        let next = if i + 1 < pts.len() { (pts[i + 1] - pts[i]).normalize_or_zero() } else { DVec2::ZERO };
        let n = (prev + next).normalize_or_zero().perp();
        let cos = n.dot((if next != DVec2::ZERO { next } else { prev }).perp()).max(0.5);
        n * half / cos
    };
    let (a, b) = (pts[s], pts[s + 1]);
    let (na, nb) = (side(s), side(s + 1));
    g.ground(colour, &[f.at(a - na, y), f.at(b - nb, y), f.at(b + nb, y), f.at(a + na, y)]);
}

pub(super) fn building(b: &Building, f: &Frame, g: &mut Geom) {
    let c = f.at(b.p, 0.0);
    let d = dir2(b.dir);
    let along = Vec3::new(d.x, 0.0, d.y);
    let mut rng = Rng::new(b.seed);
    let foot = rect(c, d, b.l, b.w);
    match b.kind {
        Kind::House | Kind::Apartment | Kind::Shed | Kind::Barn | Kind::Church | Kind::Tower => {
            g.walls(b.wall, &foot, 0.0, b.h);
            let rise = match b.kind {
                Kind::Shed => b.w * 0.18,
                Kind::Church => b.w * 0.6,
                _ => b.w * 0.42,
            };
            match b.roof {
                Roof::Flat => g.cap(b.roof_colour, &foot, b.h),
                Roof::Gable => g.gable(b.wall, b.roof_colour, &foot, b.h, rise),
                Roof::Hip => g.hip(b.roof_colour, &foot, b.h, rise * 0.85),
            }
            if b.kind == Kind::Church {
                let t = c - along * (b.l * 0.5 + 3.2);
                let tower = rect(t, d, 6.5, 6.5);
                let th = b.h * 2.4;
                g.block(b.wall, b.wall, &tower, 0.0, th);
                g.spire(b.roof_colour, &tower, th, t + Vec3::Y * (th + 11.0));
            }
            if b.kind == Kind::Tower && b.h > 60.0 {
                g.lamp(BEACON, c + Vec3::Y * (b.h + 1.0));
            }
            windows(b, c, d, &mut rng, g);
        }
        Kind::Silo => {
            let r = ring(c, b.l * 0.5, 8, 0.0);
            g.walls(b.wall, &r, 0.0, b.h);
            g.spire(b.roof_colour, &r, b.h, c + Vec3::Y * (b.h + 2.5));
        }
        Kind::Chimney => {
            let r = ring(c, 2.4, 8, 0.0);
            g.block(b.wall, b.roof_colour, &r, 0.0, b.h);
            g.lamp(BEACON, c + Vec3::Y * (b.h + 0.5));
        }
        Kind::Tank => {
            let r = ring(c, b.l * 0.5, 12, 0.0);
            g.block(b.wall, b.roof_colour, &r, 0.0, b.h);
        }
        Kind::WaterTower => {
            let tank = b.h - 7.0;
            g.walls(b.wall, &ring(c, 1.6, 6, 0.0), 0.0, tank);
            let r = ring(c, 4.5, 10, 0.0);
            g.walls(b.wall, &r, tank, b.h);
            g.face(b.wall, &r.iter().map(|p| *p + Vec3::Y * tank).collect::<Vec<_>>(), -Vec3::Y);
            g.spire(b.roof_colour, &r, b.h, c + Vec3::Y * (b.h + 2.0));
        }
        Kind::Mast => {
            lattice(g, c, along, &[(0.0, 1.2), (b.h, 0.4)], 3, (b.h / 8.0) as usize);
            g.lamp(BEACON, c + Vec3::Y * (b.h + 0.5));
            g.lamp(BEACON, c + Vec3::Y * (b.h * 0.5));
        }
        Kind::Shelter => {
            // Hardened aircraft shelter: an arched vault along `dir`, door at the front.
            let across = Vec3::new(-d.y, 0.0, d.x);
            let profile: Vec<Vec3> = (0..=6)
                .map(|i| {
                    let a = i as f32 / 6.0 * std::f32::consts::PI;
                    across * (a.cos() * b.w * 0.5) + Vec3::Y * (a.sin() * b.h)
                })
                .collect();
            let (front, back) = (c + along * (b.l * 0.5), c - along * (b.l * 0.5));
            for i in 0..6 {
                let (p, q) = (profile[i], profile[i + 1]);
                let out = (p + q) * 0.5 - Vec3::Y * 0.01;
                g.face(b.wall, &[back + p, back + q, front + q, front + p], out);
            }
            g.face(b.roof_colour, &profile.iter().map(|p| front + *p).collect::<Vec<_>>(), along);
            g.face(b.wall, &profile.iter().map(|p| back + *p).collect::<Vec<_>>(), -along);
        }
        Kind::Radar => {
            // Radome on a square tower.
            let top = b.h - 9.0;
            g.block(b.wall, b.wall, &rect(c, d, 6.0, 6.0), 0.0, top);
            let rings = [(4.6, 0.0), (5.6, 2.5), (5.0, 5.5), (3.2, 7.8)];
            for w in rings.windows(2) {
                let (lo, hi) = (ring(c + Vec3::Y * (top + w[0].1), w[0].0, 10, 0.0), ring(c + Vec3::Y * (top + w[1].1), w[1].0, 10, 0.0));
                for i in 0..10 {
                    let j = (i + 1) % 10;
                    let out = (lo[i] + lo[j] + hi[i] + hi[j]) * 0.25 - (c + Vec3::Y * (top + 4.0));
                    g.face(b.roof_colour, &[lo[i], lo[j], hi[j], hi[i]], out);
                }
            }
            g.spire(b.roof_colour, &ring(c, 3.2, 10, 0.0), top + 7.8, c + Vec3::Y * b.h);
            g.lamp(BEACON, c + Vec3::Y * (b.h + 0.3));
        }
        Kind::Windsock => {
            let top = c + Vec3::Y * b.h;
            g.line(STEEL, c, top);
            // Cone hanging half-inflated along `dir`.
            let across = Vec3::new(-d.y, 0.0, d.x);
            let rim: Vec<Vec3> = (0..6)
                .map(|i| {
                    let a = i as f32 / 6.0 * TAU;
                    top + across * (a.cos() * 0.7) + Vec3::Y * (a.sin() * 0.7 - 0.2)
                })
                .collect();
            let tip = top + along * 5.0 - Vec3::Y * 1.6;
            for i in 0..6 {
                let (p, q) = (rim[i], rim[(i + 1) % 6]);
                // Seen from inside and outside.
                g.face(b.wall, &[p, q, tip], (p + q) * 0.5 - top);
                g.face(b.wall, &[p, q, tip], top - (p + q) * 0.5);
            }
            g.lamp(BEACON, top + Vec3::Y * 0.4);
        }
        Kind::Sign => {
            let foot = rect(c, d, b.l, b.w);
            g.block(b.wall, b.roof_colour, &foot, 0.4, b.h);
            g.face(b.roof_colour, &foot.iter().map(|p| *p + Vec3::Y * 0.4).collect::<Vec<_>>(), -Vec3::Y);
        }
    }
}

/// Lit windows at night on the long sides.
fn windows(b: &Building, c: Vec3, d: Vec2, rng: &mut Rng, g: &mut Geom) {
    let n = match b.kind {
        Kind::Apartment => rng.range(1.0, 7.0) as i32,
        Kind::Tower => rng.range(2.0, 10.0) as i32,
        Kind::House if rng.chance(0.45) => 1,
        _ => 0,
    };
    let along = Vec3::new(d.x, 0.0, d.y);
    let across = Vec3::new(-d.y, 0.0, d.x);
    let floors = (b.h / 3.0).floor().max(1.0);
    for _ in 0..n {
        let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let floor = (rng.f() * floors as f64).floor() as f32;
        let x = rng.range(-0.4, 0.4) as f32 * b.l;
        g.lamp(WINDOW, c + along * x + across * side * (b.w * 0.5 + 0.15) + Vec3::Y * (floor * 3.0 + 1.6));
    }
}

/// Lattice tower: legs through the given (height, half width) sections, rings and bracing.
fn lattice(g: &mut Geom, c: Vec3, along: Vec3, sections: &[(f32, f32)], legs: usize, steps: usize) {
    let corners = |y: f32, hw: f32| -> Vec<Vec3> {
        let phase = along.z.atan2(along.x);
        (0..legs)
            .map(|i| {
                let a = phase + TAU / 8.0 * if legs == 4 { 1.0 } else { 0.0 } + i as f32 * TAU / legs as f32;
                c + Vec3::new(a.cos(), 0.0, a.sin()) * hw * if legs == 4 { std::f32::consts::SQRT_2 } else { 1.0 } + Vec3::Y * y
            })
            .collect()
    };
    for w in sections.windows(2) {
        let ((y0, h0), (y1, h1)) = (w[0], w[1]);
        let n = steps.max(1);
        let mut prev = corners(y0, h0);
        for s in 1..=n {
            let t = s as f32 / n as f32;
            let cur = corners(y0 + (y1 - y0) * t, h0 + (h1 - h0) * t);
            for i in 0..legs {
                let j = (i + 1) % legs;
                g.line(STEEL, prev[i], cur[i]);
                g.line(STEEL, cur[i], cur[j]);
                g.line(STEEL, prev[i], cur[j]);
            }
            prev = cur;
        }
    }
}

/// Conductor attachment points of a pylon (relative, `across` = horizontal unit across the line).
fn conductors(big: bool, across: Vec3) -> Vec<Vec3> {
    if big {
        vec![across * -8.5 + Vec3::Y * 23.0, across * 8.5 + Vec3::Y * 23.0, across * -5.5 + Vec3::Y * 29.0, across * 5.5 + Vec3::Y * 29.0]
    } else {
        vec![across * -1.1 + Vec3::Y * 8.6, across * 1.1 + Vec3::Y * 8.6]
    }
}

fn power(net: &Network, f: &Frame, g: &mut Geom) {
    let (min, max) = (f.min, f.min + DVec2::splat(BLOCK));
    for id in net.pylon_index.around(min, max) {
        let (li, k) = net.pylons[id as usize];
        let line = &net.lines[li as usize];
        let k = k as usize;
        let p = line.pylons[k];
        if !f.contains(p) {
            continue;
        }
        let dir_at = |k: usize| {
            let a = line.pylons[k.saturating_sub(1)];
            let b = line.pylons[(k + 1).min(line.pylons.len() - 1)];
            (b - a).normalize_or_zero()
        };
        let dir = dir_at(k);
        let across = dir3(dir.perp());
        let c = f.at(p, 0.0);
        if line.big {
            lattice(g, c, dir3(dir), &[(0.0, 4.0), (15.0, 1.6), (31.0, 0.8)], 4, 3);
            for (y, w) in [(23.0, 9.0), (29.0, 6.0)] {
                let base = c + Vec3::Y * y;
                g.line(STEEL, base - across * w, base + across * w);
                g.line(STEEL, base - across * w, base + Vec3::Y * 2.0);
                g.line(STEEL, base + across * w, base + Vec3::Y * 2.0);
            }
        } else {
            g.line(WOOD, c, c + Vec3::Y * 9.0);
            g.line(WOOD, c + across * -1.3 + Vec3::Y * 8.6, c + across * 1.3 + Vec3::Y * 8.6);
        }
        if k + 1 < line.pylons.len() {
            let q = line.pylons[k + 1];
            let cq = f.at(q, 0.0);
            let aq = dir3(dir_at(k + 1).perp());
            let span = p.distance(q) as f32;
            let sag = if line.big { span * 0.025 } else { 0.5 };
            for (pa, pb) in conductors(line.big, across).into_iter().zip(conductors(line.big, aq)) {
                let (a, b) = (c + pa, cq + pb);
                let n = if line.big { 6 } else { 2 };
                let pt = |i: usize| {
                    let t = i as f32 / n as f32;
                    a.lerp(b, t) - Vec3::Y * sag * 4.0 * t * (1.0 - t)
                };
                for i in 0..n {
                    g.line(WIRE, pt(i), pt(i + 1));
                }
            }
        }
    }
}

fn trees(sc: &Scenery, f: &Frame, g: &mut Geom, far: bool) {
    let Some(style) = &sc.style else { return };
    let n = (BLOCK / TREE_GRID) as i64;
    let (gx0, gy0) = ((f.min.x / TREE_GRID) as i64, (f.min.y / TREE_GRID) as i64);
    let seed = style.seed;
    for j in 0..n {
        for i in 0..n {
            let (gx, gy) = (gx0 + i, gy0 + j);
            let mut rng = Rng::new(hash(seed, gx, gy, 9));
            let p = DVec2::new((gx as f64 + rng.range(0.1, 0.9)) * TREE_GRID, (gy as f64 + rng.range(0.1, 0.9)) * TREE_GRID);
            let cover = noise(seed, p, 1600.0);
            let forest = ((cover - style.forest) / 0.1).clamp(0.0, 1.0);
            let roll = rng.f();
            let in_town = sc.net.town_at(p, 0.95).is_some();
            let (tree, bush) = if in_town {
                (roll < 0.05, false)
            } else {
                (roll < forest * 0.85 || roll > 0.995, forest > 0.0 && forest < 0.5 && roll > 0.97)
            };
            if !tree && !bush {
                continue;
            }
            if !sc.ground.is_free(p, 3.0) || !sc.net.road_clear(p, 4.0) || !sc.net.building_clear(p, 3.0) {
                continue;
            }
            let c = f.at(p, 0.0);
            if bush && far {
                continue;
            }
            if bush {
                let s = rng.range(1.5, 3.0) as f32;
                g.spire(0xAC, &ring(c, s, 5, rng.range(0.0, 6.3) as f32), 0.0, c + Vec3::Y * s * 1.2);
                continue;
            }
            let h = rng.range(8.0, 18.0) as f32;
            let phase = rng.range(0.0, 6.3) as f32;
            match style.tree(&mut rng) {
                Tree::Conifer if far => {
                    let col = rng.pick(&[0xAD, 0xAE]);
                    g.spire(col, &ring(c, h * 0.26, 3, phase), h * 0.08, c + Vec3::Y * h);
                }
                Tree::Broadleaf if far => {
                    let col = rng.pick(&[0xAB, 0xAC, 0xAD]);
                    let crown = ring(c, h * 0.33, 3, phase);
                    g.spire(col, &crown, h * 0.58, c + Vec3::Y * h);
                    g.spire(col, &crown, h * 0.58, c + Vec3::Y * h * 0.28);
                }
                Tree::Palm if far => {
                    let crown = ring(c, h * 0.3, 3, phase);
                    g.spire(0xA8, &crown, h * 0.9, c + Vec3::Y * h * 1.02);
                    g.spire(0xA8, &crown, h * 0.9, c + Vec3::Y * h * 0.7);
                }
                Tree::Conifer => {
                    let col = rng.pick(&[0xAD, 0xAE]);
                    g.spire(col, &ring(c, h * 0.27, 5, phase), h * 0.08, c + Vec3::Y * h * 0.72);
                    g.spire(col, &ring(c, h * 0.19, 5, phase + 0.6), h * 0.45, c + Vec3::Y * h);
                }
                Tree::Broadleaf => {
                    let col = rng.pick(&[0xAB, 0xAC, 0xAD]);
                    let crown = ring(c, h * 0.33, 6, phase);
                    let mid = h * 0.58;
                    g.walls(0xCD, &ring(c, 0.35, 3, phase), 0.0, h * 0.35);
                    g.spire(col, &crown, mid, c + Vec3::Y * h);
                    g.spire(col, &crown, mid, c + Vec3::Y * h * 0.28);
                }
                Tree::Palm => {
                    let lean = Vec3::new(rng.range(-1.0, 1.0) as f32, 0.0, rng.range(-1.0, 1.0) as f32) * h * 0.08;
                    let top = c + lean + Vec3::Y * h;
                    let base = ring(c, 0.35, 3, phase);
                    for k in 0..3 {
                        let (a, b) = (base[k], base[(k + 1) % 3]);
                        let out = (a + b) * 0.5 - c;
                        g.face(0xCC, &[a, b, top + (b - c) * 0.5, top + (a - c) * 0.5], out);
                    }
                    for k in 0..6 {
                        let a = phase + k as f32 * TAU / 6.0;
                        let d = Vec3::new(a.cos(), 0.0, a.sin());
                        let tip = top + d * h * 0.38 - Vec3::Y * h * 0.18;
                        let side = Vec3::new(-d.z, 0.0, d.x) * h * 0.06;
                        let mid = top + d * h * 0.2 + Vec3::Y * 0.4;
                        // Fronds are seen from above and below.
                        g.face(0xA8, &[top, mid + side, tip], Vec3::Y);
                        g.face(0xA8, &[top, mid + side, tip], -Vec3::Y);
                        g.face(0xA8, &[top, tip, mid - side], Vec3::Y);
                        g.face(0xA8, &[top, tip, mid - side], -Vec3::Y);
                    }
                }
            }
        }
    }
}
