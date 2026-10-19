use crate::math::{abs16, approx, bearing, cdiv, clamp_q, hi8s, s16, sign};
use crate::types::*;
use crate::weather::optical_seeker;
use crate::{N_PROJ, Sim};

impl Sim {
    /// `acquire(slot, tx, ty, talt, cls)` (V:711F). Sets `acq_aim` always and `acq_range` on
    /// success. Optical seekers also need a line of sight through the clouds (`talt` viewZ).
    fn acquire(&mut self, s: usize, tx: i32, ty: i32, talt: i32, cls: i32) -> bool {
        let m = self.proj[s];
        let dx = s16(tx - m.x);
        let dy = s16(ty - m.y);
        let r = approx(dx, dy);
        let aim = bearing(dx, s16(-dy));
        self.acq_aim = aim;
        let off = abs16(s16(aim - m.heading));
        if off > 0x1000 && cls != 3 {
            if off > 0x6000 && s < 8 && m.speed < r {
                self.proj[s].ttl = 0;
            }
            return false;
        }
        if cls == 0 && abs16(s16(m.heading - self.p.heading)) > 0x2000 {
            return false;
        }
        if optical_seeker(m.spec, cls) && !self.sees((m.x, m.y, m.alt), (tx, ty, talt)) {
            return false;
        }
        self.acq_range = r;
        true
    }

