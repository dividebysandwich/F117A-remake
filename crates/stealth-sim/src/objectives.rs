use crate::Sim;
use crate::math::{abs16, cdiv, clamp_q, s16};
use crate::types::*;

/// A helicopter's base landing area: the whole airfield beside the runway (half width, map
/// units << 5; the flight model's `landing_corridor`, keep in step).
const HELI_BASE_HALF_WIDTH: i32 = 0x200;
/// Insertion (mission type 9): landed within this many map units of the target, stopped for
/// this many seconds while the troops get out.
pub const INSERT_RADIUS: i32 = 0x20;
pub const INSERT_SECONDS: i32 = 10;

impl Sim {
    /// Mark objective `k` (0 primary, 1 secondary) as done; false if it already was.
    pub(crate) fn mark_target_reached(&mut self, k: usize) -> bool {
        let bit = pflags::PRIMARY_DONE >> k;
        if self.pl.pflags & bit != 0 {
            return false;
        }
        let t = self.pl.targets[k];
        if matches!(t.mission_type, 3 | 4) {
            self.log_event(0x0B | if k == 1 { 0x40 } else { 0x80 }, t.object);
        }
        if k == 1 {
            self.msg("Second. target");
            self.pl.waypoint = 1;
        } else {
            self.msg("Primary target");
            self.pl.waypoint = 2;
        }
        self.pl.pflags |= bit;
        if self.pl.pflags & (pflags::PRIMARY_DONE | pflags::SECONDARY_DONE)
            == pflags::PRIMARY_DONE | pflags::SECONDARY_DONE
        {
            self.pl.waypoint = 3;
        }
        self.emit(SimEvent::ObjectiveDone { primary: k == 0 });
        true
    }

    /// Insertions (mission type 9): the helicopter stopped on the ground near a target for
    /// `INSERT_SECONDS` lets the troops out there.
    fn insertions(&mut self) {
        let landed = self.pl.helicopter && self.pl.on_ground && self.pl.p.knots <= 1;
        for k in 0..2 {
            let t = self.pl.targets[k];
            let near = landed && t.mission_type == 9 && t.object >= 0 && {
                let o = self.obj(t.object);
                let (dx, dy) = (s16(self.pl.p.mx - o.x), s16(self.pl.p.my - o.y));
                dx * dx + dy * dy <= INSERT_RADIUS * INSERT_RADIUS
            };
            if !near {
                self.pl.unload[k] = 0;
                continue;
            }
            self.pl.unload[k] += 1;
            if self.pl.unload[k] == INSERT_SECONDS * self.fs && self.mark_target_reached(k) {
                self.msg("Troops inserted");
            }
        }
    }

    /// End the mission with `outcome` (ignored once ended, or after ejection unless 0 or 8,
    /// captured).
    pub(crate) fn finalize(&mut self, outcome: u8) {
        if self.ended.is_some() || (self.pl.eject != 0 && outcome != 0 && outcome != 8) {
            return;
        }
        if outcome == 0 && self.pl.eject == 0 {
            self.pl.landing_type = 3;
        }
        if self.campaign.is_some() {
            if !self.player_absent() {
                let e = MissionEnd {
                    outcome,
                    landing_type: self.pl.landing_type,
                    map_x: self.pl.p.mx,
                    map_y: self.pl.p.my,
                    damage: self.pl.damage,
                    hits: self.pl.hits,
                };
                self.finalize_sortie(e);
            }
            return;
        }
        let e = MissionEnd {
            outcome,
            landing_type: self.pl.landing_type,
            map_x: self.pl.p.mx,
            map_y: self.pl.p.my,
            damage: self.pl.damage,
            hits: self.pl.hits,
        };
        self.log_event(8, 0);
        self.ended = Some(e);
        self.emit(SimEvent::MissionEnd(e));
    }

    /// End the mission from outside (Alt-Q = 7: nothing written).
    pub fn quit(&mut self) {
        if self.ended.is_none() {
            let e = MissionEnd {
                outcome: 7,
                landing_type: self.pl.landing_type,
                map_x: self.pl.p.mx,
                map_y: self.pl.p.my,
                damage: self.pl.damage,
                hits: self.pl.hits,
            };
            self.ended = Some(e);
        }
    }

    /// Eject (Shift-F10): fails at high speed or attitude, otherwise the chute comes down.
    pub(crate) fn do_eject(&mut self) {
        if self.pl.eject != 0 {
            return;
        }
        self.sound(5, 2);
        self.sound(0x14, 2);
        let r = self.rng.random(300);
        let fail = (abs16(self.pl.p.roll) >> 5) + (abs16(self.pl.p.pitch) >> 5) + self.pl.p.knots
            > 500 + r;
        if fail {
            self.emit(SimEvent::Ejected { success: false });
            self.finalize(6);
        } else {
            self.pl.landing_type = 2;
            self.emit(SimEvent::Ejected { success: true });
        }
        self.pl.eject = 1;
        self.pl.chute = (self.pl.p.mx, self.pl.p.my, self.pl.p.view_z + 8);
    }

