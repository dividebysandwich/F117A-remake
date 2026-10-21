use crate::Sim;
use crate::math::{abs16, approx, bearing, cdiv, clamp_q, s16};
use crate::types::*;

impl Sim {
    pub(crate) fn update_aircraft(&mut self, present: &[usize]) {
        for &k in present {
            self.activate(k);
            self.pl.threat_count_last = self.pl.threat_count;
            self.pl.threat_count = 0;
        }
        let eslot = self.n_bullets + ((self.frame & 0xC) >> 2) as usize;
        if let Some(b) = self.bullets.get_mut(eslot) {
            *b = Bullet::default();
        }
        let nf = self.flights.len();
        for i in 0..nf {
            if self.flights[i].flags & fflags::EXISTS == 0 {
                self.flights[i].mode = 0;
                continue;
            }
            let kind = self.ac_type(self.flights[i].spec).kind;
            if self.flights[i].flags & fflags::AIRBORNE == 0 || self.flights[i].speed == 0 {
                self.flights[i].mode = 0;
                self.scramble(i);
                continue;
            }
            if present.len() > 1 {
                let k = self.flight_target(i);
                self.activate(k as usize);
            }
            let alive = self.flights[i].flags & fflags::DESTROYED == 0;
            match kind {
                crate::AircraftKind::Drone if alive => self.fly_drone(i),
                crate::AircraftKind::Helicopter if alive => self.fly_helo(i),
                _ => self.fly(i),
            }
        }
    }

    /// The pilot flight unit `i` goes for: the one its radar holds, else the one the alarm is
    /// about, else the nearest.
    fn flight_target(&mut self, i: usize) -> u8 {
        if self.flight_pilot.len() < self.flights.len() {
            self.flight_pilot.resize(self.flights.len(), 0);
        }
        let u = self.flights[i];
        let cur = self.flight_pilot[i];
        let k = if u.flags & fflags::CONTACT == 0 && self.alarm_timer != 0 {
            self.nearest_pilot(self.alarm_pilot, true, u.x, u.y)
        } else {
            self.nearest_pilot(cur, u.flags & fflags::CONTACT != 0, u.x, u.y)
        };
        self.flight_pilot[i] = k;
        k
    }