    pub(crate) fn update_missiles(&mut self) {
        let saved0 = self.objects.first().copied();
        self.lights.rad = false;
        self.lights.ir = false;
        self.lights.klaxon = false;
        self.lights.rad_threat = false;
        self.lights.ir_threat = false;
        let (tgx, tgy) = if self.decoy_active() { (self.cms[0].x, self.cms[0].y) } else { (self.p.mx, self.p.my) };
        let fs = self.fs;
        for s in 0..N_PROJ {
            if self.proj[s].ttl == 0 {
                continue;
            }
            let spec = self.proj[s].spec;
            let mt = self.missile(spec).clone();
            let cls = mt.class as i32;
            let mut locked = false;
            let mut aim = 0;
            let mut best;
            let mut alt0 = 0;
            let mut hit: i32 = -1;
            if s < 8 {
                // ---- enemy missile ----
                alt0 = self.p.view_z;
                locked = self.acquire(s, tgx, tgy, self.p.view_z, cls);
                best = self.acq_range;
                aim = self.acq_aim;
                let jam = (self.ecm_on() && cls > 0) || (self.irj_on() && cls <= 0) || (self.alarm_recent() > self.alarm_timer && spec != 0x21);
                if jam && ((self.diff << 8) + 0x180 < best || cls == 1 || cls == -1) {
                    aim = self.proj[s].heading;
                    locked = false;
                }
                for k in 1..4 {
                    let c = self.cms[k];
                    if c.ttl <= 0 {
                        continue;
                    }
                    let attract = (c.kind == 1 && cls <= 0)
                        || (c.kind == 2
                            && (cls == 1
                                || cls == 2
                                || (cls == 3 && (hi8s(s16(aim - self.p.heading)).abs() - 0x40).abs() < 0x40 - 12 * self.diff)));
                    if attract && self.acquire(s, c.x, c.y, self.p.view_z, cls) {
                        aim = 1;
                        locked = false;
                    }
                }
                if best > 0x200 {
                    let l = self.proj[s].launcher;
                    if l > 2 && self.obj(l).flags & oflags::TRACKING == 0 {
                        locked = false;
                    }
                    if l <= 0 && self.flights.get((-l) as usize).is_none_or(|u| u.flags & fflags::CONTACT == 0) {
                        locked = false;
                    }
                }
                if self.proj[s].speed < (mt.max_speed as i32) >> 6 && self.frame & 1 != 0 {
                    self.proj[s].speed += 1;
                }
            } else {
                // ---- player weapon ----
                best = 0x7FFF;
                if cls == 7 {
                    for j in 0..self.flights.len() {
                        let u = self.flights[j];
                        if u.flags & fflags::AIRBORNE == 0 || u.speed == 0 {
                            continue;
                        }
                        if self.acquire(s, u.x, u.y, u.alt, cls) && self.acq_range < best {
                            aim = self.acq_aim;
                            best = self.acq_range;
                            hit = j as i32;
                            alt0 = u.alt;
                            locked = true;
                            if best < 0x180 {
                                self.flights[j].flags |= fflags::HIT;
                            }
                        }
                    }
                }
                if self.proj[s].speed < (mt.max_speed as i32) >> 6 && self.frame & 1 != 0 {
                    self.proj[s].speed += 1;
                    aim = self.proj[s].heading;
                }
                let t = self.proj[s].target;
                if matches!(cls, 4..=6) && t != -1 {
                    if t == 0 {
                        let (gx, gy) = (self.proj[s].tgt_x, self.proj[s].tgt_y);
                        if self.objects.first().is_some_and(|o| (o.x, o.y) != (gx, gy)) {
                            self.find_object_at(gx, gy);
                            self.objects[0].flags = 0;
                        }
                    }
                    let o = self.obj(t);
                    if self.acquire(s, o.x, o.y, 0, cls) {
                        aim = self.acq_aim;
                        best = self.acq_range;
                        hit = t;
                        alt0 = 0;
                        locked = true;
                    }
                }
                let gl = self.gnd_lock;
                if spec == 28 && gl >= 0 && (gl as usize) < self.objects.len() {
                    let o = self.obj(gl);
                    // The laser spot needs the designator's line of sight too.
                    if self.player_sees(o.x, o.y, 0) && self.acquire(s, o.x, o.y, 0, 28) {
                        aim = self.acq_aim;
                        best = self.acq_range;
                        hit = gl;
                        alt0 = 0;
                        locked = true;
                    }
                }
            }
            if self.proj[s].ttl == 0 {
                // Overshot in acquire.
                continue;
            }
            // Warning lights / klaxon.
            if locked && s < 8 && abs16(s16(self.acq_aim - self.proj[s].heading)) < 0x1000 && self.damage & dmg::MW == 0 && !self.decoy_active() {
                self.lights.rad_threat |= cls != 0;
                self.lights.ir_threat |= cls <= 0;
                if cls <= 0 && self.frame & 2 != 0 {
                    self.lights.ir = true;
                }
                if cls != 0 && self.frame & 2 == 0 {
                    self.lights.rad = true;
                }
                if self.frame & 3 == 0 && (self.proj[s].speed << 4) > best {
                    self.lights.klaxon = true;
                    // Site launchers are object indices > 2, aircraft are -flight index.
                    let tone = if self.proj[s].launcher > 2 { RwrTone::SamLaunch } else { RwrTone::AirLaunch };
                    self.emit(SimEvent::Rwr(tone));
                }
            }
            // Steering.
            let diff = self.diff;
            let turn = mt.turn as i32;
            let m = &mut self.proj[s];
            m.locked = locked;
            if aim != 0 && locked {
                let mut d = s16(aim - m.heading);
                if s < 8 {
                    d = clamp_q(d, -(diff + 1) << 8, (diff + 1) << 8);
                }
                d = clamp_q(d, -turn << 7, turn << 7);
                m.heading = s16(m.heading + cdiv(d * 4, fs));
                m.roll = s16(d * 2);
                let ab = best.abs();
                let pa = if s < 8 && best < 0x400 {
                    bearing(s16(alt0 - m.alt) >> 4, ab)
                } else {
                    bearing((s16(alt0 - m.alt) >> 5) + if ab > 0x140 { ab >> 3 } else { 0 }, ab)
                };
                let mut dp = s16(pa - m.pitch);
                if s < 8 {
                    dp = clamp_q(dp, -(diff + 1) << 9, (diff + 1) << 9);
                }
                dp = clamp_q(dp, -turn << 8, turn << 8);
                m.pitch = s16(m.pitch + cdiv(dp * 4, fs));
            } else if m.pitch > 0 && m.spec != 30 {
                m.pitch = s16(m.pitch - cdiv(sign(m.pitch) * 0x800, fs));
            }
            if m.spec == 28 {
                m.pitch = m.pitch.min(-0x800);
            }
            if m.spec == 30 || m.alt == 1 {
                m.pitch = s16(m.pitch - cdiv(0x800, fs)).max(s16(m.launcher));
            }
            let step = cdiv(self.trig.cos_mul(m.pitch, m.speed) * 4, fs);
            m.x = (m.x + self.trig.sin_mul(m.heading, step)) & 0xFFFF;
            m.y = (m.y - self.trig.cos_mul(m.heading, step)) & 0xFFFF;
            m.alt += self.trig.sin_mul(m.pitch, cdiv(m.speed << 7, fs));
            m.ttl -= 1;
            // posHitsTerrain(m) every 4th frame per slot: flat world, objects not tested (README).
            if self.proj[s].alt < 0 {
                self.ground_impact(s);
            }
            // Proximity fuse.
            let m = self.proj[s];
            if locked && (s16(alt0 - m.alt).abs() >> 5) + best < cdiv(m.speed << 3, fs) {
                if s < 8 {
                    if m.ttl > 4 * fs {
                        self.flash = Flash { x: m.x, y: m.y, alt: m.alt, timer: 0x10 };
                        self.emit(SimEvent::Explosion { x: m.x, y: m.y, alt: m.alt, kind: 0x10 });
                        self.proj[s].ttl = 2 * fs;
                        if !self.decoy_active() {
                            self.msg(format!("Hit by {}", mt.name));
                            self.player_hit();
                            if !self.training() {
                                self.log_event(5, spec);
                            }
                        }
                    }
                } else if cls == 7 {
                    if hit >= 0 {
                        let u = self.flights[hit as usize];
                        let pm = &mut self.proj[s];
                        pm.x = u.x;
                        pm.y = u.y;
                        pm.alt = u.alt;
                        pm.ttl = 2 * fs;
                        self.flash = Flash { x: u.x, y: u.y, alt: u.alt, timer: 0x10 };
                        self.emit(SimEvent::Explosion { x: u.x, y: u.y, alt: u.alt, kind: 0x10 });
                        self.destroy_aircraft(hit as usize);
                        let n = self.ac_type(u.spec).name.clone();
                        self.msg(format!("{n} hit by {}", mt.name));
                        if self.opts.fix_original_bugs {
                            // Original: the missile sits on the wreck for 2 s, still locked, and
                            // repeats the hit every frame. Fixed: it is spent.
                            self.proj[s].ttl = 0;
                        }
                    }
                } else if hit >= 0 {
                    let o = self.obj(hit);
                    self.flash = Flash { x: o.x, y: o.y, alt: 0, timer: 0x10 };
                    self.emit(SimEvent::Explosion { x: o.x, y: o.y, alt: 0, kind: 0x10 });
                    self.proj[s].ttl = 0;
                    let r = self.rng.random(4);
                    let text = if self.compat(m.weapon, hit) > r {
                        self.destroy_ground(hit);
                        self.place_name(hit as usize)
                    } else {
                        "Ineffective".to_string()
                    };
                    self.alarm_timer = self.alarm_init();
                    self.ref_x = o.x;
                    self.ref_y = o.y;
                    self.ref_alt = 3000;
                    self.msg(format!("{text} hit by {}", mt.name));
                }
            }
        }
        if let (Some(o), Some(s)) = (self.objects.first_mut(), saved0) {
            *o = s;
        }
    }

