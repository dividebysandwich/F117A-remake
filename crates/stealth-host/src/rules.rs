//! The settings that decide how the war is fought: the host's, for everybody in a session.

use serde::{Deserialize, Serialize};
use stealth_campaign::{Aircraft, CampaignRules};
use stealth_sim::{Airframe, LandingMode, SimOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rules {
    /// Missile hits can destroy the aircraft outright, fires, damaged control surfaces.
    pub realistic_damage: bool,
    /// Proportional navigation, burnt-out motors, lofted SAM shots.
    pub realistic_guidance: bool,
    /// Collisions bounce the aircraft off, crashes do not end the flight.
    pub no_crashes: bool,
    /// The sun and the moon move: moonlight sets how far enemy pilots see at night.
    pub continuous_time: bool,
    /// The F-117A as it was: no cannon, only two bays.
    pub realistic_weapons: bool,
    /// The players' missiles and gunfire damage each other.
    #[serde(default)]
    pub friendly_fire: bool,
}

impl From<CampaignRules> for Rules {
    fn from(r: CampaignRules) -> Self {
        Self {
            realistic_damage: r.realistic_damage,
            realistic_guidance: r.realistic_guidance,
            no_crashes: r.no_crashes,
            continuous_time: r.continuous_time,
            realistic_weapons: r.realistic_weapons,
            friendly_fire: r.friendly_fire,
        }
    }
}

impl From<Rules> for CampaignRules {
    fn from(r: Rules) -> Self {
        Self {
            realistic_damage: r.realistic_damage,
            realistic_guidance: r.realistic_guidance,
            no_crashes: r.no_crashes,
            continuous_time: r.continuous_time,
            realistic_weapons: r.realistic_weapons,
            friendly_fire: r.friendly_fire,
        }
    }
}

impl Rules {
    /// The simulation's options for a campaign under these rules.
    pub fn sim_options(&self) -> SimOptions {
        SimOptions {
            landing: if self.no_crashes { LandingMode::NoCrashes } else { LandingMode::Easy },
            realistic_damage: self.realistic_damage,
            realistic_guidance: self.realistic_guidance,
            friendly_fire: self.friendly_fire,
            air_defence: true,
            campaign: true,
            ..Default::default()
        }
    }

    /// An aircraft's simulation traits under these rules (the real F-117A without a cannon).
    pub fn airframe(&self, aircraft: Aircraft) -> Airframe {
        match aircraft {
            Aircraft::F117 => Airframe { lockheed: self.realistic_weapons, ..Airframe::default() },
            Aircraft::F16 => Airframe { external_stores: true, ..Airframe::default() },
            Aircraft::UH60 => {
                Airframe { external_stores: true, helicopter: true, ..Airframe::default() }
            }
            Aircraft::AH1Z => Airframe {
                external_stores: true,
                helicopter: true,
                gunship: true,
                ..Airframe::default()
            },
        }
    }
}
