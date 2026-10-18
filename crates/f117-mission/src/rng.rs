//! MSC C runtime `rand()` as used by START.EXE (spec §2).

use serde::{Deserialize, Serialize};

/// MSC 5.x `rand()` (1A18:96CE): `holdrand = holdrand * 214013 + 2531011`, result
/// `(holdrand >> 16) & 0x7FFF`. START's `random(n)` (1A18:76C0) is `rand() % n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MscRand {
    /// `holdrand` (DS:AE8C).
    pub state: u32,
    /// Number of `rand()` calls made so far.
    pub calls: u32,
}

impl MscRand {
    /// Start from a raw `holdrand` value.
    pub fn new(state: u32) -> Self {
        Self { state, calls: 0 }
    }

    /// `srand(s)` (1A18:96BC) sets `holdrand = (u16)s`.
    pub fn srand(seed: u16) -> Self {
        Self::new(seed as u32)
    }

    /// One `rand()` call, 0..=0x7FFF.
    pub fn rand(&mut self) -> i32 {
        self.state = self.state.wrapping_mul(214013).wrapping_add(2531011);
        self.calls += 1;
        ((self.state >> 16) & 0x7FFF) as i32
    }

    /// START `random(n)` = `rand() % n` with an unsigned 16-bit divisor.
    ///
    /// `n == 0` would be a divide fault in the original; here it still consumes one `rand()`
    /// and returns 0 (never reached by the validated paths).
    pub fn r(&mut self, n: i32) -> i32 {
        let v = self.rand();
        let m = n & 0xFFFF;
        if m == 0 { 0 } else { v % m }
    }
}
