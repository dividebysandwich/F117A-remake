use crate::Sim;
use crate::math::{abs16, approx, cdiv, clamp_v, isqrt, s16};
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
        if c.toggle_bay && self.damage & dmg::BD == 0 {
            self.bay_req = !self.bay_req;
            self.sound(0x1F, 1);
        }
        if c.next_bay {
            self.cur_bay = if self.lockheed { self.cur_bay ^ 1 } else { (self.cur_bay + 1) & 3 };
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
        let bay = self.cur_bay;
        if self.bays[bay].1 == 0 {
            return;
        }
        let w = self.bays[bay].0;
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

    /// Cannon and all gun rounds: move, then the HUD-overlay hit tests.
    pub(crate) fn update_bullets(&mut self, ps: &PlayerState) {
        let fs = self.fs;
        let nb = self.n_bullets;
        if !self.lockheed && self.frame & 1 != 0 {
            let slot = ((self.frame >> 1) as usize) % nb;
            if ps.cmd.gun && self.gun_ammo > 0 && self.eject == 0 {
                self.gun_ammo = clamp_v(self.gun_ammo + cdiv(-40, fs), 0, 1000);
                self.sound(6, 2);
                let v = cdiv(0x70, fs);
                let vz = self.trig.sin_mul(self.p.pitch, v) << 5;
                let h = self.trig.cos_mul(self.p.pitch, v);
                let vx = self.trig.sin_mul(self.p.heading, h);
                let vy = -self.trig.cos_mul(self.p.heading, h);
                self.bullets[slot] = Bullet { x: self.p.mx + vx, y: self.p.my + vy, alt: self.p.view_z + vz - 2, vx, vy, vz };
                self.gun_firing = true;
            } else {
                self.bullets[slot].x = 0;
                self.gun_firing = false;
            }
        }
        for b in self.bullets.iter_mut() {
            if b.x != 0 {
                b.x += b.vx;
                b.y += b.vy;
                b.alt += b.vz;
            }
        }
        // Hit tests (the original only tests rounds that project on screen).
        let r = 0x17A / isqrt(4 * (fs + 2));
        for i in 0..self.bullets.len() {
            let b = self.bullets[i];
            if b.x == 0 {
                continue;
            }
            if i < nb {
                for j in 0..self.flights.len() {
                    let u = self.flights[j];
                    if u.flags & fflags::AIRBORNE == 0 {
                        continue;
                    }
                    let d = (s16(b.x - u.x).abs() + s16(b.y - u.y).abs() + (s16(b.alt - u.alt).abs() >> 5)).abs();
                    if d < r / (self.diff + 1) {
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
                let d = (s16(b.x - self.p.mx).abs() + s16(b.y - self.p.my).abs() + (s16(b.alt - self.p.view_z).abs() >> 5)).abs();
                if d < 0x20 {
                    self.msg("Hit by gunfire");
                    if d < 0x20 / (4 - self.diff) {
                        self.player_hit();
                    }
                    self.emit(SimEvent::Explosion { x: b.x, y: b.y, alt: b.alt, kind: -1 });
                    self.bullets[i].x = 0;
                    continue;
                }
            }
            let b = self.bullets[i];
            if b.x != 0 && b.alt < 0 {
                self.emit(SimEvent::Explosion { x: b.x, y: b.y, alt: 0, kind: -1 });
                self.bullets[i].x = 0;
                let o = self.find_object_at(b.x, b.y);
                if o == -1 || self.obj(o).flags & oflags::DESTROYED != 0 {
                    continue;
                }
                let t = self.last_tile;
                if !self.tile_is_intact(&t) {
                    continue;
                }
                let (tx, ty) = t.map_or((self.obj(o).x, self.obj(o).y), |t| t.map_pos());
                let d = approx(tx - b.x, ty - b.y);
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
