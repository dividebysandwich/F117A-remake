use crate::Sim;
use crate::math::{abs16, approx, cdiv, clamp_v, s16};
use crate::types::*;

/// Frames a nearest-target request waits for the HUD to switch to the weapon's mode.
const NEAREST_WAIT: u8 = 8;

impl Sim {
    /// Key handling for this frame's commands.
    pub(crate) fn handle_keys(&mut self, ps: &PlayerState) {
        let c = &ps.cmd;
        // HUD mode changes (F2): entering AIR clears both locks, entering GND the ground lock.
        if self.hud_mode != self.prev_hud_mode {
            match self.hud_mode {
                HudMode::Air => {
                    self.air_lock = -1;
                    self.gnd_lock = -1;
                }
                HudMode::Gnd => self.gnd_lock = -1,
                HudMode::Nav => {}
            }
        }
        // After ejecting, the flare key fires the pilot's signal flares (csar.rs).
        if c.flare && self.eject == 0 {
            self.release_cm(1);
        }
        if c.chaff && self.eject == 0 {
            self.release_cm(2);
        }
        if c.decoy && self.eject == 0 {
            self.release_cm(3);
        }
        if c.toggle_irj {
            self.pflags ^= pflags::IRJ;
        }
        if c.toggle_ecm {
            self.pflags ^= pflags::ECM;
        }
        if c.toggle_bay && self.damage & dmg::BD == 0 && !self.opts.external_stores {
            self.bay_req = !self.bay_req;
            self.sound(0x1F, 1);
        }
        if c.next_bay {
            self.next_weapon();
        }
        if c.designate {
            self.target_hold = false;
            self.pflags ^= pflags::DESIGNATE;
            if self.camera_mfd {
                self.camera_key(0);
            }
        }
        if c.next_target {
            self.target_hold = false;
            self.targeting.target_killed = false;
            self.gnd_lock |= 0x80;
            self.air_lock |= 0x80;
        }
        if let Some(d) = c.camera_dir {
            self.camera_key(d as i32);
        }
        if c.nearest_target {
            self.target_hold = false;
            self.camera_key(0);
            self.nearest_req = NEAREST_WAIT;
        }
        if c.clear_target {
            self.air_lock = -1;
            self.gnd_lock = -1;
            self.nearest_req = 0;
            self.pflags &= !pflags::DESIGNATE;
            self.target_hold = true;
        }
        if c.toggle_training {
            self.pflags ^= pflags::TRAINING;
        }
        if c.resupply && self.training() {
            self.resupply();
        }
        if c.eject {
            self.do_eject();
        }
        // Display locks, after the HUD mode change above has cleared the old ones.
        if let Some(o) = c.lock_object.map(usize::from) {
            if self.objects.get(o).is_some_and(|ob| ob.flags & oflags::DESTROYED == 0) {
                self.gnd_lock = o as i32;
                self.target_hold = true;
                self.nearest_req = 0;
                self.targeting.target_killed = false;
                self.pflags &= !pflags::DESIGNATE;
            } else {
                self.msg("No target");
            }
        }
        if let Some((x, y)) = c.designate_at {
            let o = self.find_object_at(x, y);
            if o < 0 {
                self.msg("No target");
            } else if !self.player_sees(self.obj(o).x, self.obj(o).y, 0) {
                self.msg("Target obscured");
            } else {
                self.gnd_lock = o;
                self.target_hold = true;
                self.nearest_req = 0;
                self.targeting.target_killed = false;
                self.pflags &= !pflags::DESIGNATE;
                if self.smoke_src == 0 {
                    self.smoke_src = -1;
                }
            }
        }
        if let Some(u) = c.lock_unit.map(usize::from) {
            if self.flights.get(u).is_some_and(|f| f.flags & fflags::AIRBORNE != 0 && f.flags & fflags::DESTROYED == 0) {
                self.air_lock = u as i32;
                self.target_hold = true;
                self.nearest_req = 0;
            } else {
                self.msg("No target");
            }
        }
    }

