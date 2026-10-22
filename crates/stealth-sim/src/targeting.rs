use crate::math::{abs16, approx, bearing, cdiv, s16};
use crate::types::*;
use crate::{Sim, ballistics};

impl Sim {
    fn cam_off(&self, b: i32) -> i32 {
        abs16(s16(self.pl.cam_dir - b + self.pl.p.heading))
    }

    /// Range (map units) and bearing (BAM) from the player to a map point.
    fn range_bearing(&self, x: i32, y: i32) -> (i32, i32) {
        let dx = s16(x - self.pl.p.mx);
        let dy = s16(y - self.pl.p.my);
        (approx(dx, dy), bearing(dx, s16(-dy)))
    }

    /// Projection to the 320x200 cockpit HUD (focal length 256, boresight (160, 52); assumed to
    /// match the original's HUD projection closely). Returns
    /// (screen x, screen y, depth = -forward/8); screen None when off screen or behind.
    pub(crate) fn project(&self, x: i32, y: i32, alt: i32) -> (Option<(i32, i32)>, i32) {
        let t = &self.trig;
        let de = s16(x - self.pl.p.mx) as i64;
        let dn = -(s16(y - self.pl.p.my) as i64);
        let du = ((alt - self.pl.p.view_z) >> 5) as i64;
        let (sh, ch) = (t.sine(self.pl.p.heading) as i64, t.cosine(self.pl.p.heading) as i64);
        let (sp, cp) = (t.sine(self.pl.p.pitch) as i64, t.cosine(self.pl.p.pitch) as i64);
        let (sr, cr) = (t.sine(self.pl.p.roll) as i64, t.cosine(self.pl.p.roll) as i64);
        let q = 32767i64;
        let f = (dn * ch + de * sh) / q;
        let r = (de * ch - dn * sh) / q;
        let fwd = (f * cp + du * sp) / q;
        let up = (du * cp - f * sp) / q;
        let xs = (r * cr - up * sr) / q;
        let ys = (r * sr + up * cr) / q;
        let de92 = (-fwd / 8) as i32;
        if fwd <= 0 || xs.abs() > fwd {
            return (None, de92);
        }
        let sx = (xs * 256 / fwd) as i32 + 160;
        let syr = (ys * 256 / fwd) as i32;
        let sy = 52 - (syr - (syr >> 2));
        if !(0..=319).contains(&sx) || !(0..=199).contains(&sy) {
            return (None, de92);
        }
        (Some((sx, sy)), de92)
    }

    fn valid_gnd(&self) -> Option<usize> {
        let g = self.pl.gnd_lock;
        (g >= 0 && (g as usize) < self.objects.len()).then_some(g as usize)
    }

    pub(crate) fn update_targeting(&mut self, ps: &PlayerState) {
        self.select_ground();
        self.select_air();
        self.nearest_target();
        self.designate();
        self.lock_oval(ps);
        self.bombsight(ps.ccrp);
        self.photo(ps);
        self.radio_beacon();
        self.pl.prev_fire = ps.cmd.fire;
        // An air lock found since replaces a lock on another pilot, as does his landing.
        if self.pl.air_lock >= 0 || !self.pl.pilot_lock.is_some_and(|k| self.pilot_airborne(k)) {
            self.pl.pilot_lock = None;
        }
        let t = &mut self.pl.targeting;
        t.pilot = self.pl.pilot_lock;
        t.ground = (self.pl.gnd_lock >= 0 && (self.pl.gnd_lock as usize) < self.objects.len())
            .then_some(self.pl.gnd_lock as usize);
        t.air = (self.pl.air_lock >= 0 && (self.pl.air_lock as usize) < self.flights.len())
            .then_some(self.pl.air_lock as usize);
        if let Some(g) = t.ground {
            let o = self.objects[g];
            let (r, b) = {
                let dx = s16(o.x - self.pl.p.mx);
                let dy = s16(o.y - self.pl.p.my);
                (approx(dx, dy), bearing(dx, s16(-dy)))
            };
            self.pl.targeting.range = r;
            self.pl.targeting.bearing = b;
        }
    }

