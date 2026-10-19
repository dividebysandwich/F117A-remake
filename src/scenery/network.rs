//! The theater-wide layout, generated once per theater from a fixed seed: towns with street
//! grids, roads between them (and along the original highway lines), transmission lines
//! between the larger towns, pole lines along some roads, and the buildings of towns,
//! industrial sites and farms. Ground positions are metres (x east, y north).

use bevy::math::DVec2;

use super::ground::{Ground, Index};
use super::style::Style;

/// Theater size in metres (2^20 world units).
const THEATER_M: f64 = 512_000.0;

/// splitmix64.
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Hash of a few integers.
pub fn hash(seed: u64, a: i64, b: i64, c: i64) -> u64 {
    mix(seed ^ mix(a as u64 ^ mix(b as u64 ^ mix(c as u64))))
}

/// Deterministic random sequence.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(mix(seed))
    }

    pub fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }

    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f64 {
        (self.u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f()
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f() < p
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.u64() % items.len() as u64) as usize]
    }

    /// Index into `weights` with probability proportional to the weight.
    pub fn weighted(&mut self, weights: &[f64]) -> usize {
        let mut x = self.f() * weights.iter().sum::<f64>();
        for (i, w) in weights.iter().enumerate() {
            if x < *w {
                return i;
            }
            x -= w;
        }
        weights.len() - 1
    }
}