    /// Space: the next weapon carried, then the cannon (stations with the selected weapon are
    /// skipped; the original steps through its 2 or 4 bays, the same with a different weapon in
    /// each).
    fn next_weapon(&mut self) {
        // Entries: the first station of each weapon, then the cannon (index n).
        let n = self.bays.len();
        let first_of = |b: &[(i32, i32)], i: usize| b[i].0 >= 0 && !b[..i].iter().any(|o| o.0 == b[i].0);
        let cur = if self.gun_selected { n } else { self.cur_bay };
        let cur_w = if self.gun_selected { -2 } else { self.bays.get(cur).map_or(-1, |b| b.0) };
        let gun = self.has_gun();
        let next = (1..=n + 1).map(|k| (cur + k) % (n + 1)).find(|&i| if i == n { gun } else { first_of(&self.bays, i) && self.bays[i].0 != cur_w });
        match next {
            Some(i) if i == n => self.gun_selected = true,
            Some(i) => {
                self.cur_bay = i;
                self.gun_selected = false;
            }
            None => {}
        }
    }

    /// The selected weapon (-1 none or the cannon) and its rounds on all stations.
    pub(crate) fn selected_weapon(&self) -> (i32, i32) {
        if self.gun_selected {
            return (-1, 0);
        }
        let w = self.bays.get(self.cur_bay).map_or(-1, |b| b.0);
        if w < 0 {
            return (-1, 0);
        }
        (w, self.bays.iter().filter(|b| b.0 == w).map(|b| b.1).sum())
    }

    /// Missile table index of the selected weapon (0: none or guns).
    pub(crate) fn selected_spec(&self) -> i32 {
        match self.selected_weapon().0 {
            w if w < 0 => 0,
            w => self.weapon_types[(w as usize).min(19)].spec as i32,
        }
    }

    /// Camera keys.
    fn camera_key(&mut self, dir: i32) {
        if self.pflags & pflags::CAMERA_VIEW != 0 {
            return;
        }
        self.targeting.target_killed = false;
        self.target_hold = false;
        self.maverick_view = false;
        self.cam_dir = s16(dir);
        self.air_lock = -1;
        self.gnd_lock = -1;
    }

    /// Alt-R: resupply (repair, reload bays and gun).
    fn resupply(&mut self) {
        self.damage = 0;
        self.hits = 0;
        for b in self.bays.iter_mut() {
            b.1 = if b.0 < 16 { 9 } else { 1 };
        }
        self.gun_ammo = 1000;
        self.emit(SimEvent::Resupplied);
    }

    /// Per-frame systems: JAM damage, IR jammer heat, bay doors.
    pub(crate) fn update_systems(&mut self) {
        if self.damage & dmg::JAM != 0 {
            self.pflags &= !(pflags::ECM | pflags::IRJ);
        }
        if self.irj_on() {
            self.irj_heat += 1;
            if self.irj_heat > 20 * self.fs {
                self.pflags &= !pflags::IRJ;
            }
        } else if self.irj_heat > 0 {
            self.irj_heat -= 1;
        }
        if self.damage & dmg::BD != 0 {
            self.bay_req = true;
            self.door_anim = 2;
        }
        if self.bay_req {
            if self.door_anim == 2 {
                self.pflags |= pflags::BAY_OPEN;
            } else {
                self.door_anim += 1;
            }
        } else if self.door_anim == 0 {
            self.pflags &= !(pflags::BAY_OPEN | pflags::CAMERA_VIEW);
        } else {
            self.door_anim -= 1;
        }
    }

    /// Fire handling at the end of the key handling.
    pub(crate) fn handle_fire(&mut self, ps: &PlayerState) {
        if self.fire_cooldown > 0 {
            self.fire_cooldown -= 1;
        }
        if ps.cmd.fire && self.fire_cooldown == 0 && self.eject == 0 {
            self.fire_missile();
            self.fire_cooldown = 4;
        }
    }