    /// Ground target selection.
    fn select_ground(&mut self) {
        if self.pl.gnd_cooldown > 0 {
            self.pl.gnd_cooldown -= 1;
        }
        if self.pl.target_hold
            || !(self.pl.gnd_lock & 0x80 != 0
                && self.pl.camera_mfd
                && self.pl.gnd_cooldown == 0
                && self.pl.hud_mode != HudMode::Air
                && !self.pl.external_view)
        {
            return;
        }
        let mut best = -1;
        let base_r = 100 << (6 - self.night as i32);
        let mut best_r = self.spot_range(base_r);
        let prev = if self.pl.gnd_lock != -1 { self.pl.gnd_lock - 0x80 } else { -0x100 };
        let mut prev_r = 0;
        if self.pl.gnd_lock != -1 {
            let o = self.obj(prev);
            let (r, b) = self.range_bearing(o.x, o.y);
            prev_r = if self.cam_off(b) > 0x2000 { 0 } else { r - 1 };
        }
        for i in 0..self.objects.len() {
            let o = self.objects[i];
            let (r, b) = self.range_bearing(o.x, o.y);
            let in_win = self.cam_off(b) < 0x2000;
            if !in_win && !(self.pl.hud_mode == HudMode::Nav && prev_r > 0) {
                continue;
            }
            if i as i32 == prev || o.flags & oflags::DESTROYED != 0 || !(r < best_r && r > prev_r) {
                continue;
            }
            if r >= base_r && !self.has_intel(i) {
                continue;
            }
            if self.pl.hud_mode != HudMode::Gnd && (o.flags & 0x501) <= 1 {
                continue;
            }
            if i == 0 && (o.x == 0 || o.y == 0 || o.shape == 0) {
                continue;
            }
            if !self.player_sees(o.x, o.y, 0) {
                continue;
            }
            best = i as i32;
            best_r = r;
        }
        if best == -1 {
            if self.pl.gnd_lock == -1 {
                self.pl.gnd_cooldown = 4;
            } else {
                self.pl.gnd_lock = -1;
            }
        } else {
            self.pl.gnd_lock = best;
        }
    }

    /// Farthest ground target the player can pick: recon intelligence doubles the range for
    /// its objects (campaign).
    fn spot_range(&self, base: i32) -> i32 {
        if self.campaign.is_some() { base * 2 } else { base }
    }

    /// Air target selection.
    fn select_air(&mut self) {
        if self.pl.target_hold {
            return;
        }
        let mut best_r = 0x4B << (6 - self.night as i32);
        let mut best = -1;
        let reselect = self.pl.air_lock & 0x80 != 0;
        let mut prev_r = 0;
        if reselect && self.pl.air_lock != -1 {
            let u = self
                .flights
                .get((self.pl.air_lock - 0x80).max(0) as usize)
                .copied()
                .unwrap_or_default();
            let (r, b) = self.range_bearing(u.x, u.y);
            prev_r = if self.cam_off(b) > 0x2000 { 0 } else { r };
        }
        for (j, u) in self.flights.iter().enumerate() {
            if u.flags & fflags::AIRBORNE == 0 {
                continue;
            }
            let (r, b) = self.range_bearing(u.x, u.y);
            if r >= 0x12C0 || !(reselect || r < 0x100) {
                continue;
            }
            if r < best_r
                && r > prev_r
                && !self.pl.external_view
                && u.flags & fflags::DESTROYED == 0
                && u.speed != 0
                && self.cam_off(b) < 0x2000
                && self.player_sees(u.x, u.y, u.alt)
            {
                best_r = r;
                best = j as i32;
            }
        }
        if best != -1 {
            self.pl.air_lock = best;
        }
        if self.pl.air_lock & 0x80 != 0 {
            self.pl.air_lock = -1;
        }
    }

