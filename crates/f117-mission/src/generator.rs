//! The generator proper: `missionGenerate` (1A18:40F0) and `runGenerator` (1A18:4239),
//! reproducing every `rand()` call (spec §4, §5). Ported 1:1 from tools/re/py/mission_gen.py.
//!
//! All arithmetic follows the original int16 wrap-around; `s16()` marks every place where the
//! 16-bit truncation matters (or may matter).

use crate::rng::MscRand;
use crate::tables::StartTables;
use crate::world::{Candidate, FlightUnit, MissionWorld, TheaterData, WorldObject, flags as F};

/// Truncate to a signed 16-bit value.
#[inline]
pub(crate) fn s16(v: i32) -> i32 {
    v as i16 as i32
}

/// Truncate to a signed 32-bit value.
#[inline]
fn s32(v: i64) -> i32 {
    v as i32
}

/// C (truncating) division; a zero divisor yields 0 instead of a divide fault.
#[inline]
fn cdiv(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

/// `approx(dx, dy)` (1A18:5413): max + min/2, clamped to 0x7FFF.
pub(crate) fn approx(dx: i32, dy: i32) -> i32 {
    let (dx, dy) = (s16(dx).abs(), s16(dy).abs());
    let r = if dy < dx { (dy >> 1) + dx } else { (dx >> 1) + dy };
    r.min(0x7FFF)
}

/// `bearing(dx, dy)` (1A18:5743), identical to the F-15 `calcBearing`.
pub(crate) fn bearing(dx: i32, dy: i32) -> i32 {
    let (dx, dy) = (s16(dx), s16(dy));
    if dx == 0 {
        return if dy > 0 { 0 } else { s16(0x8000) };
    }
    if dy == 0 {
        return if dx > 0 { 0x4000 } else { s16(0xC000) };
    }
    let (adx, ady) = (dx.abs() as i64, dy.abs() as i64);
    let (q, sw) = if adx > ady { ((ady << 14) / adx, true) } else { ((adx << 14) / ady, false) };
    let q = s16(q as i32) as i64;
    let inner = (0x2800 - (((0x1333 - q).abs() * 0xB00) >> 14)) * q;
    let ang = s16((inner >> 14) as i32) as i64;
    let r = match (dx > 0, dy > 0, sw) {
        (true, true, true) => 0x4000 - ang,
        (true, true, false) => ang,
        (true, false, true) => ang + 0x4000,
        (true, false, false) => 0x8000 - ang,
        (false, true, true) => ang + 0xC000,
        (false, true, false) => -ang,
        (false, false, true) => 0xC000 - ang,
        (false, false, false) => ang + 0x8000,
    };
    s16(r as i32)
}

/// `scale(level, c)` (1A18:7C05) on a 32-bit value, no rounding.
fn scale(lv: usize, c: u32) -> u32 {
    match lv {
        0 => c << 1,
        1 => c,
        2 => c >> 2,
        3 => c >> 4,
        _ => c >> 6,
    }
}

/// Target record `targets[k]` (DS:E05E, 18 bytes, spec §5).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RawTarget {
    pub ty: i32,
    pub idx: i32,
    pub base: i32,
    pub code: i32,
    pub num: i32,
    pub coord: [u8; 4],
    pub dist: i32,
}

/// Where the special aircraft (flight unit 0) starts and flies to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpecialRoute {
    pub start: i32,
    pub dest: i32,
}

/// Result of a generator run (everything the hand-off and the briefing need).
#[derive(Debug, Clone)]
pub(crate) struct GenResult {
    pub world: MissionWorld,
    pub targets: [RawTarget; 2],
    pub bd: [i32; 2],
    pub total: i32,
    pub mtime: i32,
    pub tod: i32,
    pub escort: i32,
    pub cbdc: i32,
    pub waypoints: [(i32, i32); 4],
    pub base_xp: i32,
    pub base_yp: i32,
    pub intercept_ref: Option<[u8; 4]>,
    pub special: Option<SpecialRoute>,
    pub weapons: [i32; 4],
    pub weapon_qty: [i32; 4],
    pub fuel: i32,
    pub night: bool,
    pub depart: String,
    pub deadline: String,
    pub deadline2: String,
    pub hostile_conc: bool,
    pub rng: MscRand,
}