    /// Bomb loft angle (BAM) from the player's pitch and altitude.
    pub(crate) fn loft_angle(&self) -> i32 {
        let num = (0x4000 - abs16(self.p.pitch)) << 12;
        let den = (self.p.view_z + 0x1000) & 0xFFFF;
        s16(cdiv(num, den) - 0x4000)
    }

    /// Release the selected store.
    fn fire_missile(&mut self) {
        if abs16(self.p.roll) > 0x3000 {
            return;
        }
        let (w, count) = self.selected_weapon();
        if count <= 0 {
            return;
        }
        // From the station with the most left (pairs of pylons release in turn).
        let mut bay = self.cur_bay;
        for (i, b) in self.bays.iter().enumerate() {
            if b.0 == w && b.1 > self.bays[bay].1 {
                bay = i;
            }
        }
        let wt = self.weapon_types[(w.max(0) as usize).min(19)].clone();
        let spec = wt.spec as i32;
        if spec == 0 || spec == -1 {
            return;
        }
        if self.pflags & pflags::BAY_OPEN == 0 && spec != -2 {
            self.sound(0x20, 1);
            self.msg("Bay closed");
            return;
        }
        if self.p.alt == 0 || (self.obj(self.nearest_base).flags & oflags::CARRIER != 0 && self.p.alt == 0x80) {
            self.sound(0x20, 1);
            self.msg("Insufficient Altitude");
            return;
        }
        self.bays[bay].1 -= 1;
        if spec == -2 {
            self.emit(SimEvent::FuelTransfer { lbs: 0x76C });
            self.msg("Fuel transfer");
            return;
        }
        self.log_event(4, bay as i32);
        self.emit(SimEvent::CancelTimeAccel);
        let Some(slot) = (8..12).rev().find(|&i| self.proj[i].ttl == 0) else { return };
        let mt = self.missile(spec).clone();
        let fs = self.fs;
        let mut ttl = s16(cdiv((mt.lock_range as i32 * fs) << 5, ((mt.max_speed as i32) >> 6) + 1)) + 6;
        if ttl <= 6 {
            ttl = 999;
        }
        let mut pr = Projectile {
            x: self.p.mx,
            y: self.p.my,
            alt: self.p.view_z - 20,
            speed: self.p.velocity >> 10,
            heading: self.p.heading,
            pitch: self.p.pitch,
            roll: self.p.roll,
            ttl,
            spec,
            weapon: w,
            target: -1,
            launcher: 0,
            tgt_x: -1,
            tgt_y: -1,
            locked: false,
        };
        if spec == 27 {
            self.maverick_view = true;
        }
        if matches!(spec, 28 | 29 | 30 | 38) {
            pr.speed >>= 1;
        }
        if spec == 30 {
            pr.launcher = s16(self.loft_angle() - 0x400);
        }
        let gl = self.gnd_lock;
        if gl >= 0 && (gl as usize) < self.objects.len() {
            if self.targeting.lock_valid {
                let c = mt.class as i32;
                if c == 6 || (c == 4 && self.objects[gl as usize].unit_type != 0) || (c == 5 && self.is_ship(gl)) {
                    pr.target = gl;
                    pr.tgt_x = self.objects[gl as usize].x;
                    pr.tgt_y = self.objects[gl as usize].y;
                }
            } else {
                pr.target = -1;
            }
        }
        if spec == 29 || (spec == 38 && self.opts.fix_original_bugs) {
            // Retarded bombs (and, fixed, supply pods) go straight down at speed 1.
            pr.pitch = s16(0xC000);
            pr.speed = 1;
        }
        self.proj[slot] = pr;
        self.last_missile_slot = slot as i32;
        self.msg(format!("{} fired", wt.long_name));
        self.sound(if mt.lock_range == 0 { 0x10 } else { 0x0D }, 2);
    }