    /// Nearest enemy ahead (`Commands::nearest_target`, not in the original), once the HUD mode
    /// matches the selected weapon: same window, range and sight rules as the 'B' selection,
    /// but only hostile targets (no friendly or neutral objects, no friendly aircraft).
    fn nearest_target(&mut self) {
        if self.pl.nearest_req == 0 {
            return;
        }
        self.pl.nearest_req -= 1;
        let spec = self.selected_spec();
        // The gun looks for aircraft too.
        let air = self.pl.gun_selected || (spec > 0 && self.missile(spec).class == 7);
        let want = if air { HudMode::Air } else { HudMode::Gnd };
        if self.pl.hud_mode != want || self.pl.external_view {
            return;
        }
        self.pl.nearest_req = 0;
        self.pl.targeting.target_killed = false;
        let mut best = -1;
        if air {
            let mut best_r = 0x12C0;
            for (j, u) in self.flights.iter().enumerate() {
                let (r, b) = self.range_bearing(u.x, u.y);
                if u.flags & fflags::AIRBORNE != 0
                    && u.flags & fflags::DESTROYED == 0
                    && u.speed != 0
                    && self.ac_type(u.spec).radar != -1
                    && r < best_r
                    && self.cam_off(b) < 0x2000
                    && self.player_sees(u.x, u.y, u.alt)
                {
                    best_r = r;
                    best = j as i32;
                }
            }
            if best != -1 {
                self.pl.air_lock = best;
            }
        } else {
            let base_r = 100 << (6 - self.night as i32);
            let mut best_r = self.spot_range(base_r);
            for i in 0..self.objects.len() {
                let o = self.objects[i];
                let (r, b) = self.range_bearing(o.x, o.y);
                if o.flags & (oflags::DESTROYED | oflags::FRIENDLY | oflags::NEUTRAL) != 0
                    || (r >= base_r && !self.has_intel(i))
                    || (i == 0 && (o.x == 0 || o.y == 0 || o.shape == 0))
                    || r >= best_r
                    || self.cam_off(b) >= 0x2000
                    || !self.player_sees(o.x, o.y, 0)
                {
                    continue;
                }
                best = i as i32;
                best_r = r;
            }
            if best != -1 {
                self.pl.gnd_lock = best;
            }
        }
        // Nothing hostile: whatever the original's camera-ahead acquisition locked stays.
        let locked = if air { self.pl.air_lock >= 0 } else { self.valid_gnd().is_some() };
        if !locked {
            self.msg("No target ahead");
        }
    }

    /// 'N' designation.
    fn designate(&mut self) {
        if self.pl.pflags & pflags::DESIGNATE == 0
            || !self.pl.camera_mfd
            || self.pl.hud_mode == HudMode::Air
        {
            return;
        }
        let pitch = self.pl.p.pitch;
        let d = if pitch < 0 {
            cdiv(self.trig.cos_mul(pitch, self.pl.p.view_z), self.trig.sin_mul(s16(-pitch), 32) + 1)
        } else {
            0x280
        };
        let x = self.pl.p.mx + self.trig.sin_mul(self.pl.p.heading, d);
        let y = self.pl.p.my - self.trig.cos_mul(self.pl.p.heading, d);
        let o = self.find_object_at(x, y);
        if o != -1 && !self.player_sees(self.obj(o).x, self.obj(o).y, 0) {
            self.msg("Target obscured");
            self.pl.pflags &= !pflags::DESIGNATE;
            return;
        }
        if o != -1 && (self.pl.hud_mode != HudMode::Nav || (self.obj(o).flags & 0x501) > 1) {
            self.pl.gnd_lock = o;
            if self.smoke_src == 0 {
                self.smoke_src = -1;
            }
            self.pl.pflags &= !pflags::DESIGNATE;
        }
    }