    fn fly(&mut self, i: usize) {
        let fs = self.fs;
        let nf = self.flights.len();
        let n = self.objects.len() as i32;
        let t = self.ac_type(self.flights[i].spec).clone();
        let agil = t.agility as i32;
        let (px, py, pz) = (self.pl.p.mx, self.pl.p.my, self.pl.p.view_z);
        let mut u = self.flights[i];
        // --- target selection ---
        let (tx, ty, tz, mode);
        if u.flags & fflags::RTB != 0 {
            let b = self.obj(u.wp);
            if u.flags & fflags::FINAL != 0 {
                tx = s16(b.x - 2 * s16(u.x - b.x));
                ty = b.y;
                tz = if (abs16(u.x - b.x) + b.flags) & 0x200 != 0 { 140 } else { 12 };
            } else {
                tx = b.x;
                ty = s16(b.y + self.ns * 0x280);
                tz = approx(u.x - tx, u.y - ty) + 2000;
            }
            mode = 2;
        } else if self.alarm_timer != 0
            && (u.flags & 0x140 == 0 || self.alarm_timer > self.alarm_recent())
        {
            mode = 1;
            if self.decoy_active() {
                tx = self.pl.cms[0].x;
                ty = self.pl.cms[0].y;
                tz = clamp_q(pz, 1000, 30000);
            } else {
                tx = self.ref_x;
                ty = self.ref_y;
                tz = self.ref_alt;
            }
        } else if let Some((ex, ey, ez)) = self.campaign_pursuit(i) {
            // Campaign: going for an aircraft of the other side.
            mode = 4;
            tx = ex;
            ty = ey;
            tz = ez;
        } else {
            mode = 3;
            let (mut x, mut y, mut z);
            if let Some((gx, gy)) = self.campaign_goal(i, &u) {
                x = gx;
                y = gy;
                z = clamp_q(u.alt, 6000, 16000);
            } else if u.flags & fflags::ESCORT != 0
                && self.escort_leader >= 0
                && (self.escort_leader as usize) < nf
            {
                let l = self.flights[self.escort_leader as usize];
                let a = s16((i as i32 & 7) * 0x800 + l.heading - 0x1800);
                x = s16(l.x + self.trig.sin_mul(a, l.speed));
                y = s16(l.y - self.trig.cos_mul(a, l.speed));
                z = l.alt + (i as i32 & 7) * 0x40;
            } else {
                if ((i as i32 * 8 + self.mission_tick) & 0xBF) == 0
                    && u.flags & fflags::ALERTED_PATROL == 0
                    && n > 1
                {
                    let pb = bearing(px - u.x, u.y - py);
                    let mut best = 0x7FFF;
                    for _ in 0..8 {
                        let tt = (self.rng.random(n) + 1).min(n - 1);
                        let o = self.objects[tt as usize];
                        if o.flags & oflags::FRIENDLY != 0 {
                            continue;
                        }
                        let d = abs16(s16(bearing(o.x - u.x, u.y - o.y) - pb));
                        if d < best {
                            best = d;
                            u.wp = tt;
                            if (4 - self.diff) << 12 > best {
                                break;
                            }
                        }
                    }
                    if (approx(px - u.x, py - u.y) & 0xFFC0) > 0x5780 && i != 0 {
                        u.flags &= 0x1C1;
                        u.timer = 0;
                        u.mode = 0;
                        self.flights[i] = u;
                        return;
                    }
                }
                let o = self.obj(u.wp);
                x = o.x;
                y = o.y;
                z = clamp_q(pz + 1000, 5000, 20000);
            }
            if u.flags & fflags::CONTACT != 0 {
                x = px;
                y = py;
                z = u.alt;
            }
            tx = x;
            ty = y;
            tz = z;
        }
        u.mode = mode;
        let dx = s16(tx - u.x);
        let dy = s16(ty - u.y);
        let brg = bearing(dx, s16(-dy));
        let rng = approx(dx, dy);
        let mut pitch_cmd = clamp_q(bearing(s16(tz - u.alt) >> 5, rng), -0x2000, 0x1000);
        // --- steering ---
        let mut roll_cmd;
        if mode == 1 && rng < 0x400 {
            self.pl.threat_count += 1;
            if rng < 0x200
                && self.frame & 3 == 0
                && self.alarm_timer > self.alarm_recent()
                && abs16(s16(u.heading - brg)) < 0x400
                && abs16(s16(u.pitch - pitch_cmd)) < 0x400
                && self.sees((u.x, u.y, u.alt), (self.pl.p.mx, self.pl.p.my, self.pl.p.view_z))
            {
                self.fire_gun(&u);
            }
            let ag = if i == 0 { 1 } else { clamp_q((i as i32 & 3) + self.diff, 0, 2) };
            let h = s16(u.heading + if abs16(u.bank) < 0x4000 { u.bank >> 2 } else { 0 });
            let rel = (s16(brg - h) >> 13) & 7;
            let ph = s16(self.pl.p.heading
                + if abs16(self.pl.p.roll) < 0x4000 { self.pl.p.roll >> 1 } else { 0 });
            let asp = ((s16(u.heading - ph) >> 13) + 4) & 7;
            let v =
                self.maneuver.get((ag * 64 + asp * 8 + rel) as usize).copied().unwrap_or(0) as i32;
            roll_cmd = v << 12;
            if v == 0x100 {
                pitch_cmd = 0x6000;
                roll_cmd = if self.frame & 0x800 != 0 { 0x4000 } else { -0x4000 };
            }
            if v == 0x200 {
                pitch_cmd = s16(0xA000);
                roll_cmd = if self.frame & 0x800 != 0 { -0x4000 } else { 0x4000 };
            }
            if pitch_cmd == s16(0xA000) && u.alt < 3000 - (u.pitch >> 3) {
                pitch_cmd = s16(u.pitch + 0x1000);
            }
            if abs16(u.bank) > 0x4000 {
                roll_cmd = 0;
                pitch_cmd = 0;
            }
        } else {
            roll_cmd = clamp_q(s16(brg - u.heading), -0x2000, 0x2000) * 2;
            if mode == 1 && 2 * self.diff < self.pl.threat_count_last {
                roll_cmd = 0x3000;
            }
        }
        if mode == 1
            && self.obj(self.pl.nearest_base).flags & oflags::FRIENDLY != 0
            && self.pl.nearest_base_dist < 0x780
        {
            roll_cmd = 0x3000;
        }
        roll_cmd = clamp_q(roll_cmd, -agil << 12, agil << 12);
        roll_cmd = clamp_q(s16(roll_cmd - u.bank), -agil << 8, agil << 8);
        if u.flags & fflags::TAKEOFF != 0 {
            if u.speed < 150 {
                u.pitch = 0;
            } else {
                u.pitch = s16(u.pitch + 0x100);
            }
            roll_cmd = 0;
            if u.speed < t.max_speed as i32 {
                u.speed += cdiv(40, fs);
            } else if u.alt > 300 {
                u.flags &= !fflags::TAKEOFF;
            }
        }
        if u.flags & 0x30 != 0 {
            roll_cmd = 0x400;
        }
        if ((i as i32 ^ self.frame) & 3) == 0 {
            // Terrain contact: the world is a flat plane, objects are not tested.
            u.flags &= !fflags::TERRAIN;
        }
        if u.flags & fflags::TERRAIN != 0 {
            pitch_cmd = 0x3000;
        }
        // --- integration ---
        u.bank = s16(u.bank + cdiv(roll_cmd * 4, fs));
        u.heading = s16(u.heading + cdiv(u.bank >> 3, fs));
        let mut dp = s16(pitch_cmd - u.pitch);
        if u.flags & fflags::DESTROYED != 0 {
            dp = -0x200;
            if self.frame & 1 == 0 {
                let slot = ((self.frame & 0x1E) >> 1) as usize;
                self.add_smoke(slot, u.x, u.y, u.alt);
            }
        }
        if u.pitch < 0 && u.alt < 200 - self.trig.sin_mul(u.pitch, 2000) && u.flags & 0x220 == 0 {
            dp = 0x400;
        }
        u.pitch = s16(u.pitch + cdiv(clamp_q(dp, -0x400, 0x400) * 4, fs));
        if abs16(u.pitch) > 0x4000 {
            u.heading = s16(u.heading + 0x8000);
            u.bank = s16(u.bank + 0x8000);
            u.pitch = s16(0x8000 - u.pitch);
        }
        u.flags &= !fflags::HIT;
        let mut mv = (((0x8000 - u.pitch) & 0xFFFF) * u.speed) >> 15;
        mv -= self.trig.sin_mul(u.bank, mv).abs() >> 1;
        if mode == 1 && self.diff >= self.pl.threat_count_last {
            mv += mv >> 1;
        }
        mv = cdiv(mv * 4, fs) >> 2;
        let h = self.trig.cos_mul(u.pitch, mv);
        u.wx = u.wx.wrapping_add(self.trig.sin_mul(u.heading, h));
        u.wy = u.wy.wrapping_sub(self.trig.cos_mul(u.heading, h));
        u.alt += self.trig.sin_mul(u.pitch, mv);
        u.x = (u.wx >> 5) & 0xFFFF;
        u.y = (u.wy >> 5) & 0xFFFF;
        if u.alt > 30000 {
            u.pitch = 0;
        }
        if u.alt < 0 {
            u.flags &= if i != 0 { 0x1C1 } else { 0 };
            self.flash = Flash { x: u.x, y: u.y, alt: 0, timer: -16 };
            self.emit(SimEvent::Explosion { x: u.x, y: u.y, alt: 0, kind: -16 });
            self.emit(SimEvent::AircraftCrashed { unit: i });
            if i as i32 == self.pl.air_lock {
                self.pl.air_lock = -1;
            }
        }
        if rng < 0x10 && mode == 2 {
            let base = self.place_name(u.wp.max(0) as usize);
            let ty = t.name.clone();
            let msg = if u.flags & fflags::FINAL != 0 {
                let first = u.flags & fflags::ROLLOUT == 0;
                u.flags |= fflags::ROLLOUT;
                first.then(|| format!("{base} - {ty} landing"))
            } else {
                u.flags |= fflags::FINAL;
                Some(format!("{base} - {ty} on final"))
            };
            if let Some(m) = msg
                && !self.is_traffic(i)
            {
                self.msg(m);
            }
        }
        if u.flags & fflags::ROLLOUT != 0 {
            u.bank = 0;
            u.pitch = 0;
            u.heading = if self.ns == 1 { 0 } else { s16(0x8000) };
            u.alt =
                if self.obj(self.pl.nearest_base).flags & oflags::CARRIER != 0 { 140 } else { 12 };
            if u.speed > 0 {
                u.speed -= cdiv(60, fs);
            } else {
                u.flags &= 0x1C1;
                if i == 0 && (5..=8).contains(&self.pl.targets[0].mission_type) {
                    u.flags = 0;
                    self.pl.target_escaped = true;
                }
            }
            if self.is_traffic(i) && u.speed < 100 {
                u.flags = (u.flags & 0x1C1) | 0x406;
            }
        }
        u.timer = s16(u.timer - 1);
        if u.timer == 0 {
            u.flags |= fflags::RTB;
            let mut best = (u.wp, 0x7FFF);
            for (k, o) in self.objects.iter().enumerate().take(self.woc).skip(3) {
                // A campaign may have taken the airfield (friendly now).
                if o.flags & 0x101 == 1 && o.flags & oflags::FRIENDLY == 0 {
                    let d = approx(o.x - u.x, o.y - u.y);
                    if d < best.1 {
                        best = (k as i32, d);
                    }
                }
            }
            u.wp = best.0;
        }
        self.flights[i] = u;
        let per = (fs << 4).max(1);
        let phase = ((((i as i32) & 8) >> 3) + (i as i32 & 7) * 2) * fs;
        if self.frame % per == phase
            && u.flags & fflags::DESTROYED == 0
            && u.flags & fflags::AIRBORNE != 0
        {
            self.radar_sweep(i);
        }
    }

