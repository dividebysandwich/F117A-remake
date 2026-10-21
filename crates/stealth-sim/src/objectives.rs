use crate::Sim;
use crate::math::{abs16, cdiv, clamp_q, s16};
use crate::types::*;

impl Sim {
    /// Mark objective `k` (0 primary, 1 secondary) as done; false if it already was.
    pub(crate) fn mark_target_reached(&mut self, k: usize) -> bool {
        let bit = pflags::PRIMARY_DONE >> k;
        if self.pflags & bit != 0 {
            return false;
        }
        let t = self.targets[k];
        if matches!(t.mission_type, 3 | 4) {
            self.log_event(0x0B | if k == 1 { 0x40 } else { 0x80 }, t.object);
        }
        if k == 1 {
            self.msg("Second. target");
            self.waypoint = 1;
        } else {
            self.msg("Primary target");
            self.waypoint = 2;
        }
        self.pflags |= bit;
        if self.pflags & (pflags::PRIMARY_DONE | pflags::SECONDARY_DONE)
            == pflags::PRIMARY_DONE | pflags::SECONDARY_DONE
        {
            self.waypoint = 3;
        }
        self.emit(SimEvent::ObjectiveDone { primary: k == 0 });
        true
    }

    /// End the mission with `outcome` (ignored once ended, or after ejection unless 0 or 8,
    /// captured).
    pub(crate) fn finalize(&mut self, outcome: u8) {
        if self.ended.is_some() || (self.eject != 0 && outcome != 0 && outcome != 8) {
            return;
        }
        if outcome == 0 && self.eject == 0 {
            self.landing_type = 3;
        }
        if self.campaign.is_some() {
            if !self.player_absent() {
                let e = MissionEnd {
                    outcome,
                    landing_type: self.landing_type,
                    map_x: self.p.mx,
                    map_y: self.p.my,
                    damage: self.damage,
                    hits: self.hits,
                };
                self.finalize_sortie(e);
            }
            return;
        }
        let e = MissionEnd {
            outcome,
            landing_type: self.landing_type,
            map_x: self.p.mx,
            map_y: self.p.my,
            damage: self.damage,
            hits: self.hits,
        };
        self.log_event(8, 0);
        self.ended = Some(e);
        self.emit(SimEvent::MissionEnd(e));
    }

    /// End the mission from outside (Alt-Q = 7: nothing written, like the original).
    pub fn quit(&mut self) {
        if self.ended.is_none() {
            let e = MissionEnd {
                outcome: 7,
                landing_type: self.landing_type,
                map_x: self.p.mx,
                map_y: self.p.my,
                damage: self.damage,
                hits: self.hits,
            };
            self.ended = Some(e);
        }
    }

    /// Eject (Shift-F10): fails at high speed or attitude, otherwise the chute comes down.
    pub(crate) fn do_eject(&mut self) {
        if self.eject != 0 {
            return;
        }
        self.sound(5, 2);
        self.sound(0x14, 2);
        let r = self.rng.random(300);
        let fail = (abs16(self.p.roll) >> 5) + (abs16(self.p.pitch) >> 5) + self.p.knots > 500 + r;
        if fail {
            self.emit(SimEvent::Ejected { success: false });
            self.finalize(6);
        } else {
            self.landing_type = 2;
            self.emit(SimEvent::Ejected { success: true });
        }
        self.eject = 1;
        self.chute = (self.p.mx, self.p.my, self.p.view_z + 8);
    }