    /// HUD tracking box / oval / LOCK.
    fn lock_oval(&mut self, ps: &PlayerState) {
        if !ps.hud_visible {
            return;
        }
        let t = &mut self.pl.targeting;
        t.lock_valid = false;
        t.in_range = false;
        t.screen = None;
        t.box_colour = 0;
        t.box_size = 0;
        let (w, count) = self.selected_weapon();
        let w = w.max(0);
        let spec = self.selected_spec();
        let (cls, lock) = if spec > 0 {
            let m = self.missile(spec);
            (m.class as i32, m.lock_range as i32)
        } else {
            (-100, 0)
        };
        let mut oval = false;
        let mut red = false;
        if self.pl.hud_mode == HudMode::Gnd && !self.pl.external_view {
            if let Some(g) = self.valid_gnd() {
                let o = self.objects[g];
                let (scr, de92) = self.project(o.x, o.y, 0);
                let mut colour = 0u8;
                let k = self.compat(w, g as i32);
                if k == 0
                    || count == 0
                    || (cls == 4 && o.unit_type == 0)
                    || (cls == 5 && !self.is_ship(g as i32))
                {
                    if spec != -1 {
                        colour = 6;
                    }
                } else if lock > de92 / -7 {
                    if spec == 28 {
                        let (r, _) = self.range_bearing(o.x, o.y);
                        let lim = (self.pl.p.view_z >> 5) * 5;
                        if lim > r && de92 < 0 {
                            oval = true;
                            if lim / 2 > r {
                                red = true;
                                colour = 0x29;
                            }
                        }
                    } else if cls != 7 {
                        let (sx, sy) = scr.unwrap_or((-1, -1));
                        if approx(sx - 0xA0, sy - 0x34) < 0x30 {
                            oval = true;
                            if lock > (-de92) >> 2 {
                                red = true;
                                colour = 0x29;
                            }
                        }
                    }
                }
                let t = &mut self.pl.targeting;
                t.screen = scr;
                t.box_colour = colour;
                t.box_size = if k != 0 { k + 5 } else { 9 };
            }
        } else if self.pl.hud_mode == HudMode::Air
            && !self.pl.external_view
            && self.pl.air_lock >= 0
            && (self.pl.air_lock as usize) < self.flights.len()
        {
            let u = self.flights[self.pl.air_lock as usize];
            let (scr, de92) = self.project(u.x, u.y, u.alt);
            let mut colour = 6;
            if count != 0 && cls == 7 {
                colour = 0;
                let (sx, sy) = scr.unwrap_or((-1, -1));
                if approx(sx - 0xA0, sy - 0x38) < 0x30 && lock > (-de92) >> 3 {
                    oval = true;
                    if lock > (-de92) >> 2 {
                        red = true;
                        colour = 0x29;
                    }
                }
            }
            let t = &mut self.pl.targeting;
            t.screen = scr;
            t.box_colour = colour;
            t.box_size = 9;
        }
        let t = &mut self.pl.targeting;
        t.lock_valid = oval;
        t.in_range = red;
        self.pl.lights.lock = oval && (red || self.frame & 1 != 0);
    }

    /// Where the selected bomb would land if released now (map units with fractions; None
    /// without a ballistic bomb selected or on the ground).
    pub(crate) fn bomb_impact(&self) -> Option<(f64, f64)> {
        let k = ballistics::drag(self.selected_spec(), self.opts.fix_original_bugs)?;
        let v =
            ballistics::release_velocity(self.pl.p.heading, self.pl.p.velocity, self.pl.p.climb);
        let (d, _) = ballistics::impact(
            v,
            k,
            self.pl.p.alt as f64 - ballistics::RELEASE_DROP,
            1.0 / self.fs as f64,
        )?;
        Some((
            (self.pl.p.wx as f64 + d[0]) / 32.0,
            ((0x100000 - self.pl.p.wy) as f64 + d[1]) / 32.0,
        ))
    }