    /// Enemy gun round.
    pub(crate) fn fire_gun(&mut self, u: &FlightUnit) {
        let slot = self.n_bullets + ((self.frame & 0xC) >> 2) as usize;
        let v = cdiv(224, self.fs);
        let vz = self.trig.sin_mul(s16(-u.pitch), v) << 5;
        let h = self.trig.cos_mul(u.pitch, v);
        let vx = self.trig.sin_mul(u.heading, h);
        let vy = -self.trig.cos_mul(u.heading, h);
        // The original's map units, in a round's fine units.
        let (f, m) = (ROUND_FINE, 32 * ROUND_FINE);
        if let Some(b) = self.bullets.get_mut(slot) {
            *b = Bullet {
                x: u.x * m,
                y: u.y * m,
                alt: u.alt * f,
                vx: vx * m,
                vy: vy * m,
                vz: vz * f,
            };
        }
    }

    /// Scramble check for an inactive unit: may launch it from a random alert base.
    fn scramble(&mut self, i: usize) {
        if (((self.mission_tick >> (self.roe + 4)) ^ i as i32) & 7) != 0
            || self.is_traffic(i)
            || i >= self.mission_flights
            || i == 0
        {
            return;
        }
        if self.woc == 0 {
            return;
        }
        let b = self.rng.random(self.woc as i32).clamp(0, self.woc as i32 - 1) as usize;
        let u = self.flights[i];
        let o = self.objects[b];
        let range = self.ac_type(u.spec).range as i32;
        if (self.alarm_timer > self.alarm_recent() || u.flags & fflags::SCRAMBLE != 0)
            && (o.flags & 0x181) == 1
            && o.alert == u.spec
            && range > approx(self.ref_x - o.x, self.ref_y - o.y) >> 6
        {
            self.spawn(i, b);
        }
    }