    /// Release a countermeasure `kind`: 1 flare, 2 chaff, 3 decoy.
    pub(crate) fn release_cm(&mut self, kind: usize) {
        let count = self.stores[kind];
        self.stores[kind] -= 1;
        if count <= 0 {
            self.stores[kind] = 0;
            self.msg("Stores exhausted");
            return;
        }
        let slot = if kind == 3 {
            (self.cms[0].ttl == 0 && self.damage & dmg::JAM == 0).then_some(0)
        } else {
            (1..4).rev().find(|&k| self.cms[k].ttl == 0)
        };
        if let Some(k) = slot {
            let mut ttl = (9 - 2 * self.diff) * self.fs;
            let name = match kind {
                1 => {
                    self.flare_lamp = 3 * self.fs;
                    "Flare"
                }
                2 => {
                    self.chaff_lamp = 3 * self.fs;
                    "Chaff"
                }
                _ => {
                    ttl <<= 3;
                    "Decoy"
                }
            };
            self.cms[k] = Countermeasure { x: self.p.mx, y: self.p.my, kind: kind as i32, ttl };
            self.msg(format!("{name} released"));
        }
        self.sound(15, 2);
    }

    /// Velocity of a player round leaving the cannon: the muzzle speed plus the aircraft's
    /// speed along the nose (fine units per frame: east, south, up).
    pub fn gun_round_velocity(heading: i32, pitch: i32, knots: i32, fs: i32) -> (f64, f64, f64) {
        let bam = std::f64::consts::TAU / 65536.0;
        let (h, p) = ((heading as u16) as f64 * bam, (pitch as i16) as f64 * bam);
        let v = (GUN_MUZZLE_SPEED + knots as f64 * 0.514_444) / WU_METRES * ROUND_FINE as f64 / fs as f64;
        (v * h.sin() * p.cos(), -v * h.cos() * p.cos(), v * p.sin())
    }

    /// Gravity on a player round (fine units per frame, each frame).
    pub fn gun_round_gravity(fs: i32) -> i32 {
        (GUN_GRAVITY / WU_METRES * ROUND_FINE as f64 / (fs * fs) as f64).round() as i32
    }

