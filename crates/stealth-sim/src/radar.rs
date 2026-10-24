use crate::math::{abs16, approx, bearing, cdiv, clamp_q, hi8s, s16, u16v};
use crate::types::*;
use crate::{ENEMY_SLOTS, Sim};

/// Pure EMV function.
/// `flags` = player flags, `view_z` = compressed altitude, `velocity` = knots*27.
pub fn emv(view_z: i32, velocity: i32, roll: i32, flags: i32, diff: i32, lockheed: bool) -> i32 {
    let mut r = hi8s(roll).abs();
    if r >= 0x40 {
        r = 0x80 - r;
    }
    let b = if lockheed {
        bearing((diff + 1) << 7, 0x800)
    } else {
        bearing(u16v(((view_z & 0xFFFF) / 3) * (diff + 1)), 0x800)
    };
    let mut e = hi8s(b) + ((velocity & 0xFFFF) >> 10) + (r >> 1);
    if lockheed && flags & 0xFFFF == 0 {
        e += 20;
    }
    if flags & pflags::BAY_OPEN != 0 {
        e += 24;
    }
    if flags & pflags::ECM != 0 {
        e += 16;
    }
    e
}

/// Inputs of [`detect`] that describe the player and the game settings.
#[derive(Debug, Clone, Copy)]
pub struct DetectCtx<'a> {
    /// Player map position.
    pub px: i32,
    pub py: i32,
    pub view_z: i32,
    pub heading: i32,
    pub velocity: i32,
    pub diff: i32,
    pub lockheed: bool,
    /// Theater terrain grid (16 x 16 cells, 256 bytes).
    pub grid: &'a [u8],
}

/// Pure radar signal.
/// `threat` = (lethality, tier, flags) of the radar type. Returns (score, bearing, range).
pub fn detect(
    sx: i32,
    sy: i32,
    salt: i32,
    typ: i32,
    threat: Option<(i32, i32, i32)>,
    c: &DetectCtx,
) -> (i32, i32, i32) {
    if typ == 0 || s16(typ) == -1 {
        return (0, 0, 0);
    }
    let Some((leth, tier, fl)) = threat else { return (0, 0, 0) };
    let dy = s16(c.py - sy);
    let dx = s16(c.px - sx);
    let rng = approx(dx, dy) >> 6;
    let gi = (((c.py & 0xFFFF) >> 11) * 16 + ((c.px & 0xFFFF) >> 11)) as usize;
    let t = c.grid.get(gi).map_or(0, |&g| (g & 0xC) as i32);
    let cx = s16(2 * c.diff + tier + 1);
    let num = (leth - rng) * (t * cx);
    let mut score = s16(cdiv(num, leth));
    let brg = bearing(dx, s16(-dy));
    let mut a = hi8s(abs16(s16(brg - c.heading)));
    if a > 0x40 {
        a = 0x80 - a;
    }
    let mut f = if fl & 1 != 0 {
        ((((0x60 - a) & 0xFFFF) * ((c.velocity & 0xFFFF) >> 5)) & 0xFFFF) >> 9
    } else {
        a
    };
    f = s16(f + 0x20) >> 1;
    score = s16(f * (score >> 1)) >> 4;
    if (s16(salt - c.view_z).abs() >> 10) > rng {
        score = 0;
    }
    if c.lockheed {
        score >>= 1;
    }
    (score, brg, rng)
}

/// RWR tone when a radar detects the player.
fn detected_tone(doppler: bool) -> RwrTone {
    if doppler { RwrTone::Scan } else { RwrTone::SearchWarning }
}

