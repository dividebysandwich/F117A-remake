use serde::{Deserialize, Serialize};

use crate::Sim;
use crate::math::{approx, s16};
use crate::types::*;

/// Result of the nearest-tile-object search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileHit {
    /// Current shape (the damage-list shape for destroyed tiles).
    pub shape: i32,
    /// Original tile shape.
    pub base_shape: i32,
    pub dist: i32,
    /// World position (32-bit, y north).
    pub wx: i32,
    pub wy: i32,
    pub level: u8,
    pub index: u16,
    pub col: i32,
    pub row: i32,
    /// Tile record found in the damage list.
    pub damaged: bool,
}

impl TileHit {
    /// Map position of the tile object.
    pub fn map_pos(&self) -> (i32, i32) {
        (s16(self.wx >> 5), s16(-(s16(self.wy >> 5) - 0x8000)))
    }
}

fn scale(lv: usize, c: u32) -> u32 {
    if lv == 1 { c } else { c >> 2 }
}

const DIR_DX: [i32; 9] = [-1, 1, 1, -1, 0, 1, 0, -1, 0];
const DIR_DY: [i32; 9] = [1, 1, -1, -1, 1, 0, -1, 0, 0];

impl Sim {
    /// Nearest tile object to world point (X, Y): LOD levels 1..2, 9 neighbour cells, candidates with
    /// `buf7[shape] != 0`, Manhattan distance.
    pub(crate) fn find_nearest_tile(&self, x: i32, y: i32) -> Option<TileHit> {
        let tiles = self.tiles.as_ref()?;
        let mut best = None;
        let mut bd = 0x7FFF;
        for lv in [1usize, 2] {
            for k in 0..9 {
                let fx = scale(lv, x as u32);
                let fy = scale(lv, y as u32);
                let (frx, fry) = ((fx & 0xFFF) as i32, (fy & 0xFFF) as i32);
                let (dx, dy) = (DIR_DX[k], DIR_DY[k]);
                let sx = s16((dx << 12) - frx + 0x800);
                let sy = s16((dy << 12) - fry + 0x800);
                let gx = s16(((fx >> 12) & 0xFFFF) as i32 + dx);
                let gy = s16(((fy >> 12) & 0xFFFF) as i32 + dy);
                let cell = tiles.cell(lv, gx, gy);
                if cell < 0 {
                    continue;
                }
                for (idx, &(ox, oy, sh)) in tiles.objects_of(lv, cell).iter().enumerate() {
                    let dmg = self.tile_damage.iter().find(|t| {
                        t.level as usize == lv
                            && t.index as usize == idx
                            && t.col == gx
                            && t.row == gy
                    });
                    let shape = dmg.map_or(sh as i32, |t| t.shape);
                    if self.buf7.get((shape & 0x7F) as usize).copied().unwrap_or(0) == 0 {
                        continue;
                    }
                    let (mut tx, mut ty) = (s16(ox as i32 + sx), s16(oy as i32 + sy));
                    let mut d = s16(tx.abs() + ty.abs());
                    if lv == 1 {
                        d >>= 2;
                    } else {
                        tx = s16(tx << 2);
                        ty = s16(ty << 2);
                    }
                    if d < bd {
                        bd = d;
                        best = Some(TileHit {
                            shape,
                            base_shape: sh as i32,
                            dist: d,
                            wx: x.wrapping_add(tx),
                            wy: y.wrapping_add(ty),
                            level: lv as u8,
                            index: idx as u16,
                            col: gx,
                            row: gy,
                            damaged: dmg.is_some(),
                        });
                    }
                }
            }
        }
        best
    }