    /// Put flight unit `i` on the runway (or carrier deck) of base object `b`.
    pub(crate) fn spawn(&mut self, i: usize, b: usize) {
        let o = self.objects[b];
        let t = self.ac_type(self.flights[i].spec).clone();
        let ns = self.ns;
        let fs = self.fs;
        let leader = self.escort_leader;
        let u = &mut self.flights[i];
        u.heading = if ns == 1 { 0 } else { s16(0x8000) };
        if o.flags & oflags::CARRIER != 0 {
            u.x = o.x + 3 * ns;
            u.y = o.y - 12 * ns;
            u.alt = 140;
            u.speed = 100;
            u.heading = s16(u.heading - 0x400);
        } else {
            u.x = o.x;
            u.y = o.y + 30 * ns;
            u.alt = 12;
            u.speed = 10;
        }
        u.wx = u.x << 5;
        u.wy = u.y << 5;
        u.pitch = 0;
        u.bank = 0;
        u.flags |= 0x403;
        u.wp = b as i32;
        u.timer = s16(cdiv((t.range as i32 * fs) << 11, t.max_speed as i32));
        if leader == -1 {
            u.flags &= !fflags::ESCORT;
        }
        if !self.is_traffic(i) {
            let base = self.place_name(b);
            self.msg(format!("{base} - {} taking off", t.name));
        }
        self.emit(SimEvent::AircraftTakeoff { unit: i, base: b });
    }

    /// One of the base traffic flight slots (the mission's last 4 flight units).
    pub(crate) fn is_traffic(&self, i: usize) -> bool {
        i < self.mission_flights && i + 4 >= self.mission_flights
    }

    /// Base traffic slots: two flying, two parked (None with fewer than 8 mission flights).
    fn traffic_slots(&self) -> Option<[usize; 4]> {
        let n = self.mission_flights;
        (n >= 8).then(|| [n - 1, n - 2, n - 3, n - 4])
    }