impl Sim {
    pub(crate) fn detect_ctx(&self) -> DetectCtx<'_> {
        DetectCtx {
            px: self.pl.p.mx,
            py: self.pl.p.my,
            view_z: self.pl.p.view_z,
            heading: self.pl.p.heading,
            velocity: self.pl.p.velocity,
            diff: self.diff,
            lockheed: self.pl.lockheed,
            grid: &self.grid,
        }
    }

    pub(crate) fn detect_pure(&self, sx: i32, sy: i32, salt: i32, typ: i32) -> (i32, i32, i32) {
        let th = self.threat_type(typ).map(|t| (t.lethality as i32, t.tier as i32, t.flags as i32));
        detect(sx, sy, salt, typ, th, &self.detect_ctx())
    }

    /// Campaign `detect` of site `i`: recon intelligence shortens its detection range.
    fn detect_site(&mut self, i: usize) -> (i32, i32, i32) {
        let o = self.objects[i];
        let tenths = self.detection_tenths(i);
        let th = self
            .threat_type(o.unit_type)
            .map(|t| ((t.lethality as i32 * tenths / 10).max(1), t.tier as i32, t.flags as i32));
        let r = detect(o.x, o.y, 0, o.unit_type, th, &self.detect_ctx());
        let pre = if self.pl.lockheed { r.0 * 2 } else { r.0 };
        if pre + self.pl.emv > 100 {
            self.emit(SimEvent::CancelTimeAccel);
        }
        r
    }

    /// `detect` with its side effect (time acceleration off when detected).
    pub(crate) fn detect(&mut self, sx: i32, sy: i32, salt: i32, typ: i32) -> (i32, i32, i32) {
        let r = self.detect_pure(sx, sy, salt, typ);
        // The cancel test runs before the Lockheed halving.
        let pre = if self.pl.lockheed { r.0 * 2 } else { r.0 };
        if typ != 0 && typ != -1 && pre + self.pl.emv > 100 {
            self.emit(SimEvent::CancelTimeAccel);
        }
        r
    }

    pub(crate) fn compute_emv(&mut self) {
        self.pl.emv = emv(
            self.pl.p.view_z,
            self.pl.p.velocity,
            self.pl.p.roll,
            self.pl.pflags,
            self.diff,
            self.pl.lockheed,
        );
    }

    /// Raise the theater alarm: note where the player (or decoy) is and put the sites on alert.
    pub(crate) fn alarm(&mut self) {
        self.alarm_timer = self.alarm_init();
        self.alarm_pilot = self.pl.id;
        if self.decoy_active() {
            self.ref_x = self.pl.cms[0].x;
            self.ref_y = self.pl.cms[0].y;
        } else {
            self.ref_x = self.pl.p.mx;
            self.ref_y = self.pl.p.my;
        }
        self.ref_alt = self.pl.p.view_z;
        self.ref_heading = self.pl.p.heading;
        self.alarm_window = 0xFF;
        let lo = (self.roe + self.diff - 1) << 4;
        for o in self.objects.iter_mut().take(self.woc) {
            if o.unit_type != 0 {
                o.alert = clamp_q(o.alert, lo, 255);
            }
        }
    }

    /// TRAK count rebuild every 4*fs frames.
    pub(crate) fn rebuild_tracking(&mut self) {
        let sites = self
            .objects
            .iter()
            .take(self.ground_units)
            .filter(|o| o.unit_type != 0 && o.flags & oflags::DESTROYED == 0 && o.alert > 0xC0)
            .count();
        let air = self
            .flights
            .iter()
            .filter(|u| {
                u.flags & fflags::AIRBORNE != 0 && u.flags & fflags::EXISTS != 0 && u.heat > 0xC0
            })
            .count();
        self.pl.tracking = (sites + air) as i32;
    }

    /// Ground site loop, every frame. A site sweeps for the pilot it is bound to: the one it
    /// tracks while he is in the world, else the one it sees best.
    pub(crate) fn update_sites(&mut self, present: &[usize]) {
        let fs = self.fs;
        if self.site_pilot.len() < self.ground_units {
            self.site_pilot.resize(self.ground_units, present.first().map_or(0, |&k| k as u8));
        }
        for i in 0..self.ground_units {
            let o = self.objects[i];
            if o.unit_type == 0 || o.flags & oflags::DESTROYED != 0 {
                self.objects[i].flags &= !oflags::SWEPT;
                continue;
            }
            if self.site_silent(i) {
                self.objects[i].flags &= !oflags::SWEPT;
                continue;
            }
            let s = &mut self.objects[i];
            s.timer = s16(s.timer - 1);
            if s.timer <= 0 {
                s.timer = s16((fs << 8) / ((s.alert >> 2) + 16) + i as i32);
            }
            if s.timer == 4 {
                if present.len() > 1 {
                    let k = self.site_target(i, present);
                    self.site_pilot[i] = k as u8;
                    self.activate(k);
                }
                if self.pl.sweep_anim < 0 {
                    self.site_sweep(i);
                    self.objects[i].flags |= oflags::SWEPT;
                }
            }
        }
        for &k in present {
            self.activate(k);
            self.pl.sweep_anim = s16(self.pl.sweep_anim - 1);
            if self.pl.sweep_anim < 0 {
                self.pl.egr = None;
                self.pl.ear = None;
            }
            if let Some(a) = &mut self.pl.arc {
                a.frames_left = self.pl.sweep_anim.max(0);
                if self.pl.sweep_anim < 0 {
                    self.pl.arc = None;
                }
            }
        }
    }

    /// Site `i`'s radar is off the air for now: calm sites take turns, fewer the hotter the
    /// war and after an alarm.
    fn site_silent(&self, i: usize) -> bool {
        let o = self.objects[i];
        let k = if self.alarm_last < 0x80 { 2 * self.roe + 1 } else { 2 * self.roe + 3 };
        let ph = (-((s16(self.frame) >> 10) * i as i32)) & 7;
        k < ph && o.alert == 0 && o.flags & oflags::NEUTRAL == 0
    }

    /// The pilot site `i` sweeps for: the one it tracks while he is in the world, else the
    /// one it would see best (the nearest of equals).
    fn site_target(&mut self, i: usize, present: &[usize]) -> usize {
        let cur = self.site_pilot.get(i).copied().unwrap_or(0) as usize;
        let o = self.objects[i];
        if o.flags & oflags::TRACKING != 0 && present.contains(&cur) {
            return cur;
        }
        let mut best = (i32::MIN, i32::MAX, cur);
        for &k in present {
            self.activate(k);
            let (score, _, range) = self.detect_pure(o.x, o.y, 0, o.unit_type);
            let score = score + self.pl.emv;
            if score > best.0 || (score == best.0 && range < best.1) {
                best = (score, range, k);
            }
        }
        if best.2 != cur {
            // Detected someone else so far: news for the new one.
            self.objects[i].flags &= !oflags::PAINTED;
        }
        best.2
    }

    /// Radar sweep of ground site `i`.
    fn site_sweep(&mut self, i: usize) {
        let o = self.objects[i];
        let (score, brg, range) = if self.campaign.is_some() {
            self.detect_site(i)
        } else {
            self.detect(o.x, o.y, 0, o.unit_type)
        };
        let emv = self.pl.emv;
        let doppler = self.threat_type(o.unit_type).is_some_and(|t| t.flags & 1 != 0);
        if score > 0 {
            let colour = if emv + score > 100 {
                17
            } else if emv + score > 50 {
                41
            } else {
                42
            };
            self.pl.egr = Some(SignalBar { length: score.min(99), colour });
            self.pl.sweep_anim = self.fs;
            let h = hi8s(brg);
            self.pl.arc = Some(RadarArc {
                source: i as i32,
                x: o.x,
                y: o.y,
                radius_km: (emv + score) * range / 100,
                doppler,
                arc: (o.alert != 0).then_some((h - 0x20, h + 0x20)),
                colour,
                frames_left: self.fs,
            });
            if o.flags & oflags::PAINTED == 0 {
                self.objects[i].flags |= oflags::PAINTED;
                let n = self.place_name(i);
                self.msg(format!("{n} detected"));
            }
        }
        if score + emv > 100 {
            self.pl.emv_lamp = 2 * self.fs;
            self.emit(SimEvent::Rwr(detected_tone(doppler)));
            let gain = ((self.roe + self.diff + 1) << 4) >> (self.ecm_on() as i32);
            let alert = (self.objects[i].alert + gain).min(255);
            self.objects[i].alert = alert;
            if o.flags & oflags::NEUTRAL == 0 && !self.decoy_active() && alert > 0x7F {
                self.alarm();
            }
            let alert = self.objects[i].alert;
            if self.pl.threat_count_last <= self.diff
                && alert > 0xC0
                && o.unit_type != 21
                && self.pl.nearest_base_dist > 0x500
            {
                self.pl.tracking += 1;
                if self.objects[i].flags & oflags::RADAR_ID == 0 && alert >= 0xFA {
                    self.objects[i].flags |= oflags::RADAR_ID;
                    self.pl.radar_ids += 1;
                    let n = self.place_name(i);
                    self.msg(format!("{n} - Radar ID"));
                    self.emit(SimEvent::Rwr(detected_tone(doppler)));
                    self.log_event(7, i as i32);
                } else {
                    let k = i & 7;
                    if self.proj[k].ttl == 0
                        && self.missile(self.site_missile(o.unit_type)).lock_range as i32 > range
                        && self.may_launch(self.site_missile(o.unit_type), (o.x, o.y, 0))
                    {
                        self.launch_sam(k, i, brg);
                    }
                }
            }
            self.objects[i].flags |= oflags::TRACKING;
        } else {
            if score > 0 {
                self.emit(SimEvent::Rwr(RwrTone::Track));
            }
            let s = &mut self.objects[i];
            s.flags &= !oflags::TRACKING;
            s.alert = (s.alert - 16).max(0);
        }
    }

    /// Site launch.
    fn launch_sam(&mut self, k: usize, i: usize, brg: i32) {
        let o = self.objects[i];
        let spec = self.site_missile(o.unit_type);
        let lock = self.missile(spec).lock_range as i32;
        self.proj[k] = Projectile {
            x: o.x + 8,
            y: o.y,
            alt: 0,
            speed: 1,
            heading: s16(brg),
            pitch: 0x4000,
            roll: 0,
            ttl: s16(lock * self.fs * 16),
            spec,
            weapon: 0,
            target: -1,
            launcher: i as i32,
            tgt_x: 0,
            tgt_y: 0,
            locked: false,
            fall: None,
            flight: None,
            target_pilot: None,
        };
        if self.guided(spec) {
            let tgt = self.player_target().p;
            self.launch_sam_flight(k, i, tgt);
        }
        let n = self.place_name(i);
        let sam = self.missile(spec).name.clone();
        self.msg(format!("{n} firing {sam}"));
        self.proj_pilot[k] = self.pl.id;
        self.pl.launches += 1;
        self.emit(SimEvent::MissileLaunched { slot: k, spec, launcher: Launcher::Site(i) });
    }

    /// Aircraft radar.
    pub(crate) fn radar_sweep(&mut self, i: usize) {
        if self.player_absent() {
            return;
        }
        let u = self.flights[i];
        let rt = self.ac_type(u.spec).radar as i32;
        let (mut score, brg, range) = self.detect(u.x, u.y, u.alt, rt);
        let emv = self.pl.emv;
        let doppler = self.threat_type(rt).is_some_and(|t| t.flags & 1 != 0);
        if score > 0 {
            let omni = self.threat_type(rt).is_some_and(|t| t.flags & 2 != 0);
            let mut arc = None;
            if !omni {
                let h = hi8s(u.heading);
                arc = Some((h - 0x30, h + 0x30));
                if hi8s(abs16(s16(u.heading - brg))) > 0x30 {
                    score = 0;
                }
            }
            if u.flags & fflags::CONTACT != 0 {
                let h = hi8s(brg);
                arc = Some((h - 0x20, h + 0x20));
            }
            let colour = if emv + score > 100 { 15 } else { 11 };
            self.pl.ear = Some(SignalBar { length: score.min(99), colour });
            self.pl.sweep_anim = self.fs;
            self.pl.arc = Some(RadarArc {
                source: -1 - i as i32,
                x: u.x,
                y: u.y,
                radius_km: (emv + score) * range / 100,
                doppler,
                arc,
                colour,
                frames_left: self.fs,
            });
        }
        if score + emv > 100 {
            self.pl.emv_lamp = 2 * self.fs;
            self.emit(SimEvent::Rwr(detected_tone(doppler)));
            let gain = ((self.roe + self.diff + 2) << 4) >> (self.ecm_on() as i32);
            self.flights[i].heat += gain;
            if self.flights[i].heat > 0xC0 {
                self.pl.tracking += 1;
                self.flights[i].flags |= fflags::SAW_PLAYER;
                self.alarm();
                if self.flights[i].flags & fflags::VISUAL_ID == 0
                    && self.visual_range() > range
                    && self.sees((u.x, u.y, u.alt), (self.pl.p.mx, self.pl.p.my, self.pl.p.view_z))
                {
                    self.flights[i].flags |= fflags::VISUAL_ID;
                    self.pl.visual_ids += 1;
                    let n = self.ac_type(u.spec).name.clone();
                    self.msg(format!("Visual ID - {n}"));
                    self.sound(7, 1);
                    self.log_event(6, u.spec);
                }
                if 2 * self.diff >= self.pl.threat_count_last {
                    let k = i & 7;
                    let w = u.missile;
                    if w > 0
                        && (w as usize) < self.missile_types.len()
                        && self.proj[k].ttl == 0
                        && abs16(s16(brg - u.heading)) < 0x1800
                        && self.missile(w).lock_range as i32 > range
                        && 2 * (4 - self.diff) < range
                        && self.may_launch(w, (u.x, u.y, u.alt))
                    {
                        self.launch_aam(k, i);
                    }
                }
            }
            if self.flights[i].flags & fflags::CONTACT == 0 {
                self.sound(7, 1);
            }
            self.flights[i].flags |= fflags::CONTACT;
        } else {
            if score > 0 {
                self.emit(SimEvent::Rwr(RwrTone::Track));
            }
            self.flights[i].flags &= !fflags::CONTACT;
            self.flights[i].heat -= 32;
        }
        self.flights[i].heat = clamp_q(self.flights[i].heat, 0, 255);
    }

    /// Aircraft launch.
    fn launch_aam(&mut self, k: usize, i: usize) {
        let u = self.flights[i];
        let t = self.missile(u.missile).clone();
        let speed = (t.max_speed as i32) >> 6;
        self.proj[k] = Projectile {
            x: u.x,
            y: u.y,
            alt: u.alt - 25,
            speed,
            heading: u.heading,
            pitch: u.pitch,
            roll: u.bank,
            ttl: s16(cdiv((t.lock_range as i32 * self.fs) << 5, speed)),
            spec: u.missile,
            weapon: 0,
            target: -1,
            launcher: -(i as i32),
            tgt_x: 0,
            tgt_y: 0,
            locked: false,
            fall: None,
            flight: None,
            target_pilot: None,
        };
        if self.guided(u.missile) {
            let pos = [u.wx as f64, u.wy as f64, (u.alt - 25) as f64];
            let vel = crate::guidance::direction(u.heading, u.pitch).map(|d| d * u.speed as f64);
            self.start_flight(k, pos, vel);
        }
        let ty = self.ac_type(u.spec).name.clone();
        self.msg(format!("{} fired by {ty}", t.name));
        self.proj_pilot[k] = self.pl.id;
        self.pl.launches += 1;
        self.emit(SimEvent::MissileLaunched {
            slot: k,
            spec: u.missile,
            launcher: Launcher::Aircraft(i),
        });
    }

    /// Nearest friendly / neutral base (every 8 frames).
    pub(crate) fn find_nearest_base(&mut self) {
        let (mut best, mut bd) = (-1, 0x7FFF);
        for (i, o) in self.objects.iter().enumerate().skip(3) {
            if o.flags & 0x201 != 0 && (o.flags >> 8) & 5 != 0 {
                let d = approx(self.pl.p.mx - o.x, self.pl.p.my - o.y);
                if d < bd {
                    bd = d;
                    best = i as i32;
                }
            }
        }
        self.pl.nearest_base = best;
        self.pl.nearest_base_dist = bd;
    }

    /// Lamp timers and the light states.
    pub(crate) fn update_light_timers(&mut self) {
        for t in [&mut self.pl.chaff_lamp, &mut self.pl.flare_lamp, &mut self.pl.emv_lamp] {
            if *t > 0 {
                *t -= 1;
            }
        }
        let l = &mut self.pl.lights;
        l.emv_warning = self.pl.emv_lamp > 0;
        l.chaff = self.pl.chaff_lamp > 0;
        l.flare = self.pl.flare_lamp > 0;
        l.trak = self.pl.tracking != 0 && self.frame & 1 != 0;
        l.ecm = self.pl.pflags & pflags::ECM != 0;
        l.irj = self.pl.pflags & pflags::IRJ != 0;
        l.irj_hot = self.pl.irj_heat > 0;
        l.decoy = self.pl.cms[0].ttl >= 1;
        l.bay = self.pl.pflags & pflags::BAY_OPEN != 0;
    }
}