    /// World object at a map point. Returns the WLD object at the nearest tile
    /// object, 0 for a plain tile (written into scratch object 0), -1 if there is no tile.
    ///
    /// Without theater tile data (tests) the nearest world object within 0x100 map units is
    /// used instead.
    pub(crate) fn find_object_at(&mut self, x: i32, y: i32) -> i32 {
        if self.tiles.is_none() {
            let mut best = (-1, 0x100);
            for (i, o) in self.objects.iter().enumerate().skip(3) {
                let d = approx(o.x - x, o.y - y);
                if d < best.1 {
                    best = (i as i32, d);
                }
            }
            if best.0 >= 0 {
                let o = self.objects[best.0 as usize];
                self.last_tile = Some(TileHit {
                    shape: o.shape & 0x7F,
                    base_shape: o.shape & 0x7F,
                    dist: 0,
                    wx: o.x << 5,
                    wy: (0x8000 - o.y) << 5,
                    level: 0,
                    index: best.0 as u16,
                    col: -1,
                    row: -1,
                    damaged: o.flags & oflags::DESTROYED != 0,
                });
            } else {
                self.last_tile = None;
            }
            return best.0;
        }
        let res = self.find_nearest_tile(x << 5, (0x8000 - y) << 5);
        self.last_tile = res;
        let Some(t) = res else { return -1 };
        let (mx, my) = t.map_pos();
        let (mx, my) = (mx & 0xFFFF, my & 0xFFFF);
        for i in 1..self.objects.len() {
            if self.objects[i].x == mx && self.objects[i].y == my {
                return i as i32;
            }
        }
        if let Some(o) = self.objects.get_mut(0) {
            o.x = mx;
            o.y = my;
            o.shape = (t.base_shape & 0x7F) + 0x100;
            o.name = 0;
        }
        if self.smoke_src == 0 {
            self.smoke_src = -1;
        }
        0
    }

    /// The tile is not already rubble.
    pub(crate) fn tile_is_intact(&self, t: &Option<TileHit>) -> bool {
        match t {
            Some(t) => {
                let s = t.shape & 0xFF;
                !(t.damaged && (s == self.land_rubble || s == self.water_rubble))
            }
            None => true,
        }
    }

    fn rubble_for(&self, water: bool) -> i32 {
        if water { self.water_rubble } else { self.land_rubble }
    }

    fn model_has_wreck(&self, shape: i32) -> bool {
        self.tiles
            .as_ref()
            .and_then(|t| t.model_attr.get((shape & 0x7F) as usize))
            .is_some_and(|a| a & 0x20 != 0)
    }

    /// Shape of object `i` once destroyed: the model's own wreck state, else land / water rubble.
    pub(crate) fn wreck_shape(&self, i: i32) -> i32 {
        let o = self.obj(i);
        if self.model_has_wreck(o.shape) {
            (o.shape & 0xFF) | 0x100
        } else {
            self.rubble_for(self.is_water(i)) | 0x100
        }
    }

    /// Record a new shape for a tile object in the damage list.
    fn set_tile_shape(&mut self, t: TileHit, shape: i32) {
        if t.level == 0 {
            return;
        }
        let rec = TileDamage { level: t.level, index: t.index, col: t.col, row: t.row, shape };
        if let Some(e) = self
            .tile_damage
            .iter_mut()
            .find(|e| e.level == t.level && e.index == t.index && e.col == t.col && e.row == t.row)
        {
            e.shape = shape;
        } else {
            self.tile_damage.push(rec);
        }
        self.emit(SimEvent::TileDestroyed(rec));
    }

