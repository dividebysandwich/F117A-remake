//! Realistic weapon damage (`SimOptions::realistic_damage`; in classic mode hits only
//! add up: damaged systems, a fuel leak, a weaker engine, but the aircraft never goes down).
//!
//! - A missile's warhead can destroy the airframe outright (more often against better
//!   opponents): the engine dies, the controls go and the aircraft breaks up within seconds,
//!   time enough to eject.
//! - Otherwise its fragments hit several systems at once and often start a fire. A fire
//!   burns for a while, then goes out or blows the aircraft up.
//! - Every hit to the flight controls bends a control surface: the aircraft is out of trim
//!   (a constant roll or pitch input) or a surface flaps (an oscillating one), adding up hit
//!   after hit.

use crate::Sim;
use crate::types::*;

/// Chance (%) that a missile hit destroys the airframe, by opponent skill.
const KILL_CHANCE: [i32; 4] = [20, 28, 36, 45];
/// Chance (%) that a missile hit that leaves the aircraft flying starts a fire.
const FIRE_CHANCE: i32 = 25;
/// Seconds a fire burns before it is decided, and the chance (%) it then blows the aircraft up
/// instead of going out.
const FIRE_SECS: (i32, i32) = (15, 60);
const EXPLODE_CHANCE: i32 = 30;
/// Seconds a destroyed airframe holds together.
const BREAKUP_SECS: (i32, i32) = (4, 9);
/// Largest control surface fault (thousandths of full stick).
const FAULT_MAX: i32 = 600;

impl Sim {
    /// A missile's warhead goes off next to the aircraft.
    pub(crate) fn missile_damage(&mut self) {
        if self.rng.random(100) < KILL_CHANCE[self.diff.clamp(0, 3) as usize] {
            self.airframe_destroyed();
            return;
        }
        let before = self.pl.damage;
        let mut bits = 0u8;
        for _ in 0..self.diff + 3 {
            bits |= 1u8 << self.rng.random(8).clamp(0, 7);
            self.pl.hits += 1;
        }
        if self.rng.random(100) < FIRE_CHANCE {
            bits |= dmg::FIRE;
        }
        self.pl.damage |= bits;
        self.new_damage(bits, before);
        self.emit(SimEvent::PlayerHit { bits });
        self.emit(SimEvent::DamageFlash);
        self.sound(4, 2);
    }

    /// What this hit's damage `bits` do beyond the classic effects (`before`: the damage
    /// bits before the hit).
    pub(crate) fn new_damage(&mut self, bits: u8, before: u8) {
        if bits & dmg::FIRE != 0 && before & dmg::FIRE == 0 && self.pl.breakup_frames == 0 {
            let (lo, hi) = FIRE_SECS;
            self.pl.fire_frames = (lo + self.rng.random(hi - lo)) * self.fs;
        }
        if bits & dmg::FC != 0 {
            self.bend_controls();
        }
    }

    /// A control surface is damaged: out of trim in roll or pitch, or flapping.
    fn bend_controls(&mut self) {
        let sign = if self.rng.random(2) == 0 { -1 } else { 1 };
        let kind = self.rng.random(4);
        let size = 60 + self.rng.random(140);
        let period = 6 + self.rng.random(14);
        let f = &mut self.pl.control_fault;
        match kind {
            0 | 1 => f.roll += sign * size,
            2 => f.pitch += sign * size * 2 / 3,
            _ => {
                if self.rng.random(2) == 0 {
                    f.roll_swing += size;
                } else {
                    f.pitch_swing += size * 2 / 3;
                }
                if f.period == 0 {
                    f.period = period;
                }
            }
        }
        for v in [&mut f.roll, &mut f.pitch, &mut f.roll_swing, &mut f.pitch_swing] {
            *v = (*v).clamp(-FAULT_MAX, FAULT_MAX);
        }
    }

    /// The warhead destroyed the airframe: the engine and controls are gone, it burns and
    /// breaks up within seconds.
    fn airframe_destroyed(&mut self) {
        let bits = dmg::ENG | dmg::FC | dmg::FIRE | dmg::AV;
        self.pl.damage |= bits;
        self.pl.hits += 8;
        self.pl.fire_frames = 0;
        let (lo, hi) = BREAKUP_SECS;
        self.pl.breakup_frames = (lo + self.rng.random(hi - lo)) * self.fs;
        let sign = if self.rng.random(2) == 0 { -1 } else { 1 };
        self.pl.control_fault = ControlFault {
            roll: sign * (300 + self.rng.random(300)),
            pitch: -150 - self.rng.random(250),
            roll_swing: 200,
            pitch_swing: 150,
            period: 5 + self.rng.random(6),
        };
        self.msg("Aircraft destroyed - Eject");
        self.emit(SimEvent::PlayerHit { bits });
        self.emit(SimEvent::DamageFlash);
        self.sound(4, 2);
        self.sound(5, 2);
    }

    /// Fires and a breaking airframe run their course (until the pilot has ejected).
    pub(crate) fn update_damage(&mut self) {
        if !self.opts.realistic_damage || self.pl.eject != 0 || self.ended.is_some() {
            return;
        }
        if self.pl.breakup_frames > 0 {
            self.pl.breakup_frames -= 1;
            if self.pl.breakup_frames == 0 {
                self.explode();
            }
            return;
        }
        if self.pl.fire_frames > 0 {
            self.pl.fire_frames -= 1;
            if self.pl.fire_frames == 0 {
                if self.rng.random(100) < EXPLODE_CHANCE {
                    self.explode();
                } else {
                    self.pl.damage &= !dmg::FIRE;
                    self.msg("Fire out");
                }
            }
        }
    }

    /// The aircraft blows up in the air.
    fn explode(&mut self) {
        let (x, y, alt) = (self.pl.p.mx, self.pl.p.my, self.pl.p.view_z);
        self.flash = Flash { x, y, alt, timer: 0x10 };
        self.emit(SimEvent::Explosion { x, y, alt, kind: 0x10 });
        self.msg("Aircraft exploded");
        self.sound(5, 2);
        self.pl.fire_frames = 0;
        self.pl.breakup_frames = 0;
        self.finalize(1);
    }
}