impl Sim {
    /// The radars that can see the player (hostile sites and aircraft within their detection
    /// range), most dangerous first: launching, then tracking, then the ones whose missiles
    /// reach the player, the higher threat tiers and the closer ones.
    pub fn emitters(&self) -> Vec<Emitter> {
        let (px, py) = (self.pl.p.mx, self.pl.p.my);
        let me = self.pl.id;
        let launched = |source: EmitterSource| {
            self.proj[..ENEMY_SLOTS].iter().zip(&self.proj_pilot).any(|(m, &k)| {
                m.ttl != 0
                    && k == me
                    && match source {
                        EmitterSource::Site(i) => m.launcher == i as i32,
                        EmitterSource::Aircraft(i) => m.launcher == -(i as i32) && i > 0,
                        EmitterSource::Gun(_) => false,
                    }
            })
        };
        let mut out = Vec::new();
        let mut push = |source: EmitterSource,
                        x: i32,
                        y: i32,
                        alt: i32,
                        radar_type: i32,
                        missile: i32,
                        painted: bool,
                        tracking: bool,
                        on_air: bool,
                        facing: Option<i32>| {
            let Some(t) = self.threat_type(radar_type) else { return };
            let (dx, dy) = (s16(px - x), s16(py - y));
            let range = approx(dx, dy) >> 6;
            let tenths = match source {
                EmitterSource::Site(i) => self.detection_tenths(i),
                _ => 10,
            };
            if range >= t.lethality as i32 * tenths / 10 {
                return;
            }
            // Bearing from the player to the emitter.
            let bearing = bearing(s16(x - px), s16(py - y));
            let lock_range = self.missile(missile).lock_range as i32;
            let launch = launched(source);
            // Its signal at the player: the sweep's score (none off the air or outside an
            // airborne radar's cone).
            let th =
                Some(((t.lethality as i32 * tenths / 10).max(1), t.tier as i32, t.flags as i32));
            let (score, brg, _) = detect(x, y, alt, radar_type, th, &self.detect_ctx());
            let in_cone =
                facing.is_none_or(|h| hi8s(abs16(s16(h - brg))) <= 0x30 || t.flags & 2 != 0);
            let signal = if on_air && in_cone { score.max(0) } else { 0 };
            let priority = t.tier as i32 * 8
                + if launch { 300 } else { 0 }
                + if tracking { 120 } else { 0 }
                + if painted { 30 } else { 0 }
                + if range < lock_range { 60 } else { 0 }
                + (t.lethality as i32 - range) / 4;
            out.push(Emitter {
                source,
                x,
                y,
                alt,
                radar_type,
                range,
                bearing,
                lock_range,
                painted,
                tracking,
                launch,
                priority,
                signal,
            });
        };
        for (i, o) in self.objects.iter().enumerate().take(self.ground_units) {
            if o.unit_type == 0
                || o.flags & (oflags::DESTROYED | oflags::FRIENDLY | oflags::NEUTRAL) != 0
            {
                continue;
            }
            // Painting and tracking someone else is no signal for this pilot.
            let mine = self.site_pilot.get(i).is_none_or(|&k| k == me);
            let tracking = mine && (o.flags & oflags::TRACKING != 0 || o.alert > 0xC0);
            let missile = self.site_missile(o.unit_type);
            push(
                EmitterSource::Site(i),
                o.x,
                o.y,
                0,
                o.unit_type,
                missile,
                mine && o.flags & oflags::PAINTED != 0,
                tracking,
                !self.site_silent(i),
                None,
            );
        }
        for (i, u) in self.flights.iter().enumerate() {
            let mine = self.flight_pilot.get(i).is_none_or(|&k| k == me);
            let radar = self.ac_type(u.spec).radar as i32;
            if u.flags & (fflags::EXISTS | fflags::AIRBORNE) != fflags::EXISTS | fflags::AIRBORNE
                || u.flags & fflags::DESTROYED != 0
                || radar <= 0
            {
                continue;
            }
            push(
                EmitterSource::Aircraft(i),
                u.x,
                u.y,
                u.alt,
                radar,
                u.missile,
                mine && u.flags & fflags::CONTACT != 0,
                mine && u.heat > 0xC0,
                true,
                Some(u.heading),
            );
        }
        self.aaa_emitters(&mut out);
        out.sort_by_key(|e| std::cmp::Reverse(e.priority));
        out
    }