    pub(crate) fn update_objectives(&mut self, ps: &PlayerState) {
        let fs = self.fs;
        // Delayed "target destroyed" cues.
        for k in 0..2 {
            if self.cue_timers[k] > 0 {
                self.cue_timers[k] -= 1;
                if self.cue_timers[k] == 0 {
                    self.sound(if k == 0 { 0x29 } else { 0x2A }, 2);
                }
            }
        }
        // Deadline.
        if self.mission_tick >= self.deadline && self.deadline > 0 {
            for k in 0..2 {
                let t = self.targets[k];
                if t.code & 2 != 0 && !self.time_expired_shown {
                    self.time_expired_shown = true;
                    self.msg("Time expired");
                }
                if !self.on_ground
                    && t.code & 0x20 != 0
                    && !self.deadline_done[k]
                    && t.object >= 0
                    && (t.object as usize) < self.objects.len()
                {
                    self.deadline_done[k] = true;
                    let i = t.object as usize;
                    if self.objects[i].flags & oflags::DESTROYED == 0 {
                        let o = self.objects[i];
                        self.objects[i].flags |= oflags::DESTROYED;
                        let shape = self.land_rubble | 0x100;
                        self.display_shape[i] = shape;
                        self.emit(SimEvent::GroundDestroyed { object: i, shape });
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
        if self.eject != 0 {
            if self.csar.is_some() {
                self.update_csar(ps);
                return;
            }
            self.eject += 1;
            let dz = clamp_q(0x20 - self.eject, cdiv(-0x60, fs), cdiv(0x80, fs));
            self.chute.2 += dz;
            if self.chute.2 <= 0 {
                self.chute.2 = 0;
                // A campaign has no rescue (and no capture): the sortie just ends.
                if self.mission_tick & 7 == 0 && (self.campaign.is_some() || !self.start_csar()) {
                    self.finalize(0);
                }
            }
            return;
        }
        let lm = self.opts.landing as i32;
        // Landing corridor (every 8 frames the base is refreshed; corridor test every frame).
        let b = self.obj(self.nearest_base);
        let mut ground_alt = 0;
        if self.nearest_base >= 0 && (self.nearest_base_dist < 0x200 || self.on_ground) {
            let carrier = b.flags & oflags::CARRIER != 0;
            let (mut hw, mut hl) = if carrier {
                (0x100, 0x3C0)
            } else {
                (0xA0, if b.flags & oflags::DISABLED != 0 { 0x400 } else { 0x800 })
            };
            if carrier {
                ground_alt = 0x80;
            }
            if self.opts.landing == LandingMode::Easy {
                hw += 0x80;
                hl += 0x100;
            }
            self.corridor =
                s16(self.p.mx - b.x).abs() <= hw >> 5 && s16(self.p.my - b.y).abs() <= hl >> 5;
            if !self.corridor {
                ground_alt = 0;
            }
        } else {
            self.corridor = false;
        }
        let touchdown = self.on_ground && !self.prev_on_ground;
        if touchdown && self.corridor {
            let lim = if b.flags & oflags::CARRIER != 0 { 0x100 } else { 0x80 };
            let sink = (-(lm * ps.climb_rate)) & 0xFFFF;
            let bad =
                sink > lim || (lm != 0 && (ps.gear_up || abs16(self.p.roll) > ((24 / lm) << 8)));
            if bad {
                self.sound(4, 2);
                self.finalize(5);
                return;
            }
        }
        let _ = ground_alt;
        if self.corridor {
            if self.p.knots <= 1 && self.on_ground && self.frame & 15 == 0 {
                for k in 0..2 {
                    let t = self.targets[k];
                    if t.object == self.nearest_base
                        && t.mission_type == 4
                        && self.mission_tick < self.deadline
                        && self.mark_target_reached(k)
                    {
                        self.msg("Cargo delivered");
                        // The original empties bay k (the generator loads the target's
                        // equipment there).
                        if let Some(b) =
                            self.bays.iter_mut().find(|b| b.0 == SPECIAL_EQUIPMENT && b.1 != 0)
                        {
                            b.1 = 0;
                        }
                    }
                }
                if b.flags & 0x500 != 0
                    && self.armed_landing != 0
                    && b.flags & oflags::DISABLED == 0
                {
                    let old = self.armed_landing;
                    self.armed_landing += 1;
                    if old == 1 {
                        self.msg("Safe Landing");
                        self.sound(0x2B, 2);
                    }
                    if cdiv(16, fs) < self.armed_landing {
                        // Campaign: on a friendly base the sortie goes on (rearm, or end it).
                        if self.campaign.is_some() {
                            self.armed_landing = 0;
                            let object = self.nearest_base.max(0) as usize;
                            self.emit(SimEvent::LandedAtBase { object });
                            return;
                        }
                        self.finalize(0);
                        return;
                    }
                }
            }
        } else if self.on_ground {
            if (lm != 0 || self.hits > 4 || ps.fuel == 0) && self.p.knots > 50 {
                self.sound(4, 2);
                self.finalize(1);
                return;
            }
        } else {
            self.armed_landing = 1;
        }
        // Object collision.
        if ps.collided && !self.external_view {
            if lm != 0 && self.p.alt != 0 {
                self.sound(4, 2);
                self.finalize(2);
            } else {
                self.emit(SimEvent::CollisionBounce);
            }
        }
    }
}