    /// Base traffic: the last 4 flight units of the mission live at the nearest friendly base.
    pub(crate) fn update_base_traffic(&mut self) {
        let Some([t0, t1, t2, t3]) = self.traffic_slots() else { return };
        let b = self.pl.nearest_base;
        if b == self.traffic_base {
            return;
        }
        self.traffic_base = b;
        if b < 0 {
            return;
        }
        let o = self.obj(b);
        if o.flags & oflags::DISABLED != 0 {
            self.traffic_base = -1;
            return;
        }
        let carrier = o.flags & oflags::CARRIER != 0;
        let friendly = o.flags & oflags::FRIENDLY != 0;
        let neutral = o.flags & oflags::NEUTRAL != 0 && !friendly;
        let (s1, s2) = if neutral {
            if carrier { (24, 24) } else { (27, 27) }
        } else if carrier {
            (8, 8)
        } else if friendly {
            (21, 28)
        } else {
            (0, 0)
        };
        let park = if friendly || neutral { 10 } else { 11 };
        // The two apron spots nearest the base (within 4 km), if any.
        let mut spots: Vec<(i32, i32, i32)> = self
            .parking
            .iter()
            .copied()
            .filter(|&(x, y, _)| ((x >> 5) - o.x).abs() < 0x100 && ((y >> 5) - o.y).abs() < 0x100)
            .collect();
        spots.sort_by_key(|&(x, y, _)| ((x >> 5) - o.x).pow(2) + ((y >> 5) - o.y).pow(2));
        for (k, spec) in [(t0, s1), (t1, s2)] {
            let u = &mut self.flights[k];
            u.flags = 0;
            u.spec = spec;
            u.wp = b;
        }
        for (n, k) in [t2, t3].into_iter().enumerate() {
            let u = &mut self.flights[k];
            u.spec = park;
            u.flags = fflags::EXISTS | fflags::AIRBORNE;
            u.speed = 0;
            u.wp = b;
            u.alt = if carrier { 140 } else { 12 };
            if let Some(&(wx, wy, heading)) = spots.get(n).filter(|_| !carrier) {
                u.x = wx >> 5;
                u.y = wy >> 5;
                u.wx = wx;
                u.wy = wy;
                u.heading = s16(heading);
            } else {
                u.x = o.x + 2 + 2 * n as i32;
                u.y = o.y;
                u.wx = u.x << 5;
                u.wy = u.y << 5;
                u.heading = if self.ns == 1 { 0 } else { s16(0x8000) };
            }
            u.pitch = 0;
            u.bank = 0;
        }
    }

    /// Every 128 frames one of the two traffic aircraft (alternating) appears on approach.
    pub(crate) fn spawn_traffic(&mut self) {
        let Some([t0, t1, ..]) = self.traffic_slots() else { return };
        if self.traffic_base < 0 {
            return;
        }
        self.traffic_toggle = !self.traffic_toggle;
        let k = if self.traffic_toggle { t0 } else { t1 };
        if self.flights[k].flags & fflags::EXISTS != 0 {
            return;
        }
        let o = self.obj(self.traffic_base);
        let ns = self.ns;
        let u = &mut self.flights[k];
        u.flags = 0x207;
        u.alt = 1000;
        u.speed = 250;
        u.wp = self.traffic_base;
        u.wx = o.x << 5;
        u.wy = (o.y << 5) + 0x3000 * ns;
        u.x = (u.wx >> 5) & 0xFFFF;
        u.y = (u.wy >> 5) & 0xFFFF;
        u.heading = if ns == 1 { 0 } else { s16(0x8000) };
        u.pitch = 0;
        u.bank = 0;
        u.timer = 0x7FFF;
    }

    /// New smoke puff; ages all others by 1.
    pub(crate) fn add_smoke(&mut self, slot: usize, x: i32, y: i32, alt: i32) {
        for s in self.smoke.iter_mut() {
            s.age += 1;
        }
        if let Some(s) = self.smoke.get_mut(slot) {
            *s = Smoke { x, y, alt, age: 0 };
        }
    }

    /// Smoke of the last ground kill and the falling wreck.
    pub(crate) fn update_effects(&mut self) {
        if self.smoke_src > 0 {
            for s in self.smoke.iter_mut() {
                s.alt += 4;
                s.y += s.alt >> 9;
            }
            if self.frame & 7 == 0 {
                let o = self.obj(self.smoke_src);
                let slot = ((self.frame >> 3) & 15) as usize;
                self.add_smoke(slot, o.x, o.y, 0x40);
            }
        }
        if self.wreck.alt > 0 {
            if self.wreck.vel > -16 {
                self.wreck.vel -= 12;
            }
            self.wreck.alt += self.wreck.vel;
        }
    }
}
