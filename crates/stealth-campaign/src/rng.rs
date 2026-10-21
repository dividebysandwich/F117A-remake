use serde::{Deserialize, Serialize};

/// The campaign's own random stream (the sim's LCG), so a campaign replays the same from its
/// seed and the commands and reports it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    pub hold: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self { hold: seed }
    }

    pub fn rand15(&mut self) -> i32 {
        self.hold = self.hold.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        ((self.hold >> 16) & 0x7FFF) as i32
    }

    /// 0 .. n-1 (0 for n <= 0).
    pub fn below(&mut self, n: i32) -> i32 {
        if n <= 0 { 0 } else { (self.rand15() * n) >> 15 }
    }
}