pub(crate) struct Gen<'a> {
    x: &'a StartTables,
    th: &'a TheaterData,
    w: MissionWorld,
    r: MscRand,
    theater: i32,
    roe: i32,
    kind: i32,
    diff: i32,
    realism: i32,
    avail: [u8; 20],
    /// DS:CBDC: the special aircraft's start object; a stale global between missions.
    cbdc: i32,
}

/// `rollWeaponAvailability` (1A18:0E23), spec §2. Quirk kept: it tests `avail[map[r]]` but sets
/// `avail[r]`.
pub(crate) fn weapon_availability(rng: &mut MscRand, x: &StartTables, realism: bool) -> [u8; 20] {
    let mut avail = [0u8; 20];
    let n = rng.r(5);
    let mut i = 0;
    while n > i {
        let r = rng.r(16) as usize;
        let m = x.avail_map[r];
        if m != 0xFF && avail[m as usize % 20] == 0 {
            avail[r] = 1;
            i += 1;
        }
    }
    avail[0] = realism as u8;
    avail[1] = realism as u8;
    avail
}

#[derive(PartialEq, Eq)]
enum Stage {
    Restart,
    Primary,
}

impl<'a> Gen<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        x: &'a StartTables,
        th: &'a TheaterData,
        holdrand: u32,
        roe: u8,
        kind: u8,
        diff: u8,
        realism: bool,
        avail: [u8; 20],
        cbdc: i16,
    ) -> Self {
        let mut avail = avail;
        // also forced at 1A18:07F6 / 0E84: no AIM-9 / AIM-120 on the Lockheed
        avail[0] = realism as u8;
        avail[1] = realism as u8;
        Self {
            x,
            th,
            w: th.world.clone(),
            r: MscRand::new(holdrand),
            theater: th.index as i32,
            roe: roe as i32,
            kind: kind as i32,
            diff: diff as i32,
            realism: realism as i32,
            avail,
            cbdc: cbdc as i32,
        }
    }

    // --- object access (Python-style negative indexing for stale/invalid indices) ----------
    fn oi(&self, i: i32) -> usize {
        i.rem_euclid(self.w.objects.len() as i32) as usize
    }
    fn o(&self, i: i32) -> &WorldObject {
        &self.w.objects[self.oi(i)]
    }
    fn om(&mut self, i: i32) -> &mut WorldObject {
        let k = self.oi(i);
        &mut self.w.objects[k]
    }
    fn n(&self) -> i32 {
        self.w.objects.len() as i32
    }
    fn woc(&self) -> i32 {
        self.w.world_object_count as i32
    }
    fn ott(&self, shape: usize) -> i32 {
        self.w.ott(shape)
    }

    /// `dist(a, b)` (1A18:532A).
    fn dist(&self, a: i32, b: i32) -> i32 {
        let (a, b) = (self.o(a), self.o(b));
        approx(a.x as i32 - b.x as i32, a.y as i32 - b.y as i32)
    }

    // --- tile-object search (spec §4) -------------------------------------------------------

    /// `findNearestTileObject(X, Y)` (1A18:7744).
    fn find_nearest(&self, x: i32, y: i32) -> Option<(u8, i32, i32)> {
        let mut best = None;
        let mut bd = 0x7FFF;
        for lv in [1usize, 2] {
            for i in 0..9 {
                let fx = scale(lv, x as u32);
                let fy = scale(lv, y as u32);
                let (frx, fry) = ((fx & 0xFFF) as i32, (fy & 0xFFF) as i32);
                let (dx, dy) = (self.x.dir_dx[i] as i32, self.x.dir_dy[i] as i32);
                let off = |d: i32| self.x.dir_off[(d + 1).clamp(0, 2) as usize] as i32;
                let sx = s16(off(dx) - frx + 0x800);
                let sy = s16(off(dy) - fry + 0x800);
                let gy = s16(((fy >> 12) & 0xFFFF) as i32 + dy);
                let gx = s16(((fx >> 12) & 0xFFFF) as i32 + dx);
                let cell = self.th.lookup(lv, gx, gy);
                if cell == -1 {
                    continue;
                }
                for &(ox, oy, sh) in self.th.tile_objects(lv, cell) {
                    if self.ott(sh as usize) == 0 {
                        continue;
                    }
                    let (mut tx, mut ty) = (s16(ox as i32 + sx), s16(oy as i32 + sy));
                    let mut d = s16(tx.abs() + ty.abs());
                    if lv == 1 {
                        d >>= 2;
                    } else {
                        tx = s16(tx << 2);
                        ty = s16(ty << 2);
                    }
                    if d < bd {
                        bd = d;
                        best = Some((sh, s32(tx as i64 + x as i64), s32(ty as i64 + y as i64)));
                    }
                }
            }
        }
        best
    }

    /// Candidate list of `findRandomObjectOfShape` (1A18:7904).
    fn shape_candidates(&self, shape: usize) -> &'a [Candidate] {
        let th: &'a TheaterData = self.th;
        if self.ott(shape) == 0x10 { &th.cand_lod3[shape] } else { &th.cand_lod12[shape] }
    }

    /// `findRandomObjectOfShape(shape)` (1A18:7904). Returns the result and the list length.
    fn find_random_of_shape(&mut self, shape: usize) -> (Option<(u8, i32, i32)>, usize) {
        let lst = self.shape_candidates(shape);
        if lst.is_empty() {
            return (None, 0);
        }
        let k = if lst.len() > 1 { self.r.r(lst.len() as i32) as usize } else { 0 };
        let c = lst[k];
        let (mut x, mut y) = ((c.a as i64) << 12, (c.e as i64) << 12);
        let (ox, oy) = (c.ox as i32, c.oy as i32);
        match c.level {
            1 => {
                x += s16(ox + 0x800) as i64;
                y += s16(oy + 0x800) as i64;
            }
            2 => {
                x += s16((ox + 0x800) << 2) as i64;
                y += s16((oy + 0x800) << 2) as i64;
            }
            _ => {
                x += s16(ox << 4) as i64 + 0x8000;
                y += s16(oy << 4) as i64 + 0x8000;
            }
        }
        (Some((shape as u8, s32(x), s32(y))), lst.len())
    }

    /// `place(result, slot)` (1A18:512E / 51E8 tail).
    fn place(&mut self, res: Option<(u8, i32, i32)>, slot: i32) -> i32 {
        let Some((sh, xx, yy)) = res else { return -1 };
        let x = s16(xx >> 5);
        let y = s16(-(s16(yy >> 5) - 0x8000));
        for i in 3..self.n() {
            let o = self.o(i);
            if o.x as i32 == x && o.y as i32 == y {
                return i;
            }
        }
        let o = self.om(slot);
        o.x = x as i16;
        o.y = y as i16;
        o.object_idx = (sh as i32 + 0x100) as i16;
        slot
    }

    /// `findOrPlace(x, y)` (1A18:512E): nearest tile object to a map point.
    fn find_or_place(&mut self, x: i32, y: i32, slot: i32) -> i32 {
        let xx = s32((x as i64) << 5);
        let yy = s32(((0x8000 - y) as i64) << 5);
        let res = self.find_nearest(xx, yy);
        self.place(res, slot)
    }

    /// ONC grid reference (1A18:58DF).
    fn grid_ref(&self, x: i32, y: i32) -> [u8; 4] {
        let pre = [b"TD", b"JZ", b"WX", b"CC", b"XV", b"ES", b"KU", b"CU", b"KO"][self.theater as usize];
        let (offx, offy) = match self.theater {
            0 => (6, 4),
            3 => (3, 5),
            _ => (0, 0),
        };
        let mut c = [pre[0] as i32, pre[1] as i32, b'0' as i32, b'0' as i32];
        let mut vx = s16(s16((x >> 5) * 20) >> 10) + offx;
        while vx > 9 {
            vx -= 10;
            c[0] += 1;
        }
        c[2] += vx;
        let mut vy = s16(s16((y >> 5) * 20) >> 10) + offy;
        while vy > 9 {
            vy -= 10;
            c[1] -= 1;
        }
        c[3] += 9 - vy;
        c.map(|v| v as u8)
    }

    fn obj_ref(&self, i: i32) -> [u8; 4] {
        let o = self.o(i);
        self.grid_ref(o.x as i32, o.y as i32)
    }

    /// `positionUnit(u, o)` (1A18:5356).
    fn position_unit(&mut self, u: usize, loc: i32) {
        let o = *self.o(loc);
        let f = &mut self.w.flights[u];
        let p = self.x.plane(f.plane_type);
        f.x = (o.x as i32 + 9) as i16;
        f.y = (o.y as i32 - 12) as i16;
        f.xp = ((f.x as u16 as u32) << 5) as i32;
        f.yp = ((f.y as u16 as u32) << 5) as i32;
        f.alt = if o.flags & F::CARRIER != 0 { 140 } else { 12 };
        f.speed = p.speed;
        f.heading = 0xFC00u16 as i16;
        f.pitch = 0;
        f.roll = 0;
        f.flags |= 0x403;
        f.waypoint = loc as i16;
        let fuel = if f.speed == 0 { 0 } else { ((p.range as i64) << 13) / f.speed as i64 };
        f.fuel = fuel as i16;
    }

    /// `chooseWeapon(class)` (1A18:5282). `None` when no weapon qualifies (a divide fault in
    /// the original, unreachable with the shipped tables).
    fn choose_weapon(&mut self, cat: usize) -> Option<i32> {
        let mut best = 0;
        let mut lst = Vec::new();
        for wpn in (if self.realism != 0 { 2 } else { 0 })..16 {
            if self.avail[wpn] != 0 {
                continue;
            }
            let v = self.x.suit[wpn][cat & 0xF] as i32;
            if v > best {
                best = v & 0xFF;
                lst = vec![wpn as i32];
            } else if v == best {
                lst.push(wpn as i32);
            }
        }
        match lst.len() {
            0 => None,
            1 => Some(lst[0]),
            n => Some(lst[self.r.r(n as i32) as usize]),
        }
    }

    /// Time formatter (1A18:59FE): ticks after 06:00 (day) / 18:00 (night), rounded down to
    /// 5 minutes, no 24 h wrap.
    fn fmt_time(night: bool, t: i32) -> String {
        let night = night as i32;
        let mut c = [b'0' as i32 + night, b'0' as i32, b':' as i32, b'0' as i32, b'0' as i32];
        c[1] += cdiv(t, 1800) % 10;
        c[1] += if night != 0 { 8 } else { 6 };
        if c[1] > b'9' as i32 {
            c[1] -= 10;
            c[0] += 1;
        }
        let m = (cdiv(t, 30) % 60) / 5 * 5;
        c[3] += m / 10;
        c[4] += m % 10;
        c.iter().map(|&v| (v as u8) as char).collect()
    }

    // --- runGenerator (1A18:4239) -----------------------------------------------------------
    pub fn run(mut self) -> GenResult {
        let mut t = [RawTarget::default(); 2];
        let mut mtime: i32 = 0; // DS:D876
        let (mut min_dist, mut attempt, limit) = (450i32, 900i32, 900i32);
        let mut shape: usize = 0;
        let mut bd = [0x7FFFi32; 2];
        let mut stage = Stage::Restart;
        let mut escort;
        let mut total;
        let mut intercept_ref;
        let mut special;
        let strike = self.kind & 1 != 0;
        let training = self.kind & 2 != 0;
        let libya_training = training && self.theater == 0;
        loop {
            // RESTART (4256): new strike category on the first pass / after 50 failures.
            if stage == Stage::Restart {
                attempt += 1;
                if attempt > 50 {
                    attempt = 0;
                    let lst: Vec<usize> = (0..128).filter(|&i| self.ott(i) != 0).collect();
                    loop {
                        shape = lst[self.r.r(lst.len() as i32) as usize];
                        if self.ott(shape) != 0x15 {
                            break;
                        }
                    }
                }
            }
            stage = Stage::Primary;
            // §5.1 primary target (42A8)
            let t0;
            if !strike {
                loop {
                    let c = self.r.r(self.woc() - 3) + 3;
                    if self.o(c).flags & 0xD01 == 1 {
                        t0 = c;
                        break;
                    }
                }
                t[0].idx = t0;
            } else {
                let mut found = -1;
                let mut last = None;
                loop {
                    let (res, count) = self.find_random_of_shape(shape);
                    let c = self.place(res, 1);
                    t[0].idx = c;
                    if c == -1 {
                        break;
                    }
                    let o = *self.o(c);
                    let ok = !(o.flags & 0x801 == 1) && o.shape() == shape;
                    if ok {
                        found = c;
                        break;
                    }
                    // A single candidate draws no random number: the original would retry the
                    // same object forever. Treat it as "nothing found" instead.
                    if count <= 1 && last == Some(c) {
                        break;
                    }
                    last = Some(c);
                }
                if found == -1 {
                    attempt = 900;
                    stage = Stage::Restart;
                    continue;
                }
                t0 = found;
            }
            // §5.2 secondary target (4331 / 4343)
            let mut tries = 0;
            let mut t1;
            loop {
                let (p0x, p0y) = (self.o(t0).x as i32, self.o(t0).y as i32);
                let (mut x, mut y);
                loop {
                    x = s16(self.r.r(0x3200) + p0x - 0x1900).clamp(0x840, 0x77BF);
                    y = s16(self.r.r(0x3200) + p0y - 0x1900).clamp(0x840, 0x77BF);
                    if self.w.grid[((y >> 11) * 16 + (x >> 11)) as usize] & 3 == 0 {
                        break;
                    }
                }
                t1 = self.find_or_place(x, y, 2);
                t[1].idx = t1;
                let ok = t1 != -1 && !(strike && self.o(t1).flags & 0x801 == 1);
                tries += 1;
                if tries > 100 || ok {
                    break;
                }
            }
            if tries > 100 {
                stage = Stage::Restart;
                continue;
            }
            let (p0, p1) = (*self.o(t0), *self.o(t1));
            if (p1.x == p0.x && p1.y == p0.y) || s16(self.dist(t0, t1) & 0xFFC0) > 0x6400 {
                continue; // back to PRIMARY
            }
            // §5.3 bases (4455)
            for slot in 0..2 {
                bd[slot] = 0x7FFF;
                for i in self.woc()..self.n() {
                    let o = *self.o(i);
                    let f = o.flags as i32;
                    if f & 0x500 == 0 || f & 0x201 == 0 {
                        continue;
                    }
                    if self.theater == 7 && o.name == 0x11 {
                        continue;
                    }
                    if self.realism != 0 && f & 0x200 != 0 {
                        continue;
                    }
                    let mut d = if slot == 0 {
                        s16(self.dist(t[0].idx, i) + ((self.r.r(150) - 75) << 6))
                    } else {
                        self.dist(t[slot].idx, i)
                    };
                    if f & 0x100 != 0 {
                        d = s16(d + ((self.r.r(100) + 50) << 6));
                    }
                    // F-15 clampValue quirk: d <= -0x4000 -> 0x7FFF
                    d = if d > 0x7FFF {
                        0x7FFF
                    } else if d >= 0 {
                        d
                    } else if d > -0x4000 {
                        0
                    } else {
                        0x7FFF
                    };
                    if d < 0x7FFF {
                        let r = s16(self.r.r(0x500) + d);
                        let thr = s16(bd[slot] + if f & 0x200 != 0 { 0xC80 } else { 0 });
                        if thr > r {
                            t[slot].base = i;
                            bd[slot] = d;
                        }
                    }
                }
            }
            if libya_training {
                let v = if strike { [3, 15, 0x22, 0x21] } else { [0x16, 8, 0x22, 0x23] };
                (t[0].idx, t[1].idx, t[0].base, t[1].base) = (v[0], v[1], v[2], v[3]);
                bd[0] = self.dist(t[0].idx, t[0].base);
                bd[1] = self.dist(t[1].idx, t[1].base);
            }
            // length check
            total = s16((self.dist(t[0].idx, t[1].idx) >> 6) + (bd[1] >> 6) + (bd[0] >> 6));
            if total > limit + attempt || total < min_dist {
                min_dist = s16(min_dist + self.diff - 5);
                stage = Stage::Restart;
                continue;
            }
            // §5.4 mission table (465C)
            for slot in 0..2 {
                t[slot].ty = 0;
                let mut choice = 0;
                for rep in 0..2 {
                    let mut cnt = 0;
                    let cat = self.ott(self.o(t[slot].idx).shape());
                    for (row, m) in self.x.missions.iter().enumerate() {
                        if m.theater_mask as i32 & (1 << self.theater) == 0 || m.roe_mask as i32 & (1 << self.roe) == 0 {
                            continue;
                        }
                        if cat != m.category as i32 {
                            continue;
                        }
                        if slot != 0 && t[0].num == row as i32 {
                            continue;
                        }
                        if rep == 1 && choice == cnt {
                            t[slot].ty = m.mission_type as i32;
                            t[slot].num = row as i32;
                            t[slot].code = m.code as i32 + if m.special > 0 { (m.special as i32) << 8 } else { 0 };
                        }
                        cnt += 1;
                    }
                    choice = if cnt != 0 { self.r.r(cnt) } else { 0 };
                }
            }
            if libya_training {
                let row = if strike { 5 } else { 55 };
                let m = self.x.missions[row];
                (t[0].ty, t[0].num, t[0].code) = (m.mission_type as i32, row as i32, m.code as i32);
            }
            // rejections
            if (strike && (t[0].ty >= 5 || t[1].ty >= 5)) || t[0].ty == 0 || t[1].ty == 0 {
                attempt = 900;
                stage = Stage::Restart;
                continue;
            }
            if t[1].num == t[0].num {
                stage = Stage::Restart;
                continue;
            }
            if bd[1] > bd[0] && !training {
                t.swap(0, 1);
                bd.swap(0, 1);
            }
            if t[1].code & 2 != 0
                || (5..=8).contains(&t[1].ty)
                || (t[0].ty == 4 && self.diff == 0)
                || (t[0].code & 8 != 0 && t[1].code & 8 != 0)
            {
                stage = Stage::Restart;
                continue;
            }
            // deadline
            if t[0].code & 2 != 0 {
                mtime = (self.dist(t[0].base, t[0].idx) >> 4) + 450;
            }
            if t[1].code & 2 != 0 {
                mtime = (self.dist(t[0].base, t[1].idx) >> 4) + 450;
            }
            // §5.5 special aircraft (flight unit 0)
            escort = -1; // DS:CABC
            let c5 = self.x.missions[t[0].num as usize].special as i32;
            if c5 < 0 {
                self.w.flights[0].plane_type = (-c5) as i16;
            }
            let ty = t[0].ty;
            intercept_ref = None;
            special = None;
            if ty == 5 {
                let mut best = 0x7FFF;
                self.cbdc = -1;
                for i in 0..self.woc() {
                    let d = s16(self.dist(t[0].idx, i) - bd[0]).abs();
                    let f = self.o(i).flags;
                    if d < best && f & F::AIRFIELD != 0 && f & F::NEUTRAL == 0 {
                        self.cbdc = i;
                        best = d;
                    }
                }
                if self.cbdc == -1 {
                    stage = Stage::Restart;
                    continue;
                }
                self.position_unit(0, self.cbdc);
                self.w.flights[0].waypoint = t[0].idx as i16;
                self.w.flights[0].flags |= 4;
                escort = 0;
                intercept_ref = Some(self.obj_ref(self.cbdc));
                special = Some(SpecialRoute { start: self.cbdc, dest: t[0].idx });
                let sp = self.w.flights[0].speed as i32 >> 6;
                mtime = cdiv(self.dist(self.cbdc, t[0].idx), 3 * sp);
            }
            if ty == 6 || ty == 7 {
                let mut best = 0x7FFF;
                for i in 3..self.n() {
                    let f = self.o(i).flags as i32;
                    if f & 0x500 == 0 || f & 0xA00 != 0 {
                        continue;
                    }
                    let d = s16(self.dist(t[0].idx, i) - bd[0]).abs();
                    if d < best && i != t[0].base {
                        self.cbdc = i;
                        best = d;
                    }
                }
                intercept_ref = Some(self.obj_ref(self.cbdc));
                if ty == 7 {
                    self.position_unit(0, t[0].idx);
                    self.w.flights[0].waypoint = self.cbdc as i16;
                    self.w.flights[0].flags |= 4;
                    escort = 0;
                    special = Some(SpecialRoute { start: t[0].idx, dest: self.cbdc });
                    self.cbdc = t[0].idx;
                } else {
                    self.position_unit(0, self.cbdc);
                    self.w.flights[0].waypoint = t[0].idx as i16;
                    self.w.flights[0].flags |= 4;
                    escort = 0;
                    special = Some(SpecialRoute { start: self.cbdc, dest: t[0].idx });
                    let sp = (self.w.flights[0].speed as i32 & 0xFFDF) >> 5;
                    mtime = cdiv(self.dist(self.cbdc, t[0].idx), sp);
                }
            }
            if ty == 8 {
                self.position_unit(0, t[0].idx);
                if self.w.flights[0].plane_type == 2 && self.theater == 1 {
                    self.w.flights[0].plane_type = 12; // MiG-29 -> F-14 in the Gulf
                }
                self.w.flights[0].flags |= 0x40;
                escort = 0;
                self.w.flights[0].waypoint = t[0].idx as i16;
                self.cbdc = t[0].idx;
                special = Some(SpecialRoute { start: t[0].idx, dest: t[0].idx });
            }
            if escort == 0 {
                self.w.flights[0].fuel = 0x4E1F;
            }
            break;
        }

        // ---- §5.6 post-selection (4AC9) -----------------------------------------------------
        for tk in &mut t {
            let ti = tk.idx;
            tk.coord = self.obj_ref(ti);
            if ti < 3 {
                // a scratch target takes the name of the nearest named non-base object
                let mut best = 0x7FFF;
                for i in 3..self.n() {
                    if self.o(i).flags & 0x500 != 0 {
                        continue;
                    }
                    let d = self.dist(i, ti);
                    if d < best && self.o(i).name != 0 {
                        best = d;
                        self.om(ti).name = self.o(i).name;
                    }
                }
            }
        }
        t[0].dist = mtime >> 4;
        let b0 = *self.o(t[0].base);
        let b1 = *self.o(t[1].base);
        let base_xp = (b0.x as u16 as i32) << 5;
        let base_yp = s32(((0x8000 - b0.y as i64) << 5) - if b0.flags & F::CARRIER != 0 { 0 } else { 0x708 });
        let p0 = *self.o(t[0].idx);
        let (mut tx, mut ty) = (p0.x as i32, p0.y as i32);
        let mid = (((b0.x as u16 as i32) >> 1) + ((tx & 0xFFFF) >> 1), ((b0.y as u16 as i32) >> 1) + ((ty & 0xFFFF) >> 1));
        if t[0].code & 0x10 != 0 {
            tx = (tx & 0xFC00) + 0x200;
            ty = (ty & 0xFC00) + 0x200;
        }
        let p1 = *self.o(t[1].idx);
        let waypoints = [mid, (tx, ty), (p1.x as i32, p1.y as i32), (b1.x as i32, b1.y as i32)];

        // ---- §5.7 enemy flights (4C3D) ------------------------------------------------------
        let nf = self.w.flights.len();
        for i in 0..nf.saturating_sub(4) {
            if self.w.flights[i].flags & 0x80 != 0 {
                let mut rng = if self.w.flights[i].flags & 0x40 != 0 { bd[0] << 1 } else { cdiv(bd[0], 4) * (4 - self.diff) };
                rng = s16(rng);
                let r = loop {
                    let r = self.r.r(self.woc() - 3) + 3;
                    let o = *self.o(r);
                    if o.flags & F::NEUTRAL != 0 {
                        continue;
                    }
                    let d = approx(mid.0 - o.x as i32, mid.1 - o.y as i32);
                    rng = s16(rng + 16);
                    if d <= rng {
                        break r;
                    }
                };
                self.position_unit(i, r);
                // first non-friendly object within 0x3000 of the bearing to the takeoff base
                let best = 0x3000;
                let f: FlightUnit = self.w.flights[i];
                let bb = bearing(b0.x as i32 - f.x as i32, f.y as i32 - b0.y as i32);
                for _ in 0..8 {
                    let wpi = self.r.r(self.woc()) + 1;
                    let o = *self.o(wpi);
                    if o.flags & F::FRIENDLY != 0 {
                        continue;
                    }
                    let bg = bearing(o.x as i32 - f.x as i32, f.y as i32 - o.y as i32);
                    let df = s16(-s16(bg - bb)).abs();
                    if df < best {
                        self.w.flights[i].waypoint = wpi as i16;
                        break;
                    }
                }
            }
            if self.w.flights[i].flags & 0x100 != 0 && escort != -1 {
                self.position_unit(i, self.cbdc);
                self.w.flights[i].fuel = 0x4E1F;
            }
            if i != 0 {
                let mut cnt = 0;
                let r = loop {
                    let r = self.r.r(self.woc() - 3) + 3;
                    let o = self.o(r);
                    if o.flags & 0x801 == 1 && o.patrol_count == 0 {
                        break r;
                    }
                    let c = cnt;
                    cnt += 1;
                    if c >= 20 {
                        break r;
                    }
                };
                let pt = self.w.flights[i].plane_type;
                let pc = self.r.r(self.theater + 1) + 1;
                let o = self.om(r);
                o.occupant_type = pt;
                o.patrol_count = pc as i16;
            }
        }

        // ---- §5.8 threat upgrades (4E0D) and the grid (4EC6) --------------------------------
        for i in 0..(self.w.ground_unit_count as i32).min(self.n()) {
            let u = self.o(i).unit_type as i32;
            if u == 0 || u == 21 {
                continue;
            }
            let k = self.r.r(5) + (self.roe != 0) as i32 + self.diff;
            let remap = self.x.unit_remap.get(u as usize).copied();
            let nu = match (k, remap) {
                (0 | 1 | 3, Some((_, down))) => down as i32,
                (5 | 7 | 8, Some((up, _))) => up as i32,
                _ => u,
            };
            self.om(i).unit_type = nu as i16;
            if self.o(i).flags & F::SHIP != 0 && self.roe + self.diff + 2 < self.r.r(10) {
                self.om(i).unit_type = 0;
            }
        }
        for col in 0..16 {
            for row in 0..16 {
                let g = row * 16 + col;
                if self.w.grid[g] & 0x10 != 0 && self.r.r(5) >= self.diff {
                    self.w.grid[g] &= 0xEF;
                }
            }
        }

        // ---- §5.9 weapons, fuel, night, times (4F0F) ----------------------------------------
        let mut wt = [1, 2, 0, 0];
        wt[2] = if self.r.r(2) == 0 { 5 } else { 9 };
        let fuel = s16(12 * total);
        if fuel > 10000 {
            wt[2] = 0x11;
        }
        let mut photo = false;
        for k in 0..2 {
            let mut wv = None;
            if t[k].ty == 1 && !photo {
                wv = Some(0x10);
                photo = true;
            }
            if t[k].ty == 3 || t[k].ty == 4 {
                wv = Some(0x12);
            }
            if t[k].ty == 2 {
                let sh = self.o(t[k].idx).shape();
                let class = self.w.buf7.get(sh).copied().unwrap_or(0) & 0xF;
                wv = self.choose_weapon(class as usize);
            }
            if let Some(v) = wv {
                wt[k] = v;
            }
        }
        if libya_training {
            (wt[0], wt[2]) = if strike { (5, 1) } else { (1, 5) };
            wt[3] = if fuel > 10000 { 0x11 } else { 0 };
        }
        let weapon_qty = wt.map(|v| self.x.weapon_qty.get(v as usize).copied().unwrap_or(0) as i32);
        let tod = ((t[1].num + t[0].num) & 0xF) << 8;
        let night = if t[0].ty == 4 || t[1].ty == 4 {
            true
        } else if self.realism != 0 {
            self.r.r(10) != 0
        } else {
            self.r.r(10) > 2
        };
        let depart = Self::fmt_time(night, tod); // DS:0D88
        let deadline = Self::fmt_time(night, mtime + tod); // DS:0D7C
        let deadline2 = Self::fmt_time(night, mtime + tod + 450); // DS:0D82
        mtime -= (mtime + tod) % 150;

        // ---- §5.10 missionGenerate tail (4121) ----------------------------------------------
        let hostile_conc = self.w.grid.iter().any(|g| g & 0x10 != 0);
        for o in &mut self.w.objects {
            o.flags &= !0xC0;
        }
        let mut k = self.r.r(3);
        while k != 0 {
            let r = self.r.r(self.n());
            let o = *self.o(r);
            if r != t[0].idx && r != t[1].idx && o.unit_type != 0 && o.flags & F::FRIENDLY == 0 && o.flags & F::NEUTRAL == 0 {
                self.om(r).flags |= F::OFF_THE_AIR;
                k -= 1;
            }
        }
        for i in 0..self.w.objects.len() {
            if self.w.objects[i].flags & F::OFF_THE_AIR != 0 && self.r.r(3) == 0 {
                let o = &mut self.w.objects[i];
                o.flags |= F::UNDER_REPAIR;
                o.unit_type = 0;
            }
        }

        GenResult {
            world: self.w,
            targets: t,
            bd,
            total,
            mtime: s16(mtime),
            tod,
            escort,
            cbdc: self.cbdc,
            waypoints,
            base_xp,
            base_yp,
            intercept_ref,
            special,
            weapons: wt,
            weapon_qty,
            fuel,
            night,
            depart,
            deadline,
            deadline2,
            hostile_conc,
            rng: self.r,
        }
    }
}