    /// Cannon and all gun rounds: move, then the hit tests.
    pub(crate) fn update_bullets(&mut self, ps: &PlayerState) {
        let fs = self.fs;
        let nb = self.n_bullets;
        let f = ROUND_FINE;
        // A round every frame while the trigger is held (the original fired every other frame);
        // each slot is reused after `nb` frames, which ends the round there.
        if !self.lockheed {
            let slot = (self.frame as usize) % nb;
            let trigger = ps.cmd.gun || (self.gun_selected && ps.cmd.trigger);
            if trigger && self.gun_ammo > 0 && self.eject == 0 {
                self.gun_ammo = clamp_v(self.gun_ammo + cdiv(-20, fs), 0, 1000);
                self.sound(6, 2);
                let (vx, vy, vz) = Self::gun_round_velocity(self.p.heading, self.p.pitch, self.p.knots, fs);
                let (vx, vy, vz) = (vx.round() as i32, vy.round() as i32, vz.round() as i32);
                let (x, y) = (self.p.wx * f, (0x100000 - self.p.wy) * f);
                self.bullets[slot] = Bullet { x: x + vx, y: y + vy, alt: (self.p.view_z - 2) * f + vz, vx, vy, vz };
                self.gun_firing = true;
            } else {
                self.bullets[slot].x = 0;
                self.gun_firing = false;
            }
        }
        let g = Self::gun_round_gravity(fs);
        for (i, b) in self.bullets.iter_mut().enumerate() {
            if b.x != 0 {
                b.x += b.vx;
                b.y += b.vy;
                b.alt += b.vz;
                if i < nb {
                    b.vz -= g;
                }
            }
        }
        // Hit tests (the original only tests rounds that project on screen). A player round
        // hits what passes within the hit radius of its path over this frame.
        let r = GUN_HIT_RADIUS[self.diff.clamp(0, 3) as usize] / WU_METRES;
        for i in 0..self.bullets.len() {
            let b = self.bullets[i];
            if b.x == 0 {
                continue;
            }
            if i < nb {
                for j in 0..self.flights.len() {
                    let u = self.flights[j];
                    // A falling wreck is not shot down again.
                    if u.flags & fflags::AIRBORNE == 0 || u.flags & fflags::DESTROYED != 0 {
                        continue;
                    }
                    let fine = f as f64;
                    let end = [b.x as f64 / fine, b.y as f64 / fine, b.alt as f64 / fine];
                    let step = [b.vx as f64 / fine, b.vy as f64 / fine, b.vz as f64 / fine];
                    let to = [u.wx as f64 - end[0], u.wy as f64 - end[1], u.alt as f64 - end[2]];
                    // Closest point of the segment end - step .. end.
                    let len2 = step.iter().map(|s| s * s).sum::<f64>().max(1e-9);
                    let k = (-(to[0] * step[0] + to[1] * step[1] + to[2] * step[2]) / len2).clamp(0.0, 1.0);
                    let d2: f64 = (0..3).map(|a| (to[a] + k * step[a]).powi(2)).sum();
                    if d2 < r * r {
                        self.flights[j].flags |= fflags::HIT;
                        self.destroy_aircraft(j);
                        let n = self.ac_type(u.spec).name.clone();
                        self.msg(format!("{n} destroyed by gunfire"));
                        self.bullets[i].x = 0;
                        self.flash = Flash { x: u.x, y: u.y, alt: u.alt, timer: 0x10 };
                        self.emit(SimEvent::Explosion { x: u.x, y: u.y, alt: u.alt, kind: 0x10 });
                    }
                }
            } else {
                let (bx, by) = b.map_xy();
                let d = (s16(bx - self.p.mx).abs() + s16(by - self.p.my).abs() + (s16(b.alt / f - self.p.view_z).abs() >> 5)).abs();
                if d < 0x20 {
                    self.msg("Hit by gunfire");
                    if d < 0x20 / (4 - self.diff) {
                        self.player_hit();
                    }
                    let (bx, by) = b.map_xy();
                    self.emit(SimEvent::Explosion { x: bx, y: by, alt: b.alt / f, kind: -1 });
                    self.bullets[i].x = 0;
                    continue;
                }
            }
            let b = self.bullets[i];
            if b.x != 0 && b.alt < 0 {
                let (bx, by) = b.map_xy();
                // Where the path over this frame crossed the ground.
                let above = (b.alt - b.vz) as f64;
                let k = if above > 0.0 { (above / (above - b.alt as f64)).clamp(0.0, 1.0) - 1.0 } else { 0.0 };
                let at = |p: i32, v: i32| p + (v as f64 * k).round() as i32;
                self.emit(SimEvent::RoundImpact { x: at(b.x, b.vx), y: at(b.y, b.vy) });
                self.bullets[i].x = 0;
                let o = self.find_object_at(bx, by);
                if o == -1 || self.obj(o).flags & oflags::DESTROYED != 0 {
                    continue;
                }
                let t = self.last_tile;
                if !self.tile_is_intact(&t) {
                    continue;
                }
                let (tx, ty) = t.map_or((self.obj(o).x, self.obj(o).y), |t| t.map_pos());
                let d = approx(tx - bx, ty - by);
                if self.compat(18, o) * 6 / (self.diff + 2) > d {
                    self.destroy_ground(o);
                    let n = self.place_name(o as usize);
                    self.msg(format!("{n} destroyed by gunfire"));
                    self.emit(SimEvent::Explosion { x: tx, y: ty, alt: 0, kind: 0x10 });
                }
            }
        }
    }
}
