/// Wrap to a signed 16-bit value.
#[inline]
pub fn s16(v: i32) -> i32 {
    v as i16 as i32
}

/// Wrap to an unsigned 16-bit value.
#[inline]
pub fn u16v(v: i32) -> i32 {
    v as u16 as i32
}

/// `hi8s(v)`: high byte of a 16-bit word as a signed int8.
#[inline]
pub fn hi8s(v: i32) -> i32 {
    ((v as u16 >> 8) as u8 as i8) as i32
}

/// `abs` of a 16-bit word (abs(-0x8000) stays -0x8000).
#[inline]
pub fn abs16(v: i32) -> i32 {
    s16(s16(v).abs())
}

/// Sign of a 16-bit word: -1 / 0 / 1.
#[inline]
pub fn sign(v: i32) -> i32 {
    s16(v).signum()
}

/// Approximate distance `max + min/2`, capped at 0x7FFF.
pub fn approx(dx: i32, dy: i32) -> i32 {
    let a = s16(dx).abs();
    let b = s16(dy).abs();
    (a.max(b) + (a.min(b) >> 1)).min(0x7FFF)
}

/// Bearing of (x, y): x = east, y = north, returns a u16 BAM. Integer arctangent
/// approximation.
pub fn bearing(x: i32, y: i32) -> i32 {
    let x = s16(x);
    let y = s16(y);
    if x == 0 {
        return if y > 0 { 0 } else { 0x8000 };
    }
    if y == 0 {
        return if x > 0 { 0x4000 } else { 0xC000 };
    }
    let (ax, ay) = (x.abs(), y.abs());
    let (r, sw) = if ay < ax { ((ay << 14) / ax, true) } else { ((ax << 14) / ay, false) };
    let r = s16(r);
    let t = ((0x2800 - ((s16(0x1333 - r).abs() * 0xB00) >> 14)) * r) >> 14;
    let t = t & 0xFFFF;
    let res = if x > 0 {
        if y > 0 {
            if sw { 0x4000 - t } else { t }
        } else if sw {
            t + 0x4000
        } else {
            0x8000 - t
        }
    } else if y > 0 {
        if sw { t - 0x4000 } else { -t }
    } else if sw {
        0xC000 - t
    } else {
        t + 0x8000
    };
    res & 0xFFFF
}

/// Clamp with a quirk: `v > hi → hi; v >= lo → v;
/// v <= -0x4000 → hi; else lo`. All compares are signed 16-bit.
pub fn clamp_q(v: i32, lo: i32, hi: i32) -> i32 {
    let (v, lo, hi) = (s16(v), s16(lo), s16(hi));
    if v > hi {
        hi
    } else if v >= lo {
        v
    } else if v <= -0x4000 {
        hi
    } else {
        lo
    }
}

/// Plain clamp.
pub fn clamp_v(v: i32, lo: i32, hi: i32) -> i32 {
    v.max(lo).min(hi)
}

/// Integer square root: Newton iteration seeded with v>>2, 1 for v < 4.
pub fn isqrt(v: i32) -> i32 {
    if v < 4 {
        return 1;
    }
    let mut x = v >> 2;
    loop {
        let nx = (x + v / x) >> 1;
        if nx >= x {
            return x;
        }
        x = nx;
    }
}

/// Q15 fixed-point multiply: `(a*b) >> 15` plus bit 14 (rounding).
#[inline]
pub fn fixed_mul(a: i32, b: i32) -> i32 {
    let p = s16(a) * s16(b);
    s16((p >> 15) + ((p >> 14) & 1))
}

/// C integer division truncating toward zero, with a zero divisor giving 0
/// (never reached on the ported paths).
#[inline]
pub fn cdiv(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

/// Compressed altitude used by all threat code: `alt < 0x2000 ? alt :
/// alt < 0x4000 ? 0x2000 + (alt-0x2000)/2 : 0x3000 + (alt-0x4000)/4`.
pub fn view_z(alt: u16) -> i32 {
    let a = alt as i32;
    if a < 0x2000 {
        a
    } else if a < 0x4000 {
        0x2000 + (a - 0x2000) / 2
    } else {
        0x3000 + (a - 0x4000) / 4
    }
}

/// [`view_z`] of a fractional altitude (negative below the ground).
pub fn view_z_f(alt: f64) -> f64 {
    if alt < 8192.0 {
        alt
    } else if alt < 16384.0 {
        8192.0 + (alt - 8192.0) / 2.0
    } else {
        12288.0 + (alt - 16384.0) / 4.0
    }
}

/// Sine table (Q15, 256 steps per turn + 1 guard entry) with an
/// interpolating lookup and fixed-point `x·sin` / `x·cos` products.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Trig {
    pub lut: Vec<i16>,
}

