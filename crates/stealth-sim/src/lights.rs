//! Aircraft exterior lights: position and anti-collision lights
//! are on around the aircraft's own airfields, where traffic has to see each other, and off
//! elsewhere (in combat they would give the aircraft away).

use crate::math::approx;
use crate::types::{GroundObject, oflags};
use crate::{Sim, fflags};

/// Distance from an airfield of the aircraft's own side within which its lights are on (map
/// units: 10 nm).
pub const HOME_LIGHTS_RANGE: i32 = 0x4A0;

/// Whether map point (x, y) is within `HOME_LIGHTS_RANGE` of an airfield or carrier of the
/// given side. Neutral (civil) airfields count for both sides.
pub fn near_home_airfield(objects: &[GroundObject], x: i32, y: i32, friendly: bool) -> bool {
    objects.iter().skip(3).any(|o| {
        o.flags & (oflags::AIRFIELD | oflags::CARRIER) != 0
            && (o.flags & oflags::NEUTRAL != 0 || (o.flags & oflags::FRIENDLY != 0) == friendly)
            && approx(o.x - x, o.y - y) < HOME_LIGHTS_RANGE
    })
}

impl Sim {
    /// Whether flight unit `i` shows its lights: near an airfield of its side, and not parked
    /// (an unmanned aircraft is dark) or destroyed.
    pub fn aircraft_lights(&self, i: usize) -> bool {
        let Some(u) = self.flights.get(i) else { return false };
        u.flags & fflags::EXISTS != 0
            && u.flags & fflags::DESTROYED == 0
            && u.speed != 0
            && near_home_airfield(&self.objects, u.x, u.y, self.friendly_unit(i, u.spec))
    }

    /// Whether the player's aircraft shows its lights (the rule of the friendly aircraft).
    pub fn player_lights(&self) -> bool {
        near_home_airfield(&self.objects, self.pl.p.mx, self.pl.p.my, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(x: i32, y: i32, flags: i32) -> GroundObject {
        GroundObject { x, y, flags, ..Default::default() }
    }

    #[test]
    fn lights_near_own_side_airfields() {
        let mut objects = vec![GroundObject::default(); 3];
        objects.push(base(0x1000, 0x1000, oflags::AIRFIELD | oflags::FRIENDLY));
        objects.push(base(0x3000, 0x1000, oflags::AIRFIELD));
        objects.push(base(0x5000, 0x1000, oflags::AIRFIELD | oflags::NEUTRAL));
        objects.push(base(0x7000, 0x1000, oflags::CARRIER | oflags::FRIENDLY));
        // Friendly base: friendly aircraft only, within range.
        assert!(near_home_airfield(&objects, 0x1000 + 0x400, 0x1000, true));
        assert!(!near_home_airfield(&objects, 0x1000 + 0x500, 0x1000, true));
        assert!(!near_home_airfield(&objects, 0x1000, 0x1000, false));
        // Enemy base: enemy aircraft only.
        assert!(near_home_airfield(&objects, 0x3000, 0x1100, false));
        assert!(!near_home_airfield(&objects, 0x3000, 0x1100, true));
        // Neutral airfield: both; friendly carrier: friendly aircraft.
        assert!(
            near_home_airfield(&objects, 0x5000, 0x1000, true)
                && near_home_airfield(&objects, 0x5000, 0x1000, false)
        );
        assert!(near_home_airfield(&objects, 0x7000, 0x1000, true));
        // The first three objects are not airfields.
        let first = vec![base(0x1000, 0x1000, oflags::AIRFIELD | oflags::FRIENDLY); 3];
        assert!(!near_home_airfield(&first, 0x1000, 0x1000, true));
    }
}
