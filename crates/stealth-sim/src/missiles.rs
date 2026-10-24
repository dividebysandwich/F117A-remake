use crate::ballistics;
use crate::math::{abs16, approx, bearing, cdiv, clamp_q, hi8s, s16, sign, view_z_f};
use crate::types::*;
use crate::weather::optical_seeker;
use crate::{ENEMY_SLOTS, Sim};

impl Sim {
    /// Seeker acquisition of a target at (tx, ty, talt) for class `cls`. Sets `acq_aim` always and `acq_range` on
    /// success. Optical seekers also need a line of sight through the clouds (`talt` viewZ).
    fn acquire(&mut self, s: usize, tx: i32, ty: i32, talt: i32, cls: i32) -> bool {
        let m = self.proj[s];
        let (r, aim, off) = seeker_geometry(&m, tx, ty);
        self.acq_aim = aim;
        let overshot = off > 0x6000 && s < 8 && m.speed < r;
        // Fixed: class 3 (track-via-missile) sees all around, and classic kept it circling a
        // target it had overshot; now it is spent like the others.
        if off > 0x1000 && (cls != 3 || (overshot && self.opts.fix_classic_bugs)) {
            if overshot {
                self.proj[s].ttl = 0;
            }
            return false;
        }
        if cls == 0 && abs16(s16(m.heading - self.pl.p.heading)) > 0x2000 {
            return false;
        }
        if optical_seeker(m.spec, cls) && !self.sees((m.x, m.y, m.alt), (tx, ty, talt)) {
            return false;
        }
        self.acq_range = r;
        true
    }

    /// The Hellfire in slot `s` takes the targeting pod's spot as its target: the object at it
    /// (within `HELLFIRE_SPOT_RADIUS`), else the plain ground there (scratch object 0). Kept
    /// while the pod sees no ground or the pilot has no line of sight to it.
    pub(crate) fn follow_spot(&mut self, s: usize) {
        let Some((x, y)) = self.pl.last_input.pod.and_then(|p| p.ground) else { return };
        if !self.player_sees(x, y, 0) {
            return;
        }
        let m = self.proj[s];
        if (m.tgt_x, m.tgt_y) == (x, y) && m.target > 0 {
            return;
        }
        let near = (1..self.objects.len())
            .filter(|&i| self.objects[i].flags & oflags::DESTROYED == 0)
            .map(|i| (i, approx(self.objects[i].x - x, self.objects[i].y - y)))
            .filter(|&(_, d)| d <= HELLFIRE_SPOT_RADIUS)
            .min_by_key(|&(_, d)| d);
        let t = match near {
            Some((i, _)) => i as i32,
            None => {
                self.ground_point(x, y);
                0
            }
        };
        let m = &mut self.proj[s];
        m.target = t;
        m.tgt_x = x;
        m.tgt_y = y;
    }

    /// Scratch object 0 as the plain ground at (x, y) (map units): no shape, nothing to destroy.
    pub(crate) fn ground_point(&mut self, x: i32, y: i32) {
        if let Some(o) = self.objects.get_mut(0) {
            *o = GroundObject { x, y, ..Default::default() };
        }
    }

    /// The designated ground point stays object 0 while it is the held ground lock (rounds and
    /// bombs falling on tiles reuse object 0); another lock ends it.
    pub(crate) fn hold_spi(&mut self) {
        let Some((x, y)) = self.pl.spi else { return };
        if self.pl.gnd_lock == 0 && self.pl.target_hold {
            self.ground_point(x, y);
        } else {
            self.pl.spi = None;
        }
    }

    pub(crate) fn update_missiles(&mut self, present: &[usize]) {
        let mut saved0 = Vec::new();
        for &k in present {
            self.activate(k);
            saved0.push((k, self.objects.first().copied()));
            self.pl.lights.rad = false;
            self.pl.lights.ir = false;
            self.pl.lights.klaxon = false;
            self.pl.lights.rad_threat = false;
            self.pl.lights.ir_threat = false;
        }
        // Enemy missiles, each against the pilot it was fired at.
        for s in 0..ENEMY_SLOTS {
            if self.proj[s].ttl == 0 {
                continue;
            }
            let k = self.proj_pilot[s] as usize;
            if present.len() > 1 && present.contains(&k) {
                self.activate(k);
            }
            self.missile_frame(s);
        }
        // Each pilot's own weapons: their kills are his.
        for &k in present {
            self.activate(k);
            self.credit = Some(self.pl.id);
            for s in Self::own_slots() {
                if self.proj[s].ttl != 0 {
                    self.missile_frame(s);
                }
            }
            self.credit = None;
        }
        // The other pilots' missiles homing on a pilot light his warnings.
        for (k, close) in std::mem::take(&mut self.pilot_threats) {
            self.activate(k as usize);
            if self.pl.id != k || self.pl.damage & dmg::MW != 0 {
                continue;
            }
            self.pl.lights.rad_threat = true;
            if self.frame & 2 == 0 {
                self.pl.lights.rad = true;
            }
            if close && self.frame & 3 == 0 {
                self.pl.lights.klaxon = true;
                self.emit(SimEvent::Rwr(RwrTone::AirLaunch));
            }
        }
        self.apply_pilot_hits();
        for (k, o) in saved0 {
            self.activate(k);
            if let (Some(first), Some(o)) = (self.objects.first_mut(), o) {
                *first = o;
            }
        }
    }

