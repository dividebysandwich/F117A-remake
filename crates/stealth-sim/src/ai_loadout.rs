//! What the campaign's AI fighters carry for their task, and which of it they fire at a target
//! (by its range and how far off the nose it is). The gun is always there: the missiles leave
//! the closest fights to it.

use serde::{Deserialize, Serialize};

/// Missile table indices of the friendly fighters' air-to-air missiles.
pub const AIM120: i32 = 22;
pub const AIM9: i32 = 23;
/// Missiles of a patrol (its era's radar missile).
const PATROL_MISSILES: i32 = 4;

/// What an AI fighter takes off for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FighterTask {
    /// Patrols around its goal and goes for the other side's aircraft.
    #[default]
    Patrol,
    /// Defends its airfield: engages only within a bubble around it, then lands.
    Cap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Seeker {
    #[default]
    Radar,
    Ir,
}

impl Seeker {
    /// Closest launch (m) and how far off the nose (rad) the target may be.
    fn envelope(self) -> (f64, f64) {
        match self {
            Seeker::Radar => (1500.0, 0.785),
            Seeker::Ir => (600.0, 0.52),
        }
    }
}

/// Missiles of one type (missile table index; none while the count is 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Store {
    pub missile: i32,
    pub count: i32,
    pub seeker: Seeker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AiLoadout {
    pub stores: [Store; 3],
}

impl AiLoadout {
    /// The load of aircraft spec `spec` for `task`; `era_missile` is its era's radar missile.
    pub fn for_task(spec: i32, era_missile: i32, task: FighterTask) -> Self {
        let store = |missile, count, seeker| Store { missile, count, seeker };
        let mut stores = [Store::default(); 3];
        match (task, spec) {
            // F-15C: four AMRAAMs on the fuselage, four Sidewinders on the wing pylons.
            (FighterTask::Cap, 21) => {
                stores[0] = store(AIM120, 4, Seeker::Radar);
                stores[1] = store(AIM9, 4, Seeker::Ir);
            }
            // F-16: AMRAAMs and Sidewinders on the wing stations.
            (FighterTask::Cap, _) => {
                stores[0] = store(AIM120, 4, Seeker::Radar);
                stores[1] = store(AIM9, 2, Seeker::Ir);
            }
            (FighterTask::Patrol, _) => {
                stores[0] = store(era_missile, PATROL_MISSILES, Seeker::Radar);
            }
        }
        AiLoadout { stores }
    }

    /// Missiles left.
    pub fn missiles(&self) -> i32 {
        self.stores.iter().map(|s| s.count.max(0)).sum()
    }

    /// The store to fire at a target `range` m away, `off_nose` rad off the nose, or None (out
    /// of every envelope, or nothing left). `reach` is a missile's lock range (m). Sidewinders
    /// go first when the target is well inside their reach; the radar missiles beyond it or
    /// once they are gone.
    pub fn choose(&self, range: f64, off_nose: f64, reach: impl Fn(i32) -> f64) -> Option<usize> {
        let fits = |k: usize| {
            let s = self.stores[k];
            let (min, cone) = s.seeker.envelope();
            s.count > 0 && range >= min && range <= reach(s.missile) && off_nose <= cone
        };
        let of = |seeker: Seeker, inside: f64| {
            (0..self.stores.len()).find(|&k| {
                let s = self.stores[k];
                fits(k) && s.seeker == seeker && range <= reach(s.missile) * inside
            })
        };
        of(Seeker::Ir, 0.8).or_else(|| of(Seeker::Radar, 1.0)).or_else(|| (0..3).find(|&k| fits(k)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reach(m: i32) -> f64 {
        match m {
            AIM120 => 32_000.0,
            AIM9 => 17_000.0,
            _ => 34_000.0,
        }
    }

    #[test]
    fn cap_loads() {
        let f15 = AiLoadout::for_task(21, 31, FighterTask::Cap);
        assert_eq!(f15.missiles(), 8);
        let f16 = AiLoadout::for_task(22, 31, FighterTask::Cap);
        assert_eq!(f16.missiles(), 6);
        assert!(f16.stores.iter().all(|s| s.count == 0 || matches!(s.missile, AIM120 | AIM9)));
        let patrol = AiLoadout::for_task(2, 21, FighterTask::Patrol);
        assert_eq!(patrol.stores[0], Store { missile: 21, count: 4, seeker: Seeker::Radar });
        assert_eq!(patrol.missiles(), 4);
    }

    #[test]
    fn weapon_by_range() {
        let l = AiLoadout::for_task(21, 31, FighterTask::Cap);
        let missile = |range, off| l.choose(range, off, reach).map(|k| l.stores[k].missile);
        // Far: AMRAAM; beyond it nothing.
        assert_eq!(missile(25_000.0, 0.1), Some(AIM120));
        assert_eq!(missile(40_000.0, 0.1), None);
        // Close: Sidewinder; inside its minimum range the gun's business.
        assert_eq!(missile(8_000.0, 0.1), Some(AIM9));
        assert_eq!(missile(400.0, 0.0), None);
        // Too far off the nose for the Sidewinder, not for the AMRAAM.
        assert_eq!(missile(8_000.0, 0.65), Some(AIM120));
        assert_eq!(missile(8_000.0, 1.0), None);
        // Between the AMRAAM's and the Sidewinder's minimum: Sidewinder only.
        assert_eq!(missile(1_000.0, 0.1), Some(AIM9));
    }

    #[test]
    fn falls_back_when_a_store_is_empty() {
        let mut l = AiLoadout::for_task(22, 31, FighterTask::Cap);
        l.stores[1].count = 0;
        assert_eq!(l.choose(8_000.0, 0.1, reach).map(|k| l.stores[k].missile), Some(AIM120));
        l.stores[0].count = 0;
        assert_eq!(l.choose(8_000.0, 0.1, reach), None);
        assert_eq!(l.missiles(), 0);
    }
}