    /// Detection range (km) of radar type `t` (its lethality).
    pub fn radar_range(&self, t: i32) -> i32 {
        self.threat_type(t).map_or(0, |t| t.lethality as i32)
    }

    /// Radar type `t` is a Doppler radar (else a pulse radar).
    pub fn radar_doppler(&self, t: i32) -> bool {
        self.threat_type(t).is_some_and(|t| t.flags & 1 != 0)
    }

    /// Lock range (km) of the missile a site of radar type `t` fires.
    pub fn site_lock_range(&self, t: i32) -> i32 {
        self.missile(self.site_missile(t)).lock_range as i32
    }

    /// Name of a radar type ("SA-10", "Hawk").
    pub fn threat_name(&self, t: i32) -> &str {
        self.threat_type(t).map_or("", |t| t.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured reference case (alt 2842, velocity 14935, roll 489, diff 1 -> 44).
    #[test]
    fn emv_confirmed_case() {
        assert_eq!(emv(2842, 14935, 489, 0x01, 1, false), 44);
    }

    #[test]
    fn emv_terms() {
        // Speed term: 500 kt -> 13.
        assert_eq!(emv(0, 500 * 27, 0, 1, 1, false), 13);
        // Bank 90 deg folds to +32, inverted counts as level.
        assert_eq!(emv(0, 0, 0x4000, 1, 0, false), 32);
        assert_eq!(emv(0, 0, -0x8000, 1, 0, false), 0);
        // Bays +24, ECM +16, Lockheed gear-down (+20) only when the whole flag word is 0.
        assert_eq!(emv(0, 0, 0, 1 | 4 | 0x10, 0, false), 40);
        assert_eq!(emv(0, 0, 0, 0, 3, true), 9 + 20);
        assert_eq!(emv(0, 0, 0, 1, 3, true), 9);
        // Altitude term: 1000 ft Regular -> 12.
        assert_eq!(emv(1000, 0, 0, 1, 1, false), 12);
    }

    fn ctx(grid: &[u8], heading: i32, vel: i32) -> DetectCtx<'_> {
        DetectCtx {
            px: 0x4000,
            py: 0x4000,
            view_z: 0,
            heading,
            velocity: vel,
            diff: 1,
            lockheed: false,
            grid,
        }
    }

    /// Regular, flat (t=8), SA-2 at 0 km beam: score ~72.
    #[test]
    fn detect_sa2_beam() {
        let grid = vec![8u8; 256];
        let c = ctx(&grid, 0x4000, 400 * 27);
        // Site straight north of the player, player heading east -> beam aspect.
        let (s, brg, r) = detect(0x4000, 0x4000 - 1, 0, 1, Some((200, 3, 0)), &c);
        assert_eq!(r, 0);
        assert_eq!(brg, 0x8000);
        assert_eq!(s, 72);
        // Nose-on at 100 km: 12 at 400 kt.
        let c = ctx(&grid, 0, 400 * 27);
        let (s, _, r) = detect(0x4000, 0x4000 + 100 * 64, 0, 1, Some((200, 3, 0)), &c);
        assert_eq!(r, 100);
        assert_eq!(s, 12);
    }

    #[test]
    fn detect_terrain_and_elevation() {
        let mut grid = vec![8u8; 256];
        // Mountains under the player: no detection.
        grid[(0x4000 >> 11) * 16 + (0x4000 >> 11)] = 0;
        let c = ctx(&grid, 0x4000, 400 * 27);
        assert_eq!(detect(0x4000, 0x3000, 0, 1, Some((200, 3, 0)), &c).0, 0);
        // Elevation limit: 20 km away, 30000 viewZ above -> (30000 >> 10) = 29 > 20.
        let grid = vec![8u8; 256];
        let mut c = ctx(&grid, 0x4000, 400 * 27);
        c.view_z = 30000;
        assert_eq!(detect(0x4000, 0x4000 - 20 * 64, 0, 1, Some((200, 3, 0)), &c).0, 0);
        c.view_z = 15000;
        assert!(detect(0x4000, 0x4000 - 20 * 64, 0, 1, Some((200, 3, 0)), &c).0 > 0);
        // No radar.
        assert_eq!(detect(0x4000, 0x3000, 0, 0, Some((200, 3, 0)), &c).0, 0);
        assert_eq!(detect(0x4000, 0x3000, 0, -1, Some((200, 3, 0)), &c).0, 0);
    }

    /// Doppler vs pulse aspect (reference values).
    #[test]
    fn detect_doppler_aspect() {
        let grid = vec![12u8; 256];
        let d = |h, t, th| detect(0x4000, 0x3FC0, 0, t, Some(th), &ctx(&grid, h, 500 * 27));
        assert_eq!(d(0, 4, (320, 7, 1)), (162, 0x8000, 1));
        assert_eq!(d(0x4000, 4, (320, 7, 1)), (106, 0x8000, 1));
        assert_eq!(d(0x4000, 1, (320, 7, 0)), (177, 0x8000, 1));
        assert_eq!(d(0x100, 1, (320, 7, 0)), (59, 0x8000, 1));
        // Exact tail-on keeps a = -128 (negative score, a classic quirk).
        assert_eq!(d(0, 1, (320, 7, 0)).0, -177);
    }

    /// Random reference vectors (grid value at the player's cell).
    #[test]
    #[allow(clippy::type_complexity)]
    fn detect_reference_vectors() {
        #[rustfmt::skip]
        let cases: [((i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, bool, u8, (i32, i32, i32)), (i32, i32, i32)); 8] = [
            ((24507, 12466, 3709, 19, 24444, 15122, 5139, 3801, 15257, 1, false, 4, (80, 7, 1)), (30, 32992, 41)),
            ((12323, 17710, 0, 6, 9484, 17324, 3993, 64455, 9384, 1, false, 12, (250, 5, 1)), (66, 50481, 47)),
            ((5665, 12465, 0, 6, 6003, 13117, 16666, 6729, 1232, 1, true, 0, (250, 5, 1)), (0, 27779, 12)),
            ((4147, 10437, 18840, 1, 6654, 8720, 3437, 12167, 11001, 1, false, 0, (200, 3, 0)), (0, 10113, 52)),
            ((18953, 13881, 8629, 6, 18402, 10893, 7091, 65278, 15254, 3, true, 0, (250, 5, 1)), (0, 63708, 50)),
            ((17730, 25714, 0, 19, 20122, 27307, 8203, 58118, 6250, 1, false, 12, (80, 7, 1)), (44, 22517, 49)),
            ((11569, 4277, 7134, 1, 13190, 3751, 19147, 29939, 7764, 1, false, 8, (200, 3, 0)), (58, 13084, 29)),
            ((20986, 13257, 0, 6, 23384, 15628, 12173, 40196, 17009, 0, false, 12, (250, 5, 1)), (59, 24591, 55)),
        ];
        for ((sx, sy, salt, typ, px, py, vz, h, v, d, l, t, th), want) in cases {
            let grid = vec![t; 256];
            let c = DetectCtx {
                px,
                py,
                view_z: vz,
                heading: h,
                velocity: v,
                diff: d,
                lockheed: l,
                grid: &grid,
            };
            assert_eq!(detect(sx, sy, salt, typ, Some(th), &c), want);
        }
    }
}