impl Trig {
    /// `sine(a)`: `lut[a>>8] + round((lut[i+1]-lut[i]) * (a&0xFF) / 256)`.
    pub fn sine(&self, a: i32) -> i32 {
        let a = a as u16;
        let i = (a >> 8) as usize;
        let f = (a & 0xFF) as i32;
        let v0 = self.lut[i] as i32;
        let v1 = self.lut[i + 1] as i32;
        s16(v0 + (((v1 - v0) * f + 0x80) >> 8))
    }

    pub fn cosine(&self, a: i32) -> i32 {
        self.sine(a + 0x4000)
    }

    /// `x·sin(a)`.
    pub fn sin_mul(&self, a: i32, x: i32) -> i32 {
        fixed_mul(self.sine(a), x)
    }

    /// `x·cos(a)`.
    pub fn cos_mul(&self, a: i32, x: i32) -> i32 {
        self.sin_mul(a + 0x4000, x)
    }

    /// Analytic table, used only when no game data is available (tests). Equal to the
    /// game data's table to within rounding.
    pub fn analytic() -> Self {
        let lut = (0..257)
            .map(|i| {
                if i % 256 == 0 || i == 128 {
                    0
                } else {
                    (((i as f64) * std::f64::consts::PI * 2.0 / 256.0).sin() * 32767.0).round()
                        as i16
                }
            })
            .collect();
        Self { lut }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearing_quadrants() {
        assert_eq!(bearing(0, 10), 0);
        assert_eq!(bearing(0, -10), 0x8000);
        assert_eq!(bearing(10, 0), 0x4000);
        assert_eq!(bearing(-10, 0), 0xC000);
        // Bit-exact reference values.
        assert_eq!(bearing(100, 100), 0x204D);
        assert_eq!(bearing(100, -100), 0x5FB3);
        assert_eq!(bearing(-100, -100), 0xA04D);
        assert_eq!(bearing(-100, 100), 0xDFB3);
        assert_eq!(bearing(300, -700), 0x6F77);
        assert_eq!(bearing(-5, 9000), 0xFFFB);
    }

    #[test]
    fn bearing_atan_table() {
        // Radar look-down table: hi8s(bearing(viewZ/3 * (diff+1), 0x800)).
        let t = |alt: i32, d: i32| hi8s(bearing(u16v((alt / 3) * (d + 1)), 0x800));
        assert_eq!([t(0, 0), t(0, 3)], [0, 0]);
        assert_eq!([t(100, 0), t(100, 1), t(100, 2), t(100, 3)], [0, 1, 1, 2]);
        assert_eq!([t(500, 0), t(500, 1), t(500, 2), t(500, 3)], [3, 6, 9, 12]);
        assert_eq!([t(1000, 0), t(1000, 1), t(1000, 2), t(1000, 3)], [6, 12, 18, 23]);
        assert_eq!([t(2000, 0), t(2000, 1), t(2000, 2), t(2000, 3)], [12, 23, 31, 37]);
        assert_eq!([t(5000, 0), t(5000, 1), t(5000, 2), t(5000, 3)], [27, 41, 48, 51]);
        assert_eq!([t(9096, 0), t(9096, 1), t(9096, 2), t(9096, 3)], [39, 50, 55, 57]);
        assert_eq!([t(13192, 0), t(13192, 1), t(13192, 2), t(13192, 3)], [46, 54, 58, 59]);
        assert_eq!([t(23192, 0), t(23192, 1), t(23192, 2), t(23192, 3)], [53, 58, 60, 61]);
        // Lockheed constant 2 / 4 / 7 / 9.
        let l: Vec<i32> = (0..4).map(|d| hi8s(bearing((d + 1) << 7, 0x800))).collect();
        assert_eq!(l, [2, 4, 7, 9]);
    }

    #[test]
    fn view_z_compression() {
        assert_eq!(view_z(5000), 5000);
        assert_eq!(view_z(10000), 9096);
        assert_eq!(view_z(20000), 13192);
        assert_eq!(view_z(60000), 23192);
    }

    #[test]
    fn clamp_quirk() {
        assert_eq!(clamp_q(-0x5000, -0x100, 0x100), 0x100);
        assert_eq!(clamp_q(-0x3000, -0x100, 0x100), -0x100);
        assert_eq!(clamp_q(0x50, -0x100, 0x100), 0x50);
    }

    #[test]
    fn approx_and_isqrt() {
        assert_eq!(approx(30, -40), 55);
        assert_eq!(approx(0x7000, 0x7000), 0x7FFF);
        assert_eq!(isqrt(68), 8);
        assert_eq!(isqrt(2), 1);
    }
}
