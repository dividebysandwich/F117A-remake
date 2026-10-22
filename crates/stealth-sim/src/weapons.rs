use crate::math::{abs16, approx, cdiv, clamp_v, s16};
use crate::types::*;
use crate::{PlayerView, Sim, ballistics};

/// Frames a nearest-target request waits for the HUD to switch to the weapon's mode.
const NEAREST_WAIT: u8 = 8;

impl Sim {
    /// Key handling for this frame's commands.
    pub(crate) fn handle_keys(&mut self, ps: &PlayerState) {
        let c = &ps.cmd;
        // HUD mode changes (F2): entering AIR clears both locks, entering GND the ground lock.
        if self.pl.hud_mode != self.pl.prev_hud_mode {
            match self.pl.hud_mode {
                HudMode::Air => {
                    self.pl.air_lock = -1;
                    self.pl.gnd_lock = -1;
                    self.pl.pilot_lock = None;
                }
                HudMode::Gnd => self.pl.gnd_lock = -1,
                HudMode::Nav => {}
            }
        }
        // After ejecting, the flare key fires the pilot's signal flares (csar.rs).
        if c.flare && self.pl.eject == 0 {
            self.release_cm(1);
        }
        if c.chaff && self.pl.eject == 0 {
            self.release_cm(2);
        }
        if c.decoy && self.pl.eject == 0 {
            self.release_cm(3);
        }
        if c.toggle_irj {
            self.pl.pflags ^= pflags::IRJ;
        }
        if c.toggle_ecm {
            self.pl.pflags ^= pflags::ECM;
        }
        if c.toggle_bay && self.pl.damage & dmg::BD == 0 && !self.pl.external_stores {
            self.pl.bay_req = !self.pl.bay_req;
            self.sound(0x1F, 1);
        }
        if c.next_bay {
            self.next_weapon();
        }
        if c.designate {
            self.pl.target_hold = false;
            self.pl.pflags ^= pflags::DESIGNATE;
            if self.pl.camera_mfd {
                self.camera_key(0);
            }
        }
        if c.next_target {
            self.pl.target_hold = false;
            self.pl.targeting.target_killed = false;
            self.pl.gnd_lock |= 0x80;
            self.pl.air_lock |= 0x80;
            self.pl.pilot_lock = None;
        }
        if let Some(d) = c.camera_dir {
            self.camera_key(d as i32);
        }
        if c.nearest_target {
            self.pl.target_hold = false;
            self.camera_key(0);
            self.pl.nearest_req = NEAREST_WAIT;
        }
        if c.clear_target {
            self.pl.air_lock = -1;
            self.pl.gnd_lock = -1;
            self.pl.pilot_lock = None;
            self.pl.nearest_req = 0;
            self.pl.pflags &= !pflags::DESIGNATE;
            self.pl.target_hold = true;
        }
        if c.toggle_training {
            self.pl.pflags ^= pflags::TRAINING;
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
                self.pl.gnd_lock = o as i32;
                self.pl.target_hold = true;
                self.pl.nearest_req = 0;
                self.pl.targeting.target_killed = false;
                self.pl.pflags &= !pflags::DESIGNATE;
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
                self.pl.gnd_lock = o;
                self.pl.target_hold = true;
                self.pl.nearest_req = 0;
                self.pl.targeting.target_killed = false;
                self.pl.pflags &= !pflags::DESIGNATE;
                if self.smoke_src == 0 {
                    self.smoke_src = -1;
                }
            }
        }
        if let Some(u) = c.lock_unit.map(usize::from) {
            if self.flights.get(u).is_some_and(|f| {
                f.flags & fflags::AIRBORNE != 0 && f.flags & fflags::DESTROYED == 0
            }) {
                self.pl.air_lock = u as i32;
                self.pl.pilot_lock = None;
                self.pl.target_hold = true;
                self.pl.nearest_req = 0;
            } else {
                self.msg("No target");
            }
        }
        if let Some(k) = c.lock_pilot {
            if self.pilot_airborne(k) {
                self.pl.pilot_lock = Some(k);
                self.pl.air_lock = -1;
                self.pl.target_hold = true;
                self.pl.nearest_req = 0;
            } else {
                self.msg("No target");
            }
        }
    }

    /// Space: the next weapon carried, then the cannon (stations with the selected weapon are
    /// skipped; classic mode steps through the 2 or 4 bays, the same with a different weapon in
    /// each).
    fn next_weapon(&mut self) {
        // Entries: the first station of each weapon, then the cannon (index n).
        let n = self.pl.bays.len();
        let first_of =
            |b: &[(i32, i32)], i: usize| b[i].0 >= 0 && !b[..i].iter().any(|o| o.0 == b[i].0);
        let cur = if self.pl.gun_selected { n } else { self.pl.cur_bay };
        let cur_w =
            if self.pl.gun_selected { -2 } else { self.pl.bays.get(cur).map_or(-1, |b| b.0) };
        let gun = self.has_gun();
        let next = (1..=n + 1).map(|k| (cur + k) % (n + 1)).find(|&i| {
            if i == n { gun } else { first_of(&self.pl.bays, i) && self.pl.bays[i].0 != cur_w }
        });
        match next {
            Some(i) if i == n => self.pl.gun_selected = true,
            Some(i) => {
                self.pl.cur_bay = i;
                self.pl.gun_selected = false;
            }
            None => {}
        }
    }

    /// The selected weapon (-1 none or the cannon) and its rounds on all stations.
    pub(crate) fn selected_weapon(&self) -> (i32, i32) {
        if self.pl.gun_selected {
            return (-1, 0);
        }
        let w = self.pl.bays.get(self.pl.cur_bay).map_or(-1, |b| b.0);
        if w < 0 {
            return (-1, 0);
        }
        (w, self.pl.bays.iter().filter(|b| b.0 == w).map(|b| b.1).sum())
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
        if self.pl.pflags & pflags::CAMERA_VIEW != 0 {
            return;
        }
        self.pl.targeting.target_killed = false;
        self.pl.target_hold = false;
        self.pl.maverick_view = false;
        self.pl.cam_dir = s16(dir);
        self.pl.air_lock = -1;
        self.pl.gnd_lock = -1;
    }

    /// Alt-R: resupply (repair, reload bays and gun).
    fn resupply(&mut self) {
        self.pl.damage = 0;
        self.pl.hits = 0;
        self.pl.control_fault = ControlFault::default();
        self.pl.fire_frames = 0;
        self.pl.breakup_frames = 0;
        for b in self.pl.bays.iter_mut() {
            b.1 = if b.0 < 16 { 9 } else { 1 };
        }
        self.pl.gun_ammo = 1000;
        self.emit(SimEvent::Resupplied);
    }

    /// Per-frame systems: JAM damage, IR jammer heat, bay doors.
    pub(crate) fn update_systems(&mut self) {
        if self.pl.damage & dmg::JAM != 0 {
            self.pl.pflags &= !(pflags::ECM | pflags::IRJ);
        }
        if self.irj_on() {
            self.pl.irj_heat += 1;
            if self.pl.irj_heat > 20 * self.fs {
                self.pl.pflags &= !pflags::IRJ;
            }
        } else if self.pl.irj_heat > 0 {
            self.pl.irj_heat -= 1;
        }
        if self.pl.damage & dmg::BD != 0 {
            self.pl.bay_req = true;
            self.pl.door_anim = 2;
        }
        if self.pl.bay_req {
            if self.pl.door_anim == 2 {
                self.pl.pflags |= pflags::BAY_OPEN;
            } else {
                self.pl.door_anim += 1;
            }
        } else if self.pl.door_anim == 0 {
            self.pl.pflags &= !(pflags::BAY_OPEN | pflags::CAMERA_VIEW);
        } else {
            self.pl.door_anim -= 1;
        }
    }

    /// Fire handling at the end of the key handling.
    pub(crate) fn handle_fire(&mut self, ps: &PlayerState) {
        if self.pl.fire_cooldown > 0 {
            self.pl.fire_cooldown -= 1;
        }
        let ccrp = ps.ccrp
            && self.pl.hud_mode == HudMode::Gnd
            && ballistics::drag(self.selected_spec(), self.opts.fix_classic_bugs).is_some();
        if ps.cmd.fire && self.pl.fire_cooldown == 0 && self.pl.eject == 0 {
            if !ccrp {
                self.fire_missile();
                self.pl.fire_cooldown = 4;
            } else if self.ccrp_solution().is_none() {
                self.msg("No target designated");
            }
        }
        // CCRP: with the button held, the bomb goes when the impact point reaches the target.
        let along = if ccrp { self.ccrp_solution().map(|s| s.0) } else { None };
        if let (Some(last), Some(now)) = (self.pl.ccrp_last, along)
            && last > 0.0
            && now <= 0.0
            && ps.cmd.trigger
            && self.pl.eject == 0
        {
            self.fire_missile();
        }
        self.pl.ccrp_last = along;
    }

    /// CCRP release solution against the ground lock: the ground distance (ft) the impact
    /// point of a bomb released now still has to go to the target along the flight path (<= 0
    /// at and past the release point) and the seconds to the release point.
    pub(crate) fn ccrp_solution(&self) -> Option<(f64, f64)> {
        let g = self.pl.gnd_lock;
        let o = self.objects.get(usize::try_from(g).ok()?)?;
        let (ix, iy) = self.bomb_impact()?;
        let v =
            ballistics::release_velocity(self.pl.p.heading, self.pl.p.velocity, self.pl.p.climb);
        let vh = v[0].hypot(v[1]);
        if vh < 1.0 {
            return None;
        }
        // Map units are 32 ft (y south, like the velocity).
        let (dx, dy) = ((o.x as f64 + 0.5 - ix) * 32.0, (o.y as f64 + 0.5 - iy) * 32.0);
        let along = (dx * v[0] + dy * v[1]) / vh;
        Some((along, along / vh))
    }

    /// Release the selected store.
    fn fire_missile(&mut self) {
        if abs16(self.pl.p.roll) > 0x3000 {
            return;
        }
        let (w, count) = self.selected_weapon();
        if count <= 0 {
            return;
        }
        // From the station with the most left (pairs of pylons release in turn).
        let mut bay = self.pl.cur_bay;
        for (i, b) in self.pl.bays.iter().enumerate() {
            if b.0 == w && b.1 > self.pl.bays[bay].1 {
                bay = i;
            }
        }
        let wt = self.weapon_types[(w.max(0) as usize).min(19)].clone();
        let spec = wt.spec as i32;
        if spec == 0 || spec == -1 {
            return;
        }
        if self.pl.pflags & pflags::BAY_OPEN == 0 && spec != -2 {
            self.sound(0x20, 1);
            self.msg("Bay closed");
            return;
        }
        if self.pl.p.alt == 0
            || (self.obj(self.pl.nearest_base).flags & oflags::CARRIER != 0
                && self.pl.p.alt == 0x80)
        {
            self.sound(0x20, 1);
            self.msg("Insufficient Altitude");
            return;
        }
        self.pl.bays[bay].1 -= 1;
        if spec == -2 {
            self.emit(SimEvent::FuelTransfer { lbs: 0x76C });
            self.msg("Fuel transfer");
            return;
        }
        self.log_event(4, bay as i32);
        self.emit(SimEvent::CancelTimeAccel);
        let Some(slot) = (8..12).rev().find(|&i| self.proj[i].ttl == 0) else { return };
        let mt = self.missile(spec).clone();
        let mut pr = self.launch_projectile(w, spec);
        if spec == 27 {
            self.pl.maverick_view = true;
        }
        let gl = self.pl.gnd_lock;
        if gl >= 0 && (gl as usize) < self.objects.len() {
            if self.pl.targeting.lock_valid {
                let c = mt.class as i32;
                if c == 6
                    || (c == 4 && self.objects[gl as usize].unit_type != 0)
                    || (c == 5 && self.is_ship(gl))
                {
                    pr.target = gl;
                    pr.tgt_x = self.objects[gl as usize].x;
                    pr.tgt_y = self.objects[gl as usize].y;
                }
            } else {
                pr.target = -1;
            }
        }
        if ballistics::drag(spec, self.opts.fix_classic_bugs).is_some() {
            // Free-fall and retarded bombs (and, fixed, supply pods) fall ballistically.
            // Classic flew free-fall bombs at half speed, turning down to a floor set by the
            // release altitude, and dropped retarded ones straight down at speed 1.
            let f = ROUND_FINE as f64;
            let v = ballistics::release_velocity(
                self.pl.p.heading,
                self.pl.p.velocity,
                self.pl.p.climb,
            );
            let fall = BombFall {
                x: self.pl.p.wx * ROUND_FINE,
                y: (0x100000 - self.pl.p.wy) * ROUND_FINE,
                alt: ((self.pl.p.alt as f64 - ballistics::RELEASE_DROP) * f) as i32,
                vx: (v[0] * f).round() as i32,
                vy: (v[1] * f).round() as i32,
                vz: (v[2] * f).round() as i32,
            };
            pr.fall = Some(fall);
            // Falls for as long as it takes (from high up, retarded bombs take minutes).
            pr.ttl = 0x7FFF;
        }
        self.proj[slot] = pr;
        // An air-to-air missile fired with another pilot locked goes for him.
        if mt.class == 7 {
            self.proj[slot].target_pilot = self.pl.pilot_lock.filter(|&k| self.pilot_airborne(k));
        }
        if self.guided(spec) {
            // An air-to-air missile goes for the radar's target.
            let lock = self.pl.air_lock;
            if mt.class == 7 && lock >= 0 && (lock as usize) < self.flights.len() {
                self.proj[slot].target = lock;
            }
            let (pos, vel) = self.launch_motion();
            self.start_flight(slot, pos, vel);
        }
        self.pl.last_missile_slot = slot as i32;
        self.msg(format!("{} fired", wt.long_name));
        self.sound(if mt.lock_range == 0 { 0x10 } else { 0x0D }, 2);
    }

    /// Store `w` (missile table entry `spec`) leaving the aircraft now, before its target is set.
    pub(crate) fn launch_projectile(&self, w: i32, spec: i32) -> Projectile {
        let mt = self.missile(spec);
        let fs = self.fs;
        let mut ttl =
            s16(cdiv((mt.lock_range as i32 * fs) << 5, ((mt.max_speed as i32) >> 6) + 1)) + 6;
        if ttl <= 6 {
            ttl = 999;
        }
        let mut pr = Projectile {
            x: self.pl.p.mx,
            y: self.pl.p.my,
            alt: self.pl.p.view_z - 20,
            speed: self.pl.p.velocity >> 10,
            heading: self.pl.p.heading,
            pitch: self.pl.p.pitch,
            roll: self.pl.p.roll,
            ttl,
            spec,
            weapon: w,
            target: -1,
            launcher: 0,
            tgt_x: -1,
            tgt_y: -1,
            locked: false,
            fall: None,
            flight: None,
            target_pilot: None,
        };
        if matches!(spec, 28 | 29 | 30 | 38) {
            pr.speed >>= 1;
        }
        pr
    }

    /// Where a missile under realistic guidance starts (wu: east, south, true altitude) and its
    /// velocity: off the aircraft along its nose with its speed (pull up to lob a missile).
    pub(crate) fn launch_motion(&self) -> ([f64; 3], [f64; 3]) {
        let pos =
            [self.pl.p.wx as f64, (0x100000 - self.pl.p.wy) as f64, self.pl.p.alt as f64 - 20.0];
        let vel = crate::guidance::direction(self.pl.p.heading, self.pl.p.pitch)
            .map(|d| d * self.pl.p.velocity as f64 / 16.0);
        (pos, vel)
    }

    /// Release a countermeasure `kind`: 1 flare, 2 chaff, 3 decoy.
    pub(crate) fn release_cm(&mut self, kind: usize) {
        let count = self.pl.stores[kind];
        self.pl.stores[kind] -= 1;
        if count <= 0 {
            self.pl.stores[kind] = 0;
            self.msg("Stores exhausted");
            return;
        }
        let slot = if kind == 3 {
            (self.pl.cms[0].ttl == 0 && self.pl.damage & dmg::JAM == 0).then_some(0)
        } else {
            (1..4).rev().find(|&k| self.pl.cms[k].ttl == 0)
        };
        if let Some(k) = slot {
            let mut ttl = (9 - 2 * self.diff) * self.fs;
            let name = match kind {
                1 => {
                    self.pl.flare_lamp = 3 * self.fs;
                    "Flare"
                }
                2 => {
                    self.pl.chaff_lamp = 3 * self.fs;
                    "Chaff"
                }
                _ => {
                    ttl <<= 3;
                    "Decoy"
                }
            };
            self.pl.cms[k] =
                Countermeasure { x: self.pl.p.mx, y: self.pl.p.my, kind: kind as i32, ttl };
            self.msg(format!("{name} released"));
        }
        self.sound(15, 2);
    }

    /// Velocity of a player round leaving the cannon: the muzzle speed plus the aircraft's
    /// speed along the nose (fine units per frame: east, south, up).
    pub fn gun_round_velocity(heading: i32, pitch: i32, knots: i32, fs: i32) -> (f64, f64, f64) {
        let bam = std::f64::consts::TAU / 65536.0;
        let (h, p) = ((heading as u16) as f64 * bam, (pitch as i16) as f64 * bam);
        let v = (GUN_MUZZLE_SPEED + knots as f64 * 0.514_444) / WU_METRES * ROUND_FINE as f64
            / fs as f64;
        (v * h.sin() * p.cos(), -v * h.cos() * p.cos(), v * p.sin())
    }

    /// Gravity on a player round (fine units per frame, each frame).
    pub fn gun_round_gravity(fs: i32) -> i32 {
        (GUN_GRAVITY / WU_METRES * ROUND_FINE as f64 / (fs * fs) as f64).round() as i32
    }

    /// A round leaving the cannon of an aircraft at `p`, one frame out.
    pub fn gun_round(p: &PlayerView, fs: i32) -> Bullet {
        let f = ROUND_FINE;
        let (vx, vy, vz) = Self::gun_round_velocity(p.heading, p.pitch, p.knots, fs);
        let (vx, vy, vz) = (vx.round() as i32, vy.round() as i32, vz.round() as i32);
        let (x, y) = (p.wx * f, (0x100000 - p.wy) * f);
        Bullet { x: x + vx, y: y + vy, alt: (p.view_z - 2) * f + vz, vx, vy, vz }
    }

    /// The active pilot's cannon and gun rounds: move, then the hit tests (his kills).
    pub(crate) fn update_own_rounds(&mut self, ps: &PlayerState) {
        let fs = self.fs;
        let nb = self.n_bullets;
        // A round every frame while the trigger is held (classic mode fired every other frame);
        // each slot is reused after `nb` frames, which ends the round there.
        if !self.pl.lockheed {
            let slot = (self.frame as usize) % nb;
            let trigger = ps.cmd.gun || (self.pl.gun_selected && ps.cmd.trigger);
            if trigger && self.pl.gun_ammo > 0 && self.pl.eject == 0 {
                self.pl.gun_ammo = clamp_v(self.pl.gun_ammo + cdiv(-20, fs), 0, 1000);
                self.sound(6, 2);
                self.bullets[slot] = Self::gun_round(&self.pl.p, fs);
                self.pl.gun_firing = true;
                self.pl.fired_step = self.steps;
            } else {
                self.bullets[slot].x = 0;
                self.pl.gun_firing = false;
            }
        }
        let g = Self::gun_round_gravity(fs);
        for b in self.bullets[..nb].iter_mut() {
            if b.x != 0 {
                b.x += b.vx;
                b.y += b.vy;
                b.alt += b.vz;
                b.vz -= g;
            }
        }
        // A coop client's rounds hit on his side (he reports the hits): here they only fly.
        if self.pl.client_rounds {
            for b in self.bullets[..nb].iter_mut().filter(|b| b.alt < 0) {
                b.x = 0;
            }
            return;
        }
        // Hit tests (all rounds, not only those that project on screen). A player round
        // hits what passes within the hit radius of its path over this frame.
        self.credit = Some(self.pl.id);
        for i in 0..nb {
            let b = self.bullets[i];
            if b.x == 0 {
                continue;
            }
            for j in self.round_hits(&b) {
                self.gun_kill(j);
                self.bullets[i].x = 0;
            }
            if self.bullets[i].x != 0
                && let Some(k) = self.round_hits_pilots(&b).first().copied()
            {
                self.gun_hit_pilot(k, Self::round_point(&self.pilot_ref(k as usize).p));
                self.bullets[i].x = 0;
            }
            self.round_on_ground(i);
        }
        self.credit = None;
    }

    /// Gun hit radius (world units).
    pub(crate) fn gun_hit_radius(&self) -> f64 {
        GUN_HIT_RADIUS[self.diff.clamp(0, 3) as usize] / WU_METRES
    }

    /// The airborne flight units a round's path over this frame passes within the hit radius
    /// of.
    pub(crate) fn round_hits(&self, b: &Bullet) -> Vec<usize> {
        (0..self.flights.len())
            .filter(|&j| {
                let u = self.flights[j];
                // A falling wreck is not shot down again.
                u.flags & fflags::AIRBORNE != 0
                    && u.flags & fflags::DESTROYED == 0
                    && self.round_passes(b, (u.wx, u.wy, u.alt))
            })
            .collect()
    }

    /// The other pilots in the air a round's path over this frame passes within the hit
    /// radius of.
    pub(crate) fn round_hits_pilots(&self, b: &Bullet) -> Vec<u8> {
        self.other_pilots()
            .into_iter()
            .filter(|(_, p)| self.round_passes(b, Self::round_point(p)))
            .map(|(k, _)| k)
            .collect()
    }

    /// A pilot's aircraft in the rounds' frame (world units, y growing south, viewZ).
    pub(crate) fn round_point(p: &PlayerView) -> (i32, i32, i32) {
        (p.wx, 0x100000 - p.wy, p.view_z)
    }

    /// A round's path over this frame passes within the hit radius of point `at` (world
    /// units).
    fn round_passes(&self, b: &Bullet, at: (i32, i32, i32)) -> bool {
        let r = self.gun_hit_radius();
        let fine = ROUND_FINE as f64;
        let end = [b.x as f64 / fine, b.y as f64 / fine, b.alt as f64 / fine];
        let step = [b.vx as f64 / fine, b.vy as f64 / fine, b.vz as f64 / fine];
        let len2 = step.iter().map(|s| s * s).sum::<f64>().max(1e-9);
        let to = [at.0 as f64 - end[0], at.1 as f64 - end[1], at.2 as f64 - end[2]];
        // Closest point of the segment end - step .. end.
        let k = (-(to[0] * step[0] + to[1] * step[1] + to[2] * step[2]) / len2).clamp(0.0, 1.0);
        let d2: f64 = (0..3).map(|a| (to[a] + k * step[a]).powi(2)).sum();
        d2 < r * r
    }

    /// The active pilot's gunfire hits pilot `k` (at `at`, world units as the rounds'): a
    /// spark for everybody, and (with friendly fire, at most every quarter second) damage on
    /// his turn.
    pub(crate) fn gun_hit_pilot(&mut self, k: u8, at: (i32, i32, i32)) {
        let (x, y) = (at.0 / 32, at.1 / 32);
        self.emit(SimEvent::Explosion { x, y, alt: at.2, kind: -1 });
        let (me, now) = (self.pl.id, self.steps);
        let every = (self.fs / 4).max(1) as u32;
        self.gun_damage_steps.retain(|&(_, _, s)| now.wrapping_sub(s) < every);
        if !self.gun_damage_steps.iter().any(|&(a, b, _)| (a, b) == (me, k)) {
            self.gun_damage_steps.push((me, k, now));
            self.pilot_hits.push((k, None));
        }
    }

    /// Flight unit `j` shot down by the active pilot's cannon.
    pub(crate) fn gun_kill(&mut self, j: usize) {
        let u = self.flights[j];
        self.flights[j].flags |= fflags::HIT;
        self.destroy_aircraft(j);
        let n = self.ac_type(u.spec).name.clone();
        self.msg(format!("{n} destroyed by gunfire"));
        self.flash = Flash { x: u.x, y: u.y, alt: u.alt, timer: 0x10 };
        self.emit(SimEvent::Explosion { x: u.x, y: u.y, alt: u.alt, kind: 0x10 });
    }

    /// The enemy aircraft's gun rounds: move, then the hit tests against every pilot.
    pub(crate) fn update_enemy_rounds(&mut self, present: &[usize]) {
        let nb = self.n_bullets;
        let f = ROUND_FINE;
        for b in self.bullets[nb..].iter_mut() {
            if b.x != 0 {
                b.x += b.vx;
                b.y += b.vy;
                b.alt += b.vz;
            }
        }
        for i in nb..self.bullets.len() {
            if self.bullets[i].x == 0 {
                continue;
            }
            let mut hit = false;
            for &k in present {
                if present.len() > 1 {
                    self.activate(k);
                }
                let b = self.bullets[i];
                let (bx, by) = b.map_xy();
                let d = (s16(bx - self.pl.p.mx).abs()
                    + s16(by - self.pl.p.my).abs()
                    + (s16(b.alt / f - self.pl.p.view_z).abs() >> 5))
                    .abs();
                if d < 0x20 {
                    self.msg("Hit by gunfire");
                    if d < 0x20 / (4 - self.diff) {
                        self.player_hit(false);
                    }
                    let (bx, by) = b.map_xy();
                    self.emit(SimEvent::Explosion { x: bx, y: by, alt: b.alt / f, kind: -1 });
                    self.bullets[i].x = 0;
                    hit = true;
                    break;
                }
            }
            if !hit {
                self.round_on_ground(i);
            }
        }
    }

    /// Gun round `i` below the ground: its impact, and what it destroys there.
    fn round_on_ground(&mut self, i: usize) {
        let b = self.bullets[i];
        if let Some((x, y)) = b.ground_crossing() {
            self.bullets[i].x = 0;
            self.round_impact(x, y, b.map_xy());
        }
    }

    /// A round of the active pilot's hit the ground at (x, y) (fine units), at map point `at`
    /// below it: dust, and what stands there may go.
    pub(crate) fn round_impact(&mut self, x: i32, y: i32, (bx, by): (i32, i32)) {
        {
            self.emit(SimEvent::RoundImpact { x, y });
            let o = self.find_object_at(bx, by);
            if o == -1 || self.obj(o).flags & oflags::DESTROYED != 0 {
                return;
            }
            let t = self.pl.last_tile;
            if !self.tile_is_intact(&t) {
                return;
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