    /// Bombsights: CCIP and ranging bar for free-fall / retarded bombs. The original estimated
    /// the impact point from the release altitude and pitch; it is where the bomb's path
    /// meets the ground ([`ballistics`]).
    fn bombsight(&mut self, ccrp: bool) {
        self.pl.targeting.ccip = None;
        self.pl.targeting.ranging_bar = None;
        self.pl.targeting.ccrp_release = None;
        if self.pl.hud_mode != HudMode::Gnd {
            return;
        }
        let spec = self.selected_spec();
        let Some((ix, iy)) = self.bomb_impact() else { return };
        if ccrp {
            // The ranging bar closes to the release cue as the release point comes up.
            if let Some((_, secs)) = self.ccrp_solution() {
                let frames = (secs * self.fs as f64).round() as i32;
                self.pl.targeting.ccrp_release = Some(frames);
                self.pl.targeting.ranging_bar =
                    Some(((frames / 4).clamp(0, 0x40), frames < 3 * self.fs));
            }
            return;
        }
        let p = (ix as i32, iy as i32);
        self.pl.targeting.ccip = Some(p);
        if let Some(g) = self.valid_gnd() {
            let o = self.objects[g];
            let vz = self.pl.p.view_z;
            let bar = (approx(p.0 - o.x, p.1 - o.y) >> 3).clamp(0, 0x40);
            let flash =
                if spec == 30 { 2 * (1500 - self.pl.p.knots) > vz } else { self.pl.p.knots > vz };
            self.pl.targeting.ranging_bar = Some((bar, flash));
        }
    }

    /// Camera photo.
    fn photo(&mut self, ps: &PlayerState) {
        if self.pl.photo_timer > 0 {
            self.pl.photo_timer -= 1;
        }
        let spec = self.selected_spec();
        if !(spec == -1
            && self.pl.pflags & pflags::BAY_OPEN != 0
            && self.pl.camera_mfd
            && self.pl.gnd_lock != -1)
        {
            self.pl.pflags &= !pflags::CAMERA_VIEW;
            return;
        }
        self.pl.pflags |= pflags::CAMERA_VIEW;
        let t = self.pl.gnd_lock & 0x7F;
        let mut in_view = 0;
        for k in 0..2 {
            let ts = self.pl.targets[k];
            if ts.mission_type == 1 && ts.object == t && self.obj(t).flags & oflags::DESTROYED == 0
            {
                let o = self.obj(t);
                let (r, b) = self.range_bearing(o.x, o.y);
                let a = abs16(s16(b - self.pl.p.heading)) >> 4;
                let bb =
                    abs16(s16(bearing(-(self.pl.p.view_z >> 5), r) - self.pl.p.pitch + 0x6EF)) >> 4;
                if a + bb + r < (4 - self.diff) << 7 {
                    in_view |= k + 1;
                }
            }
        }
        if self.pl.photo_timer == 0 && (ps.cmd.fire || self.pl.prev_fire) {
            self.pl.photo_timer = 4;
            self.pl.photo_frames += 1;
            self.msg(format!("Frame {}", self.pl.photo_frames));
            self.sound(0x12, 2);
            self.emit(SimEvent::PhotoTaken { frame: self.pl.photo_frames });
        }
        if self.pl.photo_timer == 3 && in_view != 0 {
            self.msg("Target photographed.");
            if in_view & 1 != 0 && self.pl.pflags & pflags::PRIMARY_DONE == 0 {
                self.mark_target_reached(0);
                self.log_event(0x8A, t);
            }
            if in_view & 2 != 0 && self.pl.pflags & pflags::SECONDARY_DONE == 0 {
                self.mark_target_reached(1);
                self.log_event(0x4A, t);
            }
        }
    }

    /// Radio beacon / "Drop supplies now".
    fn radio_beacon(&mut self) {
        let per = 8 * self.fs;
        if per == 0 || self.frame % per != 0 {
            return;
        }
        for k in 0..2 {
            let ts = self.pl.targets[k];
            if ts.code & 0x10 == 0 || self.mission_tick >= self.pl.deadline {
                continue;
            }
            let o = self.obj(ts.object);
            let (r, _) = self.range_bearing(o.x, o.y);
            if r < 0xC80 {
                let b = bearing(s16(o.x - self.pl.p.mx), s16(self.pl.p.my - o.y)) / 0xB6;
                self.msg(format!("Radio beacon bearing {b}"));
                if r < 0x100 && self.pl.bays.iter().any(|b| b.0 == SPECIAL_EQUIPMENT && b.1 != 0) {
                    self.msg("Drop supplies now");
                }
            }
        }
    }
}
