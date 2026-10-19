//! Per-theater look of the scenery: tree mix, forest cover, building colours and roofs.
//! The desert theaters (Libya, Persian Gulf, Middle East, Desert Storm) get none.
//!
//! Colour bytes are model colours (0xA0.. ramps, identical in the green theaters' palettes):
//! A0 olive greens, B0 teal, C0 cream to ochre/brown, D0 pink to dark red, E0 greys,
//! F0 blue-greys. Higher entries of a ramp are darker; haze fades towards the ramp start.

use bevy::math::DVec2;

use super::network::{Building, Kind, Rng, Roof, TownKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tree {
    Conifer,
    Broadleaf,
    Palm,
}

pub struct Style {
    pub seed: u64,
    /// Town candidates kept (one per 6.5 km square).
    pub towns: f64,
    pub town_size: f64,
    /// Weights of villages, towns and cities among the generated towns.
    pub town_mix: [f64; 3],
    /// Street spacing factor (wider blocks between the panel buildings of Soviet-built towns).
    pub street_scale: f64,
    /// Town centres of five to sixteen storey panel blocks, private houses around them.
    pub soviet: bool,
    /// Share of the lots built on at a town's edge (more towards the centre).
    pub lot_fill: f64,
    /// Farmland patchwork with shelterbelts on the open land (`fields.rs`).
    pub fields: bool,
    /// Chance of a farmstead per stretch of road.
    pub farms: f64,
    pub pole_lines: f64,
    /// Forest cover: noise level above which trees grow (0..1, lower = more forest).
    pub forest: f64,
    pub trees: &'static [(Tree, f64)],
    pub walls: &'static [u8],
    pub roofs: &'static [u8],
    /// Weights of flat, gable and hip roofs on houses.
    pub roof_kinds: [f64; 3],
    pub barn: &'static [u8],
}

impl Style {
    pub fn for_theater(id: &str, index: usize) -> Option<Style> {
        let seed = 0x5CE7_E000 + index as u64;
        let base = Style {
            seed,
            towns: 0.5,
            town_size: 1.0,
            town_mix: [0.68, 0.25, 0.07],
            street_scale: 1.0,
            soviet: false,
            lot_fill: 0.35,
            fields: false,
            farms: 0.5,
            pole_lines: 0.5,
            forest: 0.5,
            trees: &[],
            walls: &[],
            roofs: &[],
            roof_kinds: [0.1, 0.6, 0.3],
            barn: &[],
        };
        let s = match id {
            // North Cape: pine forest, red and yellow wooden houses.
            "nc" => Style {
                towns: 0.35,
                town_size: 0.8,
                farms: 0.35,
                pole_lines: 0.6,
                forest: 0.42,
                trees: &[(Tree::Conifer, 0.85), (Tree::Broadleaf, 0.15)],
                walls: &[0xD6, 0xD7, 0xC4, 0xE1, 0xCB],
                roofs: &[0xE9, 0xEB, 0xD9, 0xF7],
                roof_kinds: [0.05, 0.9, 0.05],
                barn: &[0xD7, 0xD8],
                ..base
            },
            // Central Europe: mixed woods, red tile roofs.
            "ce" => Style {
                towns: 0.65,
                town_size: 1.0,
                farms: 0.55,
                pole_lines: 0.5,
                forest: 0.56,
                trees: &[(Tree::Broadleaf, 0.55), (Tree::Conifer, 0.45)],
                walls: &[0xC1, 0xC2, 0xE1, 0xE3, 0xD3, 0xC4],
                roofs: &[0xD8, 0xD9, 0xD6, 0xCD, 0xE9],
                roof_kinds: [0.1, 0.65, 0.25],
                barn: &[0xCD, 0xE5, 0xD8],
                ..base
            },
            // Vietnam: jungle and palms, rust and grey roofs, small houses.
            "vn" => Style {
                towns: 0.5,
                town_size: 0.75,
                farms: 0.6,
                pole_lines: 0.3,
                forest: 0.44,
                trees: &[(Tree::Broadleaf, 0.6), (Tree::Palm, 0.4)],
                walls: &[0xC1, 0xC3, 0xC5, 0xE2, 0xCA],
                roofs: &[0xCC, 0xCD, 0xCE, 0xE8, 0xC9],
                roof_kinds: [0.15, 0.4, 0.45],
                barn: &[0xCC, 0xCD],
                ..base
            },
            // Cuba: palms, pastel walls, flat and tiled roofs.
            "cu" => Style {
                towns: 0.55,
                town_size: 0.9,
                farms: 0.45,
                pole_lines: 0.5,
                forest: 0.6,
                trees: &[(Tree::Palm, 0.55), (Tree::Broadleaf, 0.45)],
                walls: &[0xC1, 0xC4, 0xD2, 0xB2, 0xE1, 0xC6],
                roofs: &[0xD8, 0xCC, 0xE2, 0xE4],
                roof_kinds: [0.45, 0.25, 0.3],
                barn: &[0xCC, 0xE4],
                ..base
            },
            // Korea: mountain forest, grey and blue tiled hip roofs.
            "ko" => Style {
                towns: 0.6,
                town_size: 0.95,
                farms: 0.5,
                pole_lines: 0.6,
                forest: 0.5,
                trees: &[(Tree::Conifer, 0.5), (Tree::Broadleaf, 0.5)],
                walls: &[0xE1, 0xE3, 0xC1, 0xC2, 0xE5],
                roofs: &[0xF6, 0xF8, 0xE9, 0xEB, 0xB9],
                roof_kinds: [0.25, 0.25, 0.5],
                barn: &[0xE5, 0xF6],
                ..base
            },
            // Ukraine: steppe farmland with shelterbelts, poplars and acacias, white and
            // cream houses with grey, green and red-brown roofs, panel blocks in the towns.
            "ua" => Style {
                towns: 0.42,
                town_size: 0.9,
                town_mix: [0.86, 0.12, 0.02],
                street_scale: 1.6,
                soviet: true,
                lot_fill: 0.7,
                fields: true,
                farms: 0.3,
                pole_lines: 0.6,
                forest: 0.8,
                trees: &[(Tree::Broadleaf, 0.85), (Tree::Conifer, 0.15)],
                walls: &[0xE1, 0xE1, 0xE2, 0xC1, 0xC2, 0xE3],
                roofs: &[0xE8, 0xE9, 0xA6, 0xD9, 0xB6, 0xEB],
                roof_kinds: [0.08, 0.42, 0.5],
                barn: &[0xE5, 0xE3, 0xE8],
                ..base
            },
            _ => return None,
        };
        Some(s)
    }

    pub fn tree(&self, rng: &mut Rng) -> Tree {
        let w: Vec<f64> = self.trees.iter().map(|t| t.1).collect();
        self.trees[rng.weighted(&w)].0
    }

    fn roof(&self, rng: &mut Rng) -> Roof {
        [Roof::Flat, Roof::Gable, Roof::Hip][rng.weighted(&self.roof_kinds)]
    }

    /// A town building for a lot; `core` is 1 at the centre, 0 at the edge.
    pub fn building(&self, rng: &mut Rng, town: TownKind, core: f64) -> Building {
        let base = Building {
            p: DVec2::ZERO,
            dir: DVec2::X,
            kind: Kind::House,
            l: 10.0,
            w: 8.0,
            h: 3.0,
            roof: self.roof(rng),
            wall: rng.pick(self.walls),
            roof_colour: rng.pick(self.roofs),
            seed: rng.u64(),
        };
        let big = match town {
            TownKind::Village => 0.0,
            TownKind::Town => (core - 0.45).max(0.0) * 1.6,
            TownKind::City => (core - 0.2).max(0.0) * 1.5,
        };
        if town == TownKind::City && core > 0.7 && rng.chance(0.45) {
            let s = rng.range(18.0, 30.0) as f32;
            let h = rng.range(35.0, 30.0 + 80.0 * core) as f32;
            return Building { kind: Kind::Tower, l: s, w: s * rng.range(0.7, 1.0) as f32, h, roof: Roof::Flat, wall: rng.pick(&[0xF2, 0xF4, 0xE3, 0xB4]), roof_colour: 0xE7, ..base };
        }
        if rng.chance(big) {
            let floors = rng.range(3.0, 4.0 + 8.0 * core).floor() as f32;
            return Building {
                kind: Kind::Apartment,
                l: rng.range(26.0, 55.0) as f32,
                w: rng.range(11.0, 15.0) as f32,
                h: floors * 3.0,
                roof: if rng.chance(0.75) { Roof::Flat } else { base.roof },
                roof_colour: if rng.chance(0.6) { 0xE7 } else { base.roof_colour },
                ..base
            };
        }
        let floors = if rng.chance(0.35) { 2.0 } else { 1.0 };
        Building { l: rng.range(8.0, 13.0) as f32, w: rng.range(7.0, 10.0) as f32, h: floors * 2.9 + 0.3, ..base }
    }

    /// Whether a point of a town (`core` 1 at the centre) is a district of panel blocks.
    pub fn panel_district(&self, town: TownKind, core: f64) -> bool {
        self.soviet && core > if town == TownKind::City { 0.2 } else { 0.5 } && town != TownKind::Village
    }

    /// A panel block: a five-storey slab, a nine to sixteen storey slab or point tower.
    pub fn panel(&self, rng: &mut Rng, core: f64, max_len: f64) -> Building {
        let floors = [5.0, 5.0, 9.0, 9.0, 12.0, 16.0][(rng.f() * (2.0 + 4.0 * core)).min(5.0) as usize] as f32;
        let tower = floors > 9.0 && rng.chance(0.35);
        Building {
            p: DVec2::ZERO,
            dir: DVec2::X,
            kind: Kind::Apartment,
            l: if tower { rng.range(22.0, 28.0) } else { rng.range(0.55, 1.0) * max_len } as f32,
            w: if tower { rng.range(20.0, 26.0) } else { rng.range(12.0, 14.5) } as f32,
            h: floors * 2.8,
            roof: Roof::Flat,
            wall: rng.pick(&[0xE1, 0xE2, 0xE3, 0xC1, 0xE4, 0xF2]),
            roof_colour: 0xE7,
            seed: rng.u64(),
        }
    }

    pub fn church(&self, rng: &mut Rng, p: DVec2, dir: DVec2) -> Building {
        Building {
            p,
            dir,
            kind: Kind::Church,
            l: 28.0,
            w: 13.0,
            h: 10.0,
            roof: Roof::Gable,
            wall: rng.pick(&[0xE1, 0xC1, 0xE3]),
            roof_colour: rng.pick(self.roofs),
            seed: rng.u64(),
        }
    }

    /// Farmhouse, barn and maybe a silo.
    pub fn farm(&self, rng: &mut Rng, p: DVec2, dir: DVec2) -> Vec<Building> {
        let mut house = self.building(rng, TownKind::Village, 0.0);
        house.p = p;
        house.dir = dir;
        let barn = Building {
            p: p + dir.perp() * 24.0 + dir * rng.range(-10.0, 10.0),
            dir,
            kind: Kind::Barn,
            l: rng.range(20.0, 32.0) as f32,
            w: rng.range(11.0, 16.0) as f32,
            h: rng.range(4.5, 6.5) as f32,
            roof: Roof::Gable,
            wall: rng.pick(self.barn),
            roof_colour: rng.pick(self.roofs),
            seed: rng.u64(),
        };
        let mut out = vec![house, barn];
        if rng.chance(0.4) {
            out.push(Building {
                p: barn.p + dir * (barn.l as f64 * 0.5 + 6.0),
                dir,
                kind: Kind::Silo,
                l: 6.0,
                w: 6.0,
                h: rng.range(10.0, 16.0) as f32,
                roof: Roof::Flat,
                wall: rng.pick(&[0xE2, 0xE4, 0xF3]),
                roof_colour: 0xE6,
                seed: rng.u64(),
            });
        }
        out
    }
}