    /// Ground contact of a projectile (V:6BBD).
    fn ground_impact(&mut self, s: usize) {
        let m = self.proj[s];
        let (ex, ey) = (m.x, m.y);
        let mut ealt = m.alt;
        self.proj[s].ttl = 0;
        self.flash = Flash { x: ex, y: ey, alt: ealt, timer: -6 };
        let long = if s >= 8 { self.weapon_types[(m.weapon.max(0) as usize).min(19)].long_name.clone() } else { String::new() };
        let mut kind = -6;
        if self.opts.fix_original_bugs && s >= 8 && m.spec == 38 {
            // Intended airdrop rule (weapons.md §5.5.4), unreachable in the original.
            let mut delivered = false;
            for k in 0..2 {
                let t = self.targets[k];
                let o = self.obj(t.object);
                if approx(ex - o.x, ey - o.y) < 0x100 && t.mission_type == 3 && self.mission_tick < self.deadline {
                    self.msg("Equipment Delivered");
                    self.mark_target_reached(k);
                    delivered = true;
                }
            }
            if !delivered {
                self.msg("Bad Drop");
            }
        } else if matches!(m.spec, 28..=30) {
            self.sound(5, 2);
            let mut text = format!("{long} misses ");
            let o = self.find_object_at(ex, ey);
            if o != -1 && self.obj(o).flags & oflags::DESTROYED == 0 {
                let (tx, ty) = self.last_tile.map_or((self.obj(o).x, self.obj(o).y), |t| t.map_pos());
                let dist = approx(tx - ex, ty - ey);
                let lim = (self.compat(m.weapon, o) << 6) / (self.diff + 2);
                if (lim as u16) > (dist as u16) {
                    self.destroy_ground(o);
                    text = format!("{} destroyed by {long}", self.place_name(o as usize));
                    kind = 0x10;
                    ealt = 0;
                }
            }
            self.msg(text);
            let d = approx(self.p.mx - ex, self.p.my - ey) + (self.p.view_z >> 5);
            if self.diff * 32 > d {
                // "Blast damage" is copied into strBuf after the message: never shown.
                self.player_hit();
            }
        } else if s >= 8 {
            self.msg(format!("{long} ground impact"));
        }
        self.emit(SimEvent::Explosion { x: ex, y: ey, alt: ealt, kind });
    }

    /// Countermeasure timers (V:461C): only the first active slot counts down.
    pub(crate) fn update_cm_timers(&mut self) {
        if let Some(c) = self.cms.iter_mut().find(|c| c.ttl > 0) {
            c.ttl -= 1;
        }
    }
}
