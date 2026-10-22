//! Weather: clouds block optical lines of sight. Radar sees
//! through them. With a clear sky (classic) every check passes.

use crate::Sim;

/// viewZ (compressed altitude, `math::view_z`) back to feet.
pub fn view_z_feet(v: i32) -> i32 {
    if v < 0x2000 {
        v
    } else if v < 0x3000 {
        0x2000 + (v - 0x2000) * 2
    } else {
        0x4000 + (v - 0x3000) * 4
    }
}

/// Seekers that need to see their target: IR, TV/IR AGM, laser-guided bomb, AIM-9.
pub fn optical_seeker(spec: i32, cls: i32) -> bool {
    cls <= 0 || cls == 6 || cls == 28 || spec == 23
}

impl Sim {
    /// The weather changes (the dynamic campaign's).
    pub fn set_weather(&mut self, weather: stealth_mission::Weather) {
        self.weather = weather;
    }

    /// Clear line of sight between two points (map units, viewZ).
    pub(crate) fn sees(&self, a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
        !self.weather.any()
            || self.weather.los_clear((a.0, a.1, view_z_feet(a.2)), (b.0, b.1, view_z_feet(b.2)))
    }

    /// Clear line of sight from the player.
    pub(crate) fn player_sees(&self, x: i32, y: i32, alt: i32) -> bool {
        self.sees((self.pl.p.mx, self.pl.p.my, self.pl.p.view_z), (x, y, alt))
    }

    /// An optical-seeker missile is only launched at a player its launcher can see (and none at
    /// one who ejected).
    pub(crate) fn may_launch(&self, spec: i32, from: (i32, i32, i32)) -> bool {
        let cls = self.missile(spec).class as i32;
        self.pl.eject == 0
            && (!optical_seeker(spec, cls)
                || self.sees(from, (self.pl.p.mx, self.pl.p.my, self.pl.p.view_z)))
    }

    /// Storms halve visual ranges.
    pub(crate) fn visual_shift(&self) -> i32 {
        self.weather.has_storms() as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::view_z;

    #[test]
    fn view_z_round_trip() {
        for ft in [0u16, 500, 0x1FFF, 0x2000, 0x2F00, 0x3FFE, 0x4000, 0x8000, 0xC000] {
            let back = view_z_feet(view_z(ft));
            assert!((back - ft as i32).abs() <= 3, "{ft} -> {back}");
        }
    }
}
