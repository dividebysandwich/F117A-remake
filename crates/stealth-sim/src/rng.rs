use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    pub hold: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self { hold: seed }
    }

    /// `rand15`: `hold = hold*0x343FD + 0x269EC3; (hold>>16) & 0x7FFF`.
    pub fn rand15(&mut self) -> i32 {
        self.hold = self.hold.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        ((self.hold >> 16) & 0x7FFF) as i32
    }

    /// `random(n)` = `(rand15() * n) >> 15` (32-bit product; n is a 16-bit word).
    pub fn random(&mut self, n: i32) -> i32 {
        let r = self.rand15();
        (r * (n as i16 as i32)) >> 15
    }
}