    pub(crate) fn update_objectives(&mut self, ps: &PlayerState) {
        let fs = self.fs;
        // Delayed "target destroyed" cues.
        for k in 0..2 {
            if self.pl.cue_timers[k] > 0 {
                self.pl.cue_timers[k] -= 1;
                if self.pl.cue_timers[k] == 0 {
                    self.sound(if k == 0 { 0x29 } else { 0x2A }, 2);
                }
            }
        }
        // Deadline.
        if self.mission_tick >= self.pl.deadline && self.pl.deadline > 0 {
            for k in 0..2 {
                let t = self.pl.targets[k];
                if t.code & 2 != 0 && !self.pl.time_expired_shown {
                    self.pl.time_expired_shown = true;
                    self.msg("Time expired");
                }
                if !self.pl.on_ground
                    && t.code & 0x20 != 0
                    && !self.pl.deadline_done[k]
                    && t.object >= 0
                    && (t.object as usize) < self.objects.len()
                {
                    self.pl.deadline_done[k] = true;
                    let i = t.object as usize;
                    if self.objects[i].flags & oflags::DESTROYED == 0 {
                        let o = self.objects[i];
                        self.objects[i].flags |= oflags::DESTROYED;
                        let shape = self.land_rubble | 0x100;
                        self.display_shape[i] = shape;
                        self.emit(SimEvent::GroundDestroyed { object: i, shape, by: self.credit });
                        if let Some(th) = self.find_nearest_tile(o.x << 5, (0x8000 - o.y) << 5) {
                            let rec = TileDamage {
                                level: th.level,
                                index: th.index,
                                col: th.col,
                                row: th.row,
                                shape,
                            };
                            self.tile_damage.push(rec);
                            self.emit(SimEvent::TileDestroyed(rec));
                        }
                    }
                }
            }
        }
        // Ejection: chute descent, then the rescue over enemy territory.
        if self.pl.eject != 0 {
            if self.csar.is_some() {
                self.update_csar(ps);
                return;
            }
            self.pl.eject += 1;
            let dz = clamp_q(0x20 - self.pl.eject, cdiv(-0x60, fs), cdiv(0x80, fs));
            self.pl.chute.2 += dz;
            if self.pl.chute.2 <= 0 {
                self.pl.chute.2 = 0;
                // A campaign has no rescue (and no capture): the sortie just ends.
                if self.mission_tick & 7 == 0 && (self.campaign.is_some() || !self.start_csar()) {
                    self.finalize(0);
                }
            }
            return;
        }
        let lm = self.opts.landing as i32;
        // Landing corridor (every 8 frames the base is refreshed; corridor test every frame).
        let b = self.obj(self.pl.nearest_base);
        let mut ground_alt = 0;
        if self.pl.nearest_base >= 0 && (self.pl.nearest_base_dist < 0x200 || self.pl.on_ground) {
            let carrier = b.flags & oflags::CARRIER != 0;
            let (mut hw, mut hl) = if carrier {
                (0x100, 0x3C0)
            } else {
                let hw = if self.pl.helicopter { HELI_BASE_HALF_WIDTH } else { 0xA0 };
                (hw, if b.flags & oflags::DISABLED != 0 { 0x400 } else { 0x800 })
            };
            if carrier {
                ground_alt = 0x80;
            }
            if self.opts.landing == LandingMode::Easy {
                hw += 0x80;
                hl += 0x100;
            }
            self.pl.corridor = s16(self.pl.p.mx - b.x).abs() <= hw >> 5
                && s16(self.pl.p.my - b.y).abs() <= hl >> 5;
            if !self.pl.corridor {
                ground_alt = 0;
            }
        } else {
            self.pl.corridor = false;
        }
        let touchdown = self.pl.on_ground && !self.pl.prev_on_ground;
        // A helicopter's flight model judges its touchdowns (anywhere).
        if touchdown && self.pl.corridor && !self.pl.helicopter {
            let lim = if b.flags & oflags::CARRIER != 0 { 0x100 } else { 0x80 };
            let sink = (-(lm * ps.climb_rate)) & 0xFFFF;
            let bad =
                sink > lim || (lm != 0 && (ps.gear_up || abs16(self.pl.p.roll) > ((24 / lm) << 8)));
            if bad {
                self.sound(4, 2);
                self.finalize(5);
                return;
            }
        }
        let _ = ground_alt;
        self.insertions();
        if self.pl.corridor {
            if self.pl.p.knots <= 1 && self.pl.on_ground && self.frame & 15 == 0 {
                for k in 0..2 {
                    let t = self.pl.targets[k];
                    if t.object == self.pl.nearest_base
                        && t.mission_type == 4
                        && self.mission_tick < self.pl.deadline
                        && self.mark_target_reached(k)
                    {
                        self.msg("Cargo delivered");
                        // Empty bay k (the generator loads the target's
                        // equipment there).
                        if let Some(b) =
                            self.pl.bays.iter_mut().find(|b| b.0 == SPECIAL_EQUIPMENT && b.1 != 0)
                        {
                            b.1 = 0;
                        }
                    }
                }
                if b.flags & 0x500 != 0
                    && self.pl.armed_landing != 0
                    && b.flags & oflags::DISABLED == 0
                {
                    let old = self.pl.armed_landing;
                    self.pl.armed_landing += 1;
                    if old == 1 {
                        self.msg("Safe Landing");
                        self.sound(0x2B, 2);
                    }
                    if cdiv(16, fs) < self.pl.armed_landing {
                        // Campaign: on a friendly base the sortie goes on (rearm, or end it).
                        if self.campaign.is_some() {
                            self.pl.armed_landing = 0;
                            let object = self.pl.nearest_base.max(0) as usize;
                            self.emit(SimEvent::LandedAtBase { object });
                            return;
                        }
                        self.finalize(0);
                        return;
                    }
                }
            }
        } else if self.pl.on_ground {
            if (lm != 0 || self.pl.hits > 4 || ps.fuel == 0) && self.pl.p.knots > 50 {
                self.sound(4, 2);
                self.finalize(1);
                return;
            }
        } else {
            self.pl.armed_landing = 1;
        }
        // Object collision.
        if ps.collided && !self.pl.external_view {
            if lm != 0 && self.pl.p.alt != 0 {
                self.sound(4, 2);
                self.finalize(2);
            } else {
                self.emit(SimEvent::CollisionBounce);
            }
        }
    }
}