/// Smooth value noise in 0..1 (two octaves), `scale` metres per lattice cell.
pub fn noise(seed: u64, p: DVec2, scale: f64) -> f64 {
    let octave = |seed: u64, p: DVec2| {
        let (x0, y0) = (p.x.floor(), p.y.floor());
        let (fx, fy) = (p.x - x0, p.y - y0);
        let v = |dx: i64, dy: i64| (hash(seed, x0 as i64 + dx, y0 as i64 + dy, 0) >> 11) as f64 / (1u64 << 53) as f64;
        let s = |t: f64| t * t * (3.0 - 2.0 * t);
        let (sx, sy) = (s(fx), s(fy));
        let top = v(0, 0) + (v(1, 0) - v(0, 0)) * sx;
        let bottom = v(0, 1) + (v(1, 1) - v(0, 1)) * sx;
        top + (bottom - top) * sy
    };
    (octave(seed, p / scale) * 2.0 + octave(seed ^ 0x55, p / scale * 3.1)) / 3.0
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TownKind {
    Village,
    Town,
    City,
}

pub struct Town {
    pub c: DVec2,
    pub r: f64,
    pub kind: TownKind,
    /// Street grid axis.
    pub u: DVec2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoadKind {
    /// Along a road line of the original terrain.
    Highway,
    Road,
    Street,
}

pub struct Road {
    pub pts: Vec<DVec2>,
    /// Arc length at each point.
    pub arc: Vec<f64>,
    pub kind: RoadKind,
    pub width: f64,
}

impl Road {
    fn new(pts: Vec<DVec2>, kind: RoadKind, width: f64) -> Self {
        let mut arc = vec![0.0];
        for w in pts.windows(2) {
            arc.push(arc.last().unwrap() + w[0].distance(w[1]));
        }
        Self { pts, arc, kind, width }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    House,
    Barn,
    Silo,
    Apartment,
    Tower,
    Church,
    Shed,
    Chimney,
    Tank,
    WaterTower,
    Mast,
    /// Hardened aircraft shelter (`h`: arch height, door at the `dir` end).
    Shelter,
    Radar,
    Windsock,
    /// Airfield guidance sign.
    Sign,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Roof {
    Flat,
    Gable,
    Hip,
}

#[derive(Clone, Copy, Debug)]
pub struct Building {
    pub p: DVec2,
    /// Unit direction of the length.
    pub dir: DVec2,
    pub kind: Kind,
    pub l: f32,
    pub w: f32,
    /// Wall height.
    pub h: f32,
    pub roof: Roof,
    pub wall: u8,
    pub roof_colour: u8,
    pub seed: u64,
}

impl Building {
    pub fn radius(&self) -> f64 {
        (self.l.max(self.w) * 0.5) as f64
    }
}

pub struct PowerLine {
    pub pylons: Vec<DVec2>,
    /// Transmission line (lattice towers) or a pole line.
    pub big: bool,
}

#[derive(Default)]
pub struct Network {
    pub towns: Vec<Town>,
    pub town_index: Index,
    pub roads: Vec<Road>,
    /// (road, segment) ids.
    pub segment_index: Index,
    pub segments: Vec<(u32, u32)>,
    pub buildings: Vec<Building>,
    pub building_index: Index,
    pub lines: Vec<PowerLine>,
    /// (line, pylon) ids by pylon position.
    pub pylon_index: Index,
    pub pylons: Vec<(u32, u32)>,
}

pub fn dist_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

impl Network {
    /// Whether a circle stays clear of every road surface.
    pub fn road_clear(&self, p: DVec2, r: f64) -> bool {
        let d = DVec2::splat(r + 10.0);
        !self.segment_index.around(p - d, p + d).any(|i| {
            let (road, s) = self.segments[i as usize];
            let road = &self.roads[road as usize];
            dist_to_segment(p, road.pts[s as usize], road.pts[s as usize + 1]) < road.width * 0.5 + r
        })
    }

    pub fn building_clear(&self, p: DVec2, r: f64) -> bool {
        let d = DVec2::splat(r + 60.0);
        !self.building_index.around(p - d, p + d).any(|i| {
            let b = &self.buildings[i as usize];
            b.p.distance(p) < b.radius() + r
        })
    }

    /// The town whose area contains a point (scaled by `f`).
    pub fn town_at(&self, p: DVec2, f: f64) -> Option<&Town> {
        self.town_index.at(p).iter().map(|&i| &self.towns[i as usize]).find(|t| t.c.distance(p) < t.r * f)
    }

    pub fn build(ground: &Ground, style: &Style) -> Network {
        let mut n = Network {
            towns: vec![],
            town_index: Index::default(),
            roads: vec![],
            segment_index: Index::default(),
            segments: vec![],
            buildings: vec![],
            building_index: Index::default(),
            lines: vec![],
            pylon_index: Index::default(),
            pylons: vec![],
        };
        n.place_towns(ground, style);
        for t in 0..n.towns.len() {
            n.streets(t, ground, style);
        }
        for h in &ground.highways {
            n.roads.push(Road::new(subdivide(h, 150.0), RoadKind::Highway, 10.0));
        }
        n.connect_towns(ground, style);
        n.index_roads();
        for t in 0..n.towns.len() {
            n.town_buildings(t, ground, style);
        }
        n.farms(ground, style);
        n.power_lines(ground, style);
        n
    }

    fn place_towns(&mut self, ground: &Ground, style: &Style) {
        let spacing = 6500.0;
        let cells = (THEATER_M / spacing) as i64;
        for gy in 0..cells {
            for gx in 0..cells {
                let mut rng = Rng::new(hash(style.seed, gx, gy, 1));
                if !rng.chance(style.towns) {
                    continue;
                }
                let c = DVec2::new((gx as f64 + rng.range(0.15, 0.85)) * spacing, (gy as f64 + rng.range(0.15, 0.85)) * spacing);
                let (kind, r) = match rng.weighted(&[0.68, 0.25, 0.07]) {
                    0 => (TownKind::Village, rng.range(160.0, 300.0)),
                    1 => (TownKind::Town, rng.range(420.0, 700.0)),
                    _ => (TownKind::City, rng.range(900.0, 1400.0)),
                };
                let r = r * style.town_size;
                if !ground.is_free(c, r * 0.35) {
                    continue;
                }
                let free = (0..8)
                    .filter(|&i| {
                        let a = i as f64 * std::f64::consts::TAU / 8.0;
                        ground.is_free(c + DVec2::from_angle(a) * r * 0.85, 10.0)
                    })
                    .count();
                if free < 6 || self.towns.iter().any(|t| t.c.distance(c) < t.r + r + 1500.0) {
                    continue;
                }
                let u = DVec2::from_angle(rng.range(0.0, std::f64::consts::PI));
                let id = self.towns.len() as u32;
                let d = DVec2::splat(r + 200.0);
                self.town_index.insert(c - d, c + d, id);
                self.towns.push(Town { c, r, kind, u });
            }
        }
    }

    /// Street grid of a town (villages: one or two streets through the centre).
    fn streets(&mut self, t: usize, ground: &Ground, style: &Style) {
        let town = &self.towns[t];
        let (c, r, u) = (town.c, town.r, town.u);
        let v = u.perp();
        let mut rng = Rng::new(hash(style.seed, t as i64, 0, 2));
        let spacing = street_spacing(town.kind);
        let (n, width) = match town.kind {
            TownKind::Village => (0, 7.0),
            _ => ((r / spacing) as i32, 8.0),
        };
        for (axis, across) in [(u, v), (v, u)] {
            if town.kind == TownKind::Village && axis == v && !rng.chance(0.6) {
                continue;
            }
            for i in -n..=n {
                let off = i as f64 * spacing;
                let half = (r * r - off * off).max(0.0).sqrt() * if i == 0 { 1.0 } else { rng.range(0.75, 1.0) };
                if half < spacing * 0.6 {
                    continue;
                }
                // Runs of the street over free land.
                let o = c + across * off;
                let n = (2.0 * half / 20.0).ceil() as usize;
                let mut run: Vec<DVec2> = Vec::new();
                for k in 0..=n + 1 {
                    let p = o + axis * (-half + 2.0 * half * k as f64 / n as f64);
                    if k <= n && ground.is_free(p, 4.0) {
                        run.push(p);
                        continue;
                    }
                    if run.len() >= 4 {
                        let pts = subdivide(&[run[0], *run.last().unwrap()], 120.0);
                        self.roads.push(Road::new(pts, RoadKind::Street, if i == 0 { width + 2.0 } else { width }));
                    }
                    run.clear();
                }
            }
        }
    }

    /// Roads to the nearest towns, and from towns without one to the nearest highway.
    fn connect_towns(&mut self, ground: &Ground, style: &Style) {
        let mut pairs = Vec::new();
        for (i, a) in self.towns.iter().enumerate() {
            let k = match a.kind {
                TownKind::Village => 2,
                TownKind::Town => 3,
                TownKind::City => 4,
            };
            let mut near: Vec<(f64, usize)> = self
                .towns
                .iter()
                .enumerate()
                .filter(|&(j, b)| j != i && b.c.distance(a.c) < 24_000.0)
                .map(|(j, b)| (b.c.distance(a.c), j))
                .collect();
            near.sort_by(|x, y| x.0.total_cmp(&y.0));
            for &(_, j) in near.iter().take(k) {
                let p = (i.min(j), i.max(j));
                if !pairs.contains(&p) {
                    pairs.push(p);
                }
            }
        }
        let mut connected = vec![false; self.towns.len()];
        for (i, j) in pairs {
            let (a, b) = (&self.towns[i], &self.towns[j]);
            let (pa, pb) = (exit(a, b.c), exit(b, a.c));
            if let Some(pts) = route(ground, style.seed ^ ((i as u64) << 20 | j as u64), pa, pb) {
                self.roads.push(Road::new(pts, RoadKind::Road, 8.0));
                connected[i] = true;
                connected[j] = true;
            }
        }
        let highway_points: Vec<DVec2> =
            self.roads.iter().filter(|r| r.kind == RoadKind::Highway).flat_map(|r| r.pts.iter().copied()).collect();
        for (i, town) in self.towns.iter().enumerate() {
            if connected[i] {
                continue;
            }
            let Some(&h) = highway_points.iter().min_by(|a, b| a.distance(town.c).total_cmp(&b.distance(town.c))) else { break };
            if h.distance(town.c) > 9000.0 {
                continue;
            }
            if let Some(pts) = route(ground, style.seed ^ i as u64, exit(town, h), h) {
                self.roads.push(Road::new(pts, RoadKind::Road, 8.0));
            }
        }
    }

    fn index_roads(&mut self) {
        for (ri, road) in self.roads.iter().enumerate() {
            for s in 0..road.pts.len() - 1 {
                let (a, b) = (road.pts[s], road.pts[s + 1]);
                let d = DVec2::splat(road.width);
                let id = self.segments.len() as u32;
                self.segment_index.insert(a.min(b) - d, a.max(b) + d, id);
                self.segments.push((ri as u32, s as u32));
            }
        }
    }

    fn add_building(&mut self, b: Building) {
        let d = DVec2::splat(b.radius());
        let id = self.buildings.len() as u32;
        self.building_index.insert(b.p - d, b.p + d, id);
        self.buildings.push(b);
    }

    /// Lots along a town's streets (larger buildings towards the centre), a church, a water
    /// tower and industrial sites at the edge.
    fn town_buildings(&mut self, t: usize, ground: &Ground, style: &Style) {
        let town = &self.towns[t];
        let (c, r, kind, u) = (town.c, town.r, town.kind, town.u);
        let spacing = street_spacing(kind);
        let mut rng = Rng::new(hash(style.seed, t as i64, 0, 3));
        let streets: Vec<(DVec2, DVec2, f64)> = self
            .roads
            .iter()
            .filter(|road| road.kind == RoadKind::Street && road.pts[0].distance(c) <= r + 1.0 && road.pts.last().unwrap().distance(c) <= r + 1.0)
            .map(|road| (road.pts[0], *road.pts.last().unwrap(), road.width))
            .collect();
        let mut lots = Vec::new();
        for (a, b, width) in streets {
            let dir = (b - a).normalize();
            let len = a.distance(b);
            for side in [-1.0, 1.0] {
                let mut s = rng.range(5.0, 20.0);
                while s < len {
                    let p = a + dir * s;
                    let core = 1.0 - p.distance(c) / r;
                    let mut b = style.building(&mut rng, kind, core);
                    let step = b.l as f64 + rng.range(4.0, 14.0) * if core < 0.3 { 1.8 } else { 1.0 };
                    // Lot centre: across the street, clear of the cross streets.
                    let along = (p - c).dot(dir).rem_euclid(spacing);
                    let near_cross = kind != TownKind::Village && along.min(spacing - along) < b.l as f64 * 0.5 + 12.0;
                    let back = width * 0.5 + rng.range(3.0, 7.0) + b.w as f64 * 0.5;
                    b.p = p + dir.perp() * side * back;
                    b.dir = dir;
                    // Fewer buildings towards the edge.
                    if !near_cross && rng.chance(0.35 + core * 0.9) {
                        lots.push(b);
                    }
                    s += step;
                }
            }
        }
        if kind != TownKind::Village || rng.chance(0.7) {
            // Church near the centre.
            let p = c + u * spacing * 0.5 + u.perp() * spacing * 0.5;
            lots.push(style.church(&mut rng, p, u));
        }
        for b in lots {
            if ground.is_free(b.p, b.radius()) && self.road_clear(b.p, b.w as f64 * 0.5) && self.building_clear(b.p, b.radius()) {
                self.add_building(b);
            }
        }
        if kind != TownKind::Village || rng.chance(0.3) {
            let p = c + DVec2::from_angle(rng.range(0.0, std::f64::consts::TAU)) * (r + 40.0);
            let wt = Building { p, dir: u, kind: Kind::WaterTower, l: 9.0, w: 9.0, h: 26.0, roof: Roof::Flat, wall: 0xE4, roof_colour: 0xE6, seed: rng.u64() };
            if ground.is_free(p, 10.0) && self.road_clear(p, 8.0) && self.building_clear(p, 8.0) {
                self.add_building(wt);
            }
        }
        let sites = match kind {
            TownKind::Village => 0,
            TownKind::Town => 1,
            TownKind::City => 3,
        };
        for _ in 0..sites {
            let p = c + DVec2::from_angle(rng.range(0.0, std::f64::consts::TAU)) * (r + rng.range(120.0, 300.0));
            self.industry(p, ground, style, &mut rng);
        }
    }

    /// Sheds, chimneys and tanks around a point.
    fn industry(&mut self, p: DVec2, ground: &Ground, style: &Style, rng: &mut Rng) {
        if !ground.is_free(p, 90.0) || !self.road_clear(p, 60.0) || !self.building_clear(p, 90.0) {
            return;
        }
        let dir = DVec2::from_angle(rng.range(0.0, std::f64::consts::PI));
        let mut parts = vec![];
        let n = rng.range(1.0, 3.99) as i32;
        for i in 0..n {
            let off = dir.perp() * (i as f64 - (n - 1) as f64 * 0.5) * 38.0;
            let roof = if rng.chance(0.5) { Roof::Flat } else { Roof::Gable };
            parts.push(Building {
                p: p + off,
                dir,
                kind: Kind::Shed,
                l: rng.range(40.0, 80.0) as f32,
                w: rng.range(18.0, 28.0) as f32,
                h: rng.range(7.0, 12.0) as f32,
                roof,
                wall: rng.pick(&[0xE3, 0xE5, 0xF3, 0xC2]),
                roof_colour: rng.pick(&[0xE8, 0xE9, 0xF5, 0xD9]),
                seed: rng.u64(),
            });
        }
        let edge = p + dir * 55.0;
        for i in 0..rng.range(1.0, 2.99) as i32 {
            parts.push(Building {
                p: edge + dir.perp() * (i as f64 * 14.0 - 7.0),
                dir,
                kind: Kind::Chimney,
                l: 5.0,
                w: 5.0,
                h: rng.range(40.0, 85.0) as f32,
                roof: Roof::Flat,
                wall: rng.pick(&[0xD6, 0xE5, 0xE3]),
                roof_colour: 0xEE,
                seed: rng.u64(),
            });
        }
        for i in 0..rng.range(0.0, 3.99) as i32 {
            parts.push(Building {
                p: p - dir * 60.0 + dir.perp() * (i as f64 * 26.0 - 26.0),
                dir,
                kind: Kind::Tank,
                l: 20.0,
                w: 20.0,
                h: rng.range(8.0, 14.0) as f32,
                roof: Roof::Flat,
                wall: 0xE2,
                roof_colour: 0xE4,
                seed: rng.u64(),
            });
        }
        let _ = style;
        for b in parts {
            if ground.is_free(b.p, b.radius()) {
                self.add_building(b);
            }
        }
    }

    /// Farmsteads along the roads between towns, and an occasional radio mast.
    fn farms(&mut self, ground: &Ground, style: &Style) {
        let mut sites = Vec::new();
        for (ri, road) in self.roads.iter().enumerate() {
            if road.kind == RoadKind::Street {
                continue;
            }
            let mut rng = Rng::new(hash(style.seed, ri as i64, 0, 4));
            let total = *road.arc.last().unwrap();
            let mut s = rng.range(100.0, 400.0);
            while s < total {
                let (p, dir) = point_at(road, s);
                let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
                let site = p + dir.perp() * side * rng.range(45.0, 90.0);
                if rng.chance(style.farms) {
                    sites.push((site, dir, rng.u64()));
                }
                s += rng.range(250.0, 600.0);
            }
        }
        for (p, dir, seed) in sites {
            if self.town_at(p, 1.0).is_some() || self.towns.iter().any(|t| t.c.distance(p) < t.r + 120.0) {
                continue;
            }
            if !ground.is_free(p, 40.0) || !self.road_clear(p, 25.0) || !self.building_clear(p, 45.0) {
                continue;
            }
            let mut rng = Rng::new(seed);
            for b in style.farm(&mut rng, p, dir) {
                if ground.is_free(b.p, b.radius()) && self.road_clear(b.p, b.radius()) {
                    self.add_building(b);
                }
            }
        }
        // Radio masts on open land.
        let spacing = 16_000.0;
        let cells = (THEATER_M / spacing) as i64;
        for gy in 0..cells {
            for gx in 0..cells {
                let mut rng = Rng::new(hash(style.seed, gx, gy, 5));
                if !rng.chance(0.25) {
                    continue;
                }
                let p = DVec2::new((gx as f64 + rng.f()) * spacing, (gy as f64 + rng.f()) * spacing);
                if ground.is_free(p, 30.0) && self.road_clear(p, 30.0) && self.building_clear(p, 30.0) && self.town_at(p, 1.3).is_none() {
                    let h = rng.range(60.0, 140.0) as f32;
                    self.add_building(Building { p, dir: DVec2::X, kind: Kind::Mast, l: 6.0, w: 6.0, h, roof: Roof::Flat, wall: 0xE6, roof_colour: 0xE6, seed: rng.u64() });
                }
            }
        }
    }

    /// Whether a pylon may stand here.
    fn pylon_spot(&self, ground: &Ground, p: DVec2) -> bool {
        ground.is_free(p, 12.0) && self.road_clear(p, 6.0) && self.building_clear(p, 10.0)
    }

    /// Transmission lines linking the towns and cities (a spanning tree of short links), and
    /// pole lines along some of the roads.
    fn power_lines(&mut self, ground: &Ground, style: &Style) {
        let hubs: Vec<usize> = (0..self.towns.len()).filter(|&i| self.towns[i].kind != TownKind::Village).collect();
        let mut joined = vec![false; hubs.len()];
        for start in 0..hubs.len() {
            if joined[start] {
                continue;
            }
            joined[start] = true;
            let mut tree = vec![start];
            // Prim's algorithm over links up to 45 km, within this connected group.
            loop {
                let mut best: Option<(f64, usize, usize)> = None;
                for &a in &tree {
                    for b in 0..hubs.len() {
                        if joined[b] {
                            continue;
                        }
                        let d = self.towns[hubs[a]].c.distance(self.towns[hubs[b]].c);
                        if d < 45_000.0 && best.is_none_or(|x| d < x.0) {
                            best = Some((d, a, b));
                        }
                    }
                }
                let Some((_, a, b)) = best else { break };
                joined[b] = true;
                tree.push(b);
                let (ta, tb) = (&self.towns[hubs[a]], &self.towns[hubs[b]]);
                let dir = (tb.c - ta.c).normalize();
                let (pa, pb) = (ta.c + dir * (ta.r + 150.0), tb.c - dir * (tb.r + 150.0));
                let mut rng = Rng::new(hash(style.seed, a as i64, b as i64, 6));
                if let Some(pylons) = self.pylon_route(ground, &mut rng, pa, pb) {
                    self.add_line(PowerLine { pylons, big: true });
                }
            }
        }
        for ri in 0..self.roads.len() {
            let road = &self.roads[ri];
            let mut rng = Rng::new(hash(style.seed, ri as i64, 0, 7));
            if road.kind != RoadKind::Road || !rng.chance(style.pole_lines) {
                continue;
            }
            let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
            let off = road.width * 0.5 + 5.0;
            let total = *road.arc.last().unwrap();
            let mut run = Vec::new();
            let mut s = 20.0;
            let mut runs = Vec::new();
            while s < total - 20.0 {
                let (p, dir) = point_at(road, s);
                let q = p + dir.perp() * side * off;
                if ground.is_free(q, 3.0) && self.building_clear(q, 4.0) && self.town_at(q, 0.95).is_none() {
                    run.push(q);
                } else if run.len() > 1 {
                    runs.push(std::mem::take(&mut run));
                } else {
                    run.clear();
                }
                s += 55.0;
            }
            if run.len() > 1 {
                runs.push(run);
            }
            for pylons in runs {
                self.add_line(PowerLine { pylons, big: false });
            }
        }
    }

    /// Pylons every ~330 m from `a` to `b`, bending once around obstacles; a pylon that can't
    /// stand is moved along the line or left out if the span stays under 700 m.
    fn pylon_route(&self, ground: &Ground, rng: &mut Rng, a: DVec2, b: DVec2) -> Option<Vec<DVec2>> {
        let bends = [0.0, 0.12, -0.12, 0.25, -0.25];
        'attempt: for &bend in &bends {
            let mid = (a + b) * 0.5 + (b - a).perp() * bend * rng.range(0.8, 1.2);
            let mut out: Vec<DVec2> = vec![];
            for (p, q) in [(a, mid), (mid, b)] {
                let len = p.distance(q);
                let n = (len / 330.0).ceil().max(1.0) as usize;
                for i in 0..=n {
                    if i == 0 && !out.is_empty() {
                        continue;
                    }
                    let base = p.lerp(q, i as f64 / n as f64);
                    let dir = (q - p).normalize();
                    let spot = [0.0, 40.0, -40.0, 80.0, -80.0].iter().map(|&o| base + dir * o).find(|&s| self.pylon_spot(ground, s));
                    match spot {
                        Some(s) => out.push(s),
                        None if i != 0 && i != n => {}
                        None => continue 'attempt,
                    }
                    if out.len() > 1 && out[out.len() - 2].distance(out[out.len() - 1]) > 700.0 {
                        continue 'attempt;
                    }
                }
            }
            return Some(out);
        }
        None
    }

    fn add_line(&mut self, line: PowerLine) {
        let li = self.lines.len() as u32;
        for (k, &p) in line.pylons.iter().enumerate() {
            let id = self.pylons.len() as u32;
            self.pylon_index.insert(p, p, id);
            self.pylons.push((li, k as u32));
        }
        self.lines.push(line);
    }
}

pub fn street_spacing(kind: TownKind) -> f64 {
    match kind {
        TownKind::Village => 1e9,
        TownKind::Town => 105.0,
        TownKind::City => 125.0,
    }
}

/// Where a road towards `to` leaves a town: the end of a main street facing it.
fn exit(t: &Town, to: DVec2) -> DVec2 {
    let d = (to - t.c).normalize_or_zero();
    let axis = [t.u, -t.u, t.u.perp(), -t.u.perp()].into_iter().max_by(|a, b| a.dot(d).total_cmp(&b.dot(d))).unwrap();
    t.c + axis * t.r
}

/// Position and direction at an arc length along a road.
pub fn point_at(road: &Road, s: f64) -> (DVec2, DVec2) {
    let i = road.arc.partition_point(|&a| a <= s).clamp(1, road.pts.len() - 1) - 1;
    let (a, b) = (road.pts[i], road.pts[i + 1]);
    let seg = road.arc[i + 1] - road.arc[i];
    let t = if seg > 0.0 { (s - road.arc[i]) / seg } else { 0.0 };
    (a.lerp(b, t.clamp(0.0, 1.0)), (b - a).normalize_or_zero())
}

/// Splits a polyline so no segment is longer than `max`.
fn subdivide(pts: &[DVec2], max: f64) -> Vec<DVec2> {
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        let n = (w[0].distance(w[1]) / max).ceil().max(1.0) as usize;
        out.extend((1..=n).map(|i| w[0].lerp(w[1], i as f64 / n as f64)));
    }
    out
}

/// A gently winding road from `a` to `b` over free land, or None.
fn route(ground: &Ground, seed: u64, a: DVec2, b: DVec2) -> Option<Vec<DVec2>> {
    let len = a.distance(b);
    if len < 50.0 {
        return None;
    }
    for attempt in 0..4 {
        let mut rng = Rng::new(seed ^ (attempt << 40));
        let amp = if attempt == 0 { 0.03 } else { 0.08 + 0.05 * attempt as f64 };
        // Midpoint displacement, two levels.
        let mut pts = vec![a, b];
        for level in 0..3 {
            let mut next = vec![pts[0]];
            for w in pts.windows(2) {
                let d = w[1] - w[0];
                let m = (w[0] + w[1]) * 0.5 + d.perp() * rng.range(-amp, amp) / (level as f64 + 1.0);
                next.extend([m, w[1]]);
            }
            pts = next;
        }
        let pts = subdivide(&pts, 120.0);
        let ok = pts.windows(2).all(|w| {
            let n = (w[0].distance(w[1]) / 40.0).ceil() as usize;
            (0..=n).all(|i| ground.is_free(w[0].lerp(w[1], i as f64 / n.max(1) as f64), 6.0))
        });
        if ok {
            return Some(pts);
        }
    }
    None
}