    /// Destroy ground object `i`, the only kill path for ground objects.
    pub(crate) fn destroy_ground(&mut self, i: i32) {
        if i < 0 || i as usize >= self.objects.len() {
            return;
        }
        let iu = i as usize;
        let mut ev = 1;
        if self.objects[iu].flags & oflags::DESTROYED == 0 {
            if self.objects[iu].flags & oflags::RADAR_ID != 0 {
                self.radar_ids -= 1;
            }
            let o = self.objects[iu];
            self.destroyed_at.push((o.x, o.y));
            let res = if self.tiles.is_some() {
                self.find_nearest_tile(o.x << 5, (0x8000 - o.y) << 5)
            } else {
                None
            };
            let shape;
            if i != 0 {
                if o.unit_type == 0 {
                    ev = 12;
                }
                self.objects[iu].flags |= oflags::DESTROYED;
                self.objects[iu].unit_type = 0;
                for k in 0..2 {
                    if self.targets[k].mission_type == 2 && self.targets[k].object == i {
                        self.mark_target_reached(k);
                        ev |= if k == 0 { 0x80 } else { 0x40 };
                        self.sound(4, 2);
                        self.cue_timers[k] = 4 * self.fs;
                    }
                }
                self.log_event(ev, i);
                shape = self.wreck_shape(i);
                self.display_shape[iu] = shape;
                self.emit(SimEvent::GroundDestroyed { object: iu, shape });
            } else {
                // Scratch tile object.
                let t = self.last_tile;
                if !self.tile_is_intact(&t) {
                    return;
                }
                let base = t.map_or(o.shape & 0x7F, |t| t.base_shape);
                if let Some(b) = self.buf8.get_mut((base & 0x7F) as usize) {
                    *b = b.wrapping_add(1);
                }
                self.log_event(2, base);
                let s = if self.model_has_wreck(o.shape) {
                    o.shape & 0xFF
                } else {
                    self.rubble_for(self.is_water(0))
                };
                shape = s | 0x100;
                self.objects[0].shape = shape;
                self.emit(SimEvent::GroundDestroyed { object: 0, shape });
            }
            let res = if i == 0 { self.last_tile } else { res };
            if let Some(t) = res {
                self.set_tile_shape(t, shape);
            }
        }
        self.smoke_src = i;
        self.sound(5, 2);
        if self.hud_mode == HudMode::Gnd && i == self.gnd_lock {
            self.targeting.target_killed = true;
        }
        if self.diff < 2 {
            self.alarm();
        }
    }

    /// Destroy flight unit `j`.
    pub(crate) fn destroy_aircraft(&mut self, j: usize) {
        if j >= self.flights.len() {
            return;
        }
        let u = self.flights[j];
        if u.flags & fflags::DESTROYED == 0 {
            let spec = (u.spec.max(0) as usize).min(self.aircraft_types.len() - 1);
            self.aircraft_types[spec].kills = self.aircraft_types[spec].kills.wrapping_add(1);
            if u.flags & fflags::VISUAL_ID != 0 {
                self.visual_ids -= 1;
            }
            if j as i32 == self.escort_leader {
                self.escort_leader = -1;
            }
            self.flights[j].flags |= fflags::DESTROYED;
            self.smoke_src = -1;
            self.wreck = Wreck { x: u.x, y: u.y, alt: u.alt, vel: 0x80 };
            let mut ev = 3;
            if (5..=8).contains(&self.targets[0].mission_type) && j == 0 {
                self.mark_target_reached(0);
                ev = 0x83;
            }
            self.log_event(ev, u.spec + if u.flags & fflags::SAW_PLAYER != 0 { 0x80 } else { 0 });
            if u.speed == 0 {
                self.flights[j].flags &= 0x1C1;
            }
            self.emit(SimEvent::AircraftDestroyed { unit: j, spec: u.spec });
            self.drone_killed(j);
        }
        self.sound(5, 2);
        if self.hud_mode == HudMode::Air && j as i32 == self.air_lock {
            self.targeting.target_killed = true;
        }
    }

    /// The player is hit (`missile`: by a missile's warhead).
    pub(crate) fn player_hit(&mut self, missile: bool) {
        if self.training() {
            return;
        }
        if missile && self.opts.realistic_damage {
            self.missile_damage();
            return;
        }
        let before = self.damage;
        let mut bits = 0u8;
        for _ in 0..=self.diff {
            let b = 1u8 << self.rng.random(8).clamp(0, 7);
            bits |= b;
            self.damage |= b;
            self.hits += 1;
        }
        if self.opts.realistic_damage {
            self.new_damage(bits, before);
        }
        self.emit(SimEvent::PlayerHit { bits });
        self.emit(SimEvent::DamageFlash);
        self.sound(4, 2);
    }
}