    /// One frame of the projectile in slot `s`.
    /// The pilots' weapons' hits on other pilots (`pilot_hits`), each on his turn: damage
    /// with friendly fire. A pilot's gunfire damages another at most every quarter second.
    pub(crate) fn apply_pilot_hits(&mut self) {
        let hits = std::mem::take(&mut self.pilot_hits);
        if !self.opts.friendly_fire {
            return;
        }
        let back = self.pl.id as usize;
        for (k, spec) in hits {
            self.activate(k as usize);
            if self.pl.id != k || self.pl.eject != 0 {
                continue;
            }
            match spec {
                Some(spec) => {
                    let name = self.missile(spec).name.clone();
                    self.msg(format!("Hit by {name}"));
                    self.player_hit(true);
                }
                None => self.player_hit(false),
            }
        }
        self.activate(back);
    }

    fn missile_frame(&mut self, s: usize) {
        let (tgx, tgy) = if self.decoy_active() {
            (self.pl.cms[0].x, self.pl.cms[0].y)
        } else {
            (self.pl.p.mx, self.pl.p.my)
        };
        let fs = self.fs;
        if self.proj[s].ttl == 0 {
            return;
        }
        if self.proj[s].fall.is_some() {
            self.fall_bomb(s);
            return;
        }
        let spec = self.proj[s].spec;
        let mt = self.missile(spec).clone();
        let cls = mt.class as i32;
        // Realistic guidance flies this one (`guidance`).
        let guided = self.proj[s].flight.is_some();
        let mut locked = false;
        let mut aim = 0;
        let mut best;
        let mut alt0 = 0;
        let mut hit: i32 = -1;
        // Another pilot this one homes on (and whether on his decoy).
        let mut pilot_hit: Option<(u8, bool)> = None;
        if s < 8 {
            // ---- enemy missile ----
            alt0 = self.pl.p.view_z;
            locked = self.acquire(s, tgx, tgy, self.pl.p.view_z, cls);
            best = self.acq_range;
            aim = self.acq_aim;
            let jam = (self.ecm_on() && cls > 0)
                || (self.irj_on() && cls <= 0)
                || (self.alarm_recent() > self.alarm_timer && spec != 0x21);
            if jam && ((self.diff << 8) + 0x180 < best || cls == 1 || cls == -1) {
                aim = self.proj[s].heading;
                locked = false;
            }
            // A pilot who ejected is no target: the missile flies on past his chute.
            if self.pl.eject != 0 {
                locked = false;
            }
            for k in 1..4 {
                let c = self.pl.cms[k];
                if c.ttl <= 0 {
                    continue;
                }
                let attract = (c.kind == 1 && cls <= 0)
                    || (c.kind == 2
                        && (cls == 1
                            || cls == 2
                            || (cls == 3
                                && (hi8s(s16(aim - self.pl.p.heading)).abs() - 0x40).abs()
                                    < 0x40 - 12 * self.diff)));
                if attract && self.acquire(s, c.x, c.y, self.pl.p.view_z, cls) {
                    aim = 1;
                    locked = false;
                }
            }
            if best > 0x200 {
                let l = self.proj[s].launcher;
                if l > 2 && self.obj(l).flags & oflags::TRACKING == 0 {
                    locked = false;
                }
                if l <= 0
                    && self
                        .flights
                        .get((-l) as usize)
                        .is_none_or(|u| u.flags & fflags::CONTACT == 0)
                {
                    locked = false;
                }
            }
            if !guided && self.proj[s].speed < (mt.max_speed as i32) >> 6 && self.frame & 1 != 0 {
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
                    // Fixed: a falling wreck is no target (classic homed on it until it hit
                    // the ground).
                    if self.opts.fix_classic_bugs && u.flags & fflags::DESTROYED != 0 {
                        continue;
                    }
                    // The seeker leaves the campaign's friendly aircraft alone unless the
                    // pilot locked one up himself.
                    let friend = self.campaign_flight(j).is_some_and(|f| f.blue);
                    if friend && self.proj[s].target != j as i32 {
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
                // Guided: the target locked at launch while the seeker sees it.
                let d = self.proj[s].target;
                if guided && d >= 0 && d != hit && (d as usize) < self.flights.len() {
                    let u = self.flights[d as usize];
                    if u.flags & fflags::AIRBORNE != 0
                        && u.flags & fflags::DESTROYED == 0
                        && u.speed != 0
                        && self.acquire(s, u.x, u.y, u.alt, cls)
                    {
                        aim = self.acq_aim;
                        best = self.acq_range;
                        hit = d;
                        alt0 = u.alt;
                        locked = true;
                    }
                }
                // Fired at another pilot: him while the seeker sees him (or his decoy).
                if let Some(k) = self.proj[s].target_pilot.filter(|&k| self.pilot_airborne(k)) {
                    let p = self.pilot_ref(k as usize);
                    let decoy = p.cms[0].ttl > 0;
                    let (x, y) = if decoy { (p.cms[0].x, p.cms[0].y) } else { (p.p.mx, p.p.my) };
                    let z = p.p.view_z;
                    if self.acquire(s, x, y, z, cls) {
                        aim = self.acq_aim;
                        best = self.acq_range;
                        hit = -1;
                        alt0 = z;
                        locked = true;
                        pilot_hit = Some((k, decoy));
                        let close = (self.proj[s].speed << 4) > best;
                        self.pilot_threats.push((k, close));
                    }
                }
            }
            if !guided && self.proj[s].speed < (mt.max_speed as i32) >> 6 && self.frame & 1 != 0 {
                self.proj[s].speed += 1;
                aim = self.proj[s].heading;
            }
            let t = self.proj[s].target;
            if spec == HELLFIRE_SPEC {
                // The Hellfire rides the laser spot: wherever the targeting pod looks now, while
                // the pilot sees it (else on to where it last was).
                // Its seeker sees the spot wider than the Maverick's sees its target.
                self.follow_spot(s);
                let t = self.proj[s].target;
                if t >= 0 && self.proj[s].tgt_x >= 0 {
                    let o = self.obj(t);
                    let (r, a, off) = seeker_geometry(&self.proj[s], o.x, o.y);
                    self.acq_aim = a;
                    if off <= HELLFIRE_SEEKER {
                        self.acq_range = r;
                        aim = a;
                        best = r;
                        hit = t;
                        alt0 = 0;
                        locked = true;
                    }
                }
            } else if matches!(cls, 4..=6) && t != -1 {
                if t == 0 {
                    let (gx, gy) = (self.proj[s].tgt_x, self.proj[s].tgt_y);
                    if self.objects.first().is_some_and(|o| (o.x, o.y) != (gx, gy)) {
                        if s >= 8 && self.pl.spi == Some((gx, gy)) {
                            self.ground_point(gx, gy);
                        } else {
                            self.find_object_at(gx, gy);
                            self.objects[0].flags = 0;
                        }
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
            let gl = self.pl.gnd_lock;
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
            return;
        }
        // Warning lights / klaxon.
        if locked
            && s < 8
            && abs16(s16(self.acq_aim - self.proj[s].heading)) < 0x1000
            && self.pl.damage & dmg::MW == 0
            && !self.decoy_active()
        {
            self.pl.lights.rad_threat |= cls != 0;
            self.pl.lights.ir_threat |= cls <= 0;
            if cls <= 0 && self.frame & 2 != 0 {
                self.pl.lights.ir = true;
            }
            if cls != 0 && self.frame & 2 == 0 {
                self.pl.lights.rad = true;
            }
            if self.frame & 3 == 0 && (self.proj[s].speed << 4) > best {
                self.pl.lights.klaxon = true;
                // Site launchers are object indices > 2, aircraft are -flight index.
                let tone =
                    if self.proj[s].launcher > 2 { RwrTone::SamLaunch } else { RwrTone::AirLaunch };
                self.emit(SimEvent::Rwr(tone));
            }
        }
        // Steering.
        let fused = if guided {
            let target = if s < 8 {
                Some(if self.decoy_active() {
                    let c = self.pl.cms[0];
                    let p =
                        [c.x as f64 * 32.0 + 16.0, c.y as f64 * 32.0 + 16.0, self.pl.p.alt as f64];
                    crate::guidance::Target { p, v: [0.0; 3] }
                } else {
                    self.player_target()
                })
            } else if let Some((k, decoy)) = pilot_hit {
                Some(if decoy {
                    let p = self.pilot_ref(k as usize);
                    let c = p.cms[0];
                    let p = [c.x as f64 * 32.0 + 16.0, c.y as f64 * 32.0 + 16.0, p.p.alt as f64];
                    crate::guidance::Target { p, v: [0.0; 3] }
                } else {
                    self.pilot_target(k)
                })
            } else if hit < 0 {
                None
            } else if cls == 7 {
                Some(self.unit_target(hit as usize))
            } else {
                Some(self.object_target(hit))
            };
            // The fuse goes off near the player whatever the seeker sees, and at the decoy it
            // steers for (harmlessly; a missile that cannot reach it would circle it).
            let fuse = if s < 8 && self.pl.eject != 0 {
                None
            } else if s < 8 && !self.decoy_active() {
                Some(self.player_target())
            } else {
                target
            };
            let fused = self.fly_guided(s, target.filter(|_| locked && aim != 0), fuse);
            if self.proj[s].ttl == 0 {
                return;
            }
            fused
        } else {
            let mut m = self.proj[s];
            let fused = self.classic_step(&mut m, s < 8, locked, aim, best, alt0);
            self.proj[s] = m;
            // Terrain contact: flat world, objects are not tested.
            if m.alt < 0 {
                self.ground_impact(s);
            }
            fused
        };
        // Proximity fuse.
        let m = self.proj[s];
        if fused {
            if s < 8 {
                if m.ttl > 4 * fs {
                    self.flash = Flash { x: m.x, y: m.y, alt: m.alt, timer: 0x10 };
                    self.emit(SimEvent::Explosion { x: m.x, y: m.y, alt: m.alt, kind: 0x10 });
                    self.proj[s].ttl = if guided { 0 } else { 2 * fs };
                    if !self.decoy_active() {
                        self.msg(format!("Hit by {}", mt.name));
                        self.player_hit(true);
                        if !self.training() {
                            self.log_event(5, spec);
                        }
                    }
                }
            } else if let Some((k, decoy)) = pilot_hit {
                // At another pilot: the burst is everybody's to see, the damage his (with
                // friendly fire), applied when his turn comes.
                let p = self.pilot_ref(k as usize).p;
                let (x, y, alt) = if decoy { (m.x, m.y, m.alt) } else { (p.mx, p.my, p.view_z) };
                self.flash = Flash { x, y, alt, timer: 0x10 };
                self.emit(SimEvent::Explosion { x, y, alt, kind: 0x10 });
                self.proj[s].ttl = 0;
                if !decoy {
                    self.pilot_hits.push((k, Some(spec)));
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
                    if self.opts.fix_classic_bugs {
                        // Classic: the missile sits on the wreck for 2 s, still locked, and
                        // repeats the hit every frame. Fixed: it is spent.
                        self.proj[s].ttl = 0;
                    }
                }
            } else if hit == 0 && self.objects.first().is_some_and(|o| o.shape == 0) {
                // On the plain ground at the spot.
                let o = self.obj(0);
                self.flash = Flash { x: o.x, y: o.y, alt: 0, timer: 0x10 };
                self.emit(SimEvent::Explosion { x: o.x, y: o.y, alt: 0, kind: 0x10 });
                self.proj[s].ttl = 0;
                self.msg(format!("{} impact", mt.name));
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

    /// One frame of the classic flight of missile `m` (an enemy missile when `enemy`): turns
    /// toward `aim` when `locked` (the target `best` map units away at viewZ `alt0`) and moves.
    /// True when the fuse goes off.
    pub(crate) fn classic_step(
        &self,
        m: &mut Projectile,
        enemy: bool,
        locked: bool,
        aim: i32,
        best: i32,
        alt0: i32,
    ) -> bool {
        let (diff, fs) = (self.diff, self.fs);
        let turn = self.missile(m.spec).turn as i32;
        m.locked = locked;
        if aim != 0 && locked {
            let mut d = s16(aim - m.heading);
            if enemy {
                d = clamp_q(d, -(diff + 1) << 8, (diff + 1) << 8);
            }
            d = clamp_q(d, -turn << 7, turn << 7);
            m.heading = s16(m.heading + cdiv(d * 4, fs));
            m.roll = s16(d * 2);
            let ab = best.abs();
            let pa = if enemy && best < 0x400 {
                bearing(s16(alt0 - m.alt) >> 4, ab)
            } else {
                bearing((s16(alt0 - m.alt) >> 5) + if ab > 0x140 { ab >> 3 } else { 0 }, ab)
            };
            let mut dp = s16(pa - m.pitch);
            if enemy {
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
        locked && (s16(alt0 - m.alt).abs() >> 5) + best < cdiv(m.speed << 3, fs)
    }

    /// A ballistic bomb's frame: it moves along its path ([`ballistics`]) and lands where the
    /// path meets the ground.
    fn fall_bomb(&mut self, s: usize) {
        let m = self.proj[s];
        let Some(f) = m.fall else { return };
        let Some(k) = ballistics::drag(m.spec, self.opts.fix_classic_bugs) else { return };
        let fine = ROUND_FINE as f64;
        let before = [f.x as f64 / fine, f.y as f64 / fine, f.alt as f64 / fine];
        let (mut p, mut v) = (before, [f.vx as f64 / fine, f.vy as f64 / fine, f.vz as f64 / fine]);
        ballistics::step(&mut p, &mut v, k, 1.0 / self.fs as f64);
        let landed = p[2] <= 0.0;
        if landed {
            // Where the step crossed the ground.
            let t = if before[2] > 0.0 { before[2] / (before[2] - p[2]) } else { 0.0 };
            p = [before[0] + (p[0] - before[0]) * t, before[1] + (p[1] - before[1]) * t, 0.0];
        }
        let q = |a: f64| (a * fine).round() as i32;
        let nf = BombFall {
            x: q(p[0]),
            y: q(p[1]),
            alt: q(p[2]),
            vx: q(v[0]),
            vy: q(v[1]),
            vz: q(v[2]),
        };
        let bam = 65536.0 / std::f64::consts::TAU;
        let pm = &mut self.proj[s];
        pm.fall = Some(nf);
        pm.x = (nf.x / (32 * ROUND_FINE)) & 0xFFFF;
        pm.y = (nf.y / (32 * ROUND_FINE)) & 0xFFFF;
        pm.alt = if landed { -1 } else { view_z_f(p[2]) as i32 };
        pm.heading = s16((v[0].atan2(-v[1]) * bam) as i32);
        pm.pitch = s16((v[2].atan2(v[0].hypot(v[1])) * bam) as i32);
        pm.ttl -= 1;
        if landed {
            self.ground_impact(s);
        }
    }

    /// Ground contact of a projectile.
    pub(crate) fn ground_impact(&mut self, s: usize) {
        let m = self.proj[s];
        let (ex, ey) = (m.x, m.y);
        let mut ealt = m.alt;
        self.proj[s].ttl = 0;
        self.flash = Flash { x: ex, y: ey, alt: ealt, timer: -6 };
        let long = if s >= 8 {
            self.weapon_types[(m.weapon.max(0) as usize).min(self.weapon_types.len() - 1)]
                .long_name
                .clone()
        } else {
            String::new()
        };
        let mut kind = -6;
        if self.opts.fix_classic_bugs && s >= 8 && m.spec == 38 {
            // Intended airdrop rule, unreachable in classic mode.
            let mut delivered = false;
            for k in 0..2 {
                let t = self.pl.targets[k];
                let o = self.obj(t.object);
                if approx(ex - o.x, ey - o.y) < 0x100
                    && t.mission_type == 3
                    && self.mission_tick < self.pl.deadline
                {
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
                let (tx, ty) =
                    self.pl.last_tile.map_or((self.obj(o).x, self.obj(o).y), |t| t.map_pos());
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
            let d = approx(self.pl.p.mx - ex, self.pl.p.my - ey) + (self.pl.p.view_z >> 5);
            if self.diff * 32 > d {
                // Classic builds a "Blast damage" message after the miss text but never shows it.
                self.player_hit(false);
            }
        } else if s >= 8 {
            self.msg(format!("{long} ground impact"));
        }
        self.emit(SimEvent::Explosion { x: ex, y: ey, alt: ealt, kind });
    }

    /// Countermeasure timers: only the first active slot counts down.
    pub(crate) fn update_cm_timers(&mut self) {
        if let Some(c) = self.pl.cms.iter_mut().find(|c| c.ttl > 0) {
            c.ttl -= 1;
        }
    }
}

/// Range (map units), bearing and angle off the heading of (tx, ty) from missile `m`.
pub(crate) fn seeker_geometry(m: &Projectile, tx: i32, ty: i32) -> (i32, i32, i32) {
    let dx = s16(tx - m.x);
    let dy = s16(ty - m.y);
    let aim = bearing(dx, s16(-dy));
    (approx(dx, dy), aim, abs16(s16(aim - m.heading)))
}
