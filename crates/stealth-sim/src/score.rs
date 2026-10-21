//! Mission debrief: statistics from the event log, points and a rating (the remake's own scoring).

use serde::{Deserialize, Serialize};

use crate::Sim;
use crate::types::MissionEnd;

/// How the flight ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Landed,
    BadLanding,
    /// Ejected and came down by parachute.
    Ejected,
    /// Ejected over enemy territory and picked up by the rescue helicopter.
    Rescued,
    /// Ejected over enemy territory and taken prisoner.
    Captured,
    Crashed,
    EjectionFailed,
    Aborted,
}

impl Outcome {
    pub fn from_end(e: Option<MissionEnd>) -> Self {
        match e.map(|e| (e.outcome, e.landing_type)) {
            Some((0, 2)) => Self::Ejected,
            Some((0, 4)) => Self::Rescued,
            Some((8, _)) => Self::Captured,
            Some((0, _)) => Self::Landed,
            Some((1 | 2, _)) => Self::Crashed,
            Some((5, _)) => Self::BadLanding,
            Some((6, _)) => Self::EjectionFailed,
            _ => Self::Aborted,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Self::Landed => "Safe landing",
            Self::BadLanding => "Bad landing",
            Self::Ejected => "Ejected - landed by parachute",
            Self::Rescued => "Ejected - rescued by CSAR",
            Self::Captured => "Ejected - captured",
            Self::Crashed => "Crashed",
            Self::EjectionFailed => "Ejection failed",
            Self::Aborted => "Mission aborted",
        }
    }

    /// The pilot did not survive.
    pub fn fatal(self) -> bool {
        matches!(self, Self::Crashed | Self::EjectionFailed)
    }
}

/// One line of the mission timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// Mission clock (seconds).
    pub tick: i32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionReport {
    pub outcome: Outcome,
    pub training: bool,
    pub primary_done: bool,
    pub secondary_done: bool,
    /// Aircraft shot down (not counting drones).
    pub air_kills: i32,
    pub ground_kills: i32,
    /// Aircraft shot down before they had seen the player.
    pub unseen_kills: i32,
    pub weapons_fired: i32,
    pub hits_taken: i32,
    /// Attack drones shot down / that reached their target (drone defense: a failure).
    #[serde(default)]
    pub drones_killed: i32,
    #[serde(default)]
    pub drones_leaked: i32,
    #[serde(default)]
    pub nuclear_hit: bool,
    pub radar_ids: i32,
    pub visual_ids: i32,
    /// Mission clock at the end (seconds).
    pub time: i32,
    /// Points before the difficulty multiplier, by reason.
    pub points: Vec<(String, i32)>,
    /// Difficulty multiplier.
    pub multiplier: f32,
    pub total: i32,
    pub rating: String,
    pub timeline: Vec<TimelineEntry>,
}

/// Rating for a mission total.
pub fn rating(total: i32) -> &'static str {
    match total {
        i32::MIN..=0 => "Unsatisfactory",
        1..=49 => "Marginal",
        50..=149 => "Satisfactory",
        150..=299 => "Excellent",
        _ => "Outstanding",
    }
}

/// Difficulty multiplier: opponents 0..3, situation 0..2, training halves.
pub fn multiplier(diff: i32, roe: i32, training: bool) -> f32 {
    let m = (1.0 + diff.clamp(0, 3) as f32 * 0.5) * (1.0 + roe.clamp(0, 2) as f32 * 0.25);
    if training { m * 0.5 } else { m }
}

impl Sim {
    /// Debrief for the mission so far (complete once `mission_end` is set or the mission was quit).
    pub fn mission_report(&self) -> MissionReport {
        let o = self.objectives();
        let outcome = Outcome::from_end(self.mission_end());
        let training = self.invulnerable();
        let (mut air, mut ground, mut unseen, mut fired, mut hits, mut radar_ids, mut visual_ids) =
            (0, 0, 0, 0, 0, 0, 0);
        let mut drones_killed = 0;
        let mut timeline = Vec::new();
        for e in self.event_log() {
            let tag = match e.kind & 0xC0 {
                0x80 => " (primary)",
                0x40 => " (secondary)",
                _ => "",
            };
            let text = match e.kind & 0x3F {
                1 | 12 => {
                    ground += 1;
                    Some(format!("Destroyed {}{tag}", self.place_name(e.arg as usize)))
                }
                2 => {
                    ground += 1;
                    Some(format!("Destroyed ground target{tag}"))
                }
                3 => {
                    let spec = e.arg as i32 & 0x7F;
                    if self.ac_type(spec).kind == crate::AircraftKind::Drone {
                        drones_killed += 1;
                    } else {
                        air += 1;
                        if e.arg & 0x80 == 0 {
                            unseen += 1;
                        }
                    }
                    Some(format!("Shot down {}{tag}", self.aircraft_name(spec)))
                }
                4 => {
                    fired += 1;
                    None
                }
                5 => {
                    hits += 1;
                    Some("Hit by a missile".to_string())
                }
                6 => {
                    visual_ids += 1;
                    Some(format!("Seen by a {}", self.aircraft_name(e.arg as i32)))
                }
                7 => {
                    radar_ids += 1;
                    Some(format!("Radar ID by {}", self.place_name(e.arg as usize)))
                }
                0x0B => Some(format!("Objective reached{tag}")),
                0x0D => Some(format!("{} got through", self.aircraft_name(e.arg as i32))),
                0x0A => Some(format!("Photo taken{tag}")),
                _ => None,
            };
            if let Some(text) = text {
                timeline.push(TimelineEntry { tick: e.tick, text });
            }
        }
        let drones = self.drones();
        let defend = drones.is_some_and(|d| d.defend);
        let (drones_leaked, nuclear_hit) = drones.map_or((0, false), |d| (d.leaked, d.nuclear_hit));
        let mut points = Vec::new();
        let mut add = |what: &str, p: i32| {
            if p != 0 {
                points.push((what.to_string(), p));
            }
        };
        if outcome != Outcome::Aborted {
            add("Primary objective", if o.primary_done { 100 } else { 0 });
            add("Secondary objective", if o.secondary_done { 50 } else { 0 });
            add("Aircraft destroyed", 10 * air);
            add("Shot down unseen", 5 * unseen);
            add("Ground targets destroyed", 10 * ground);
            add("Drones shot down", 15 * drones_killed);
            if defend {
                add("Drones through", -30 * drones_leaked);
                add("Nuclear plant hit", if nuclear_hit { -200 } else { 0 });
            }
            add("Detected", -5 * (radar_ids + visual_ids));
            add("Missile hits", -10 * hits);
            add(
                outcome.text(),
                match outcome {
                    Outcome::Landed => 25,
                    Outcome::BadLanding => 5,
                    Outcome::Ejected | Outcome::Rescued => 0,
                    Outcome::Captured => -100,
                    Outcome::Crashed | Outcome::EjectionFailed => -50,
                    Outcome::Aborted => 0,
                },
            );
        }
        let multiplier = multiplier(self.diff, self.roe, training);
        let raw: i32 = points.iter().map(|p| p.1).sum();
        let total = (raw as f32 * multiplier).round() as i32;
        MissionReport {
            outcome,
            training,
            primary_done: o.primary_done,
            secondary_done: o.secondary_done,
            air_kills: air,
            ground_kills: ground,
            unseen_kills: unseen,
            weapons_fired: fired,
            hits_taken: hits,
            drones_killed,
            drones_leaked,
            nuclear_hit,
            radar_ids,
            visual_ids,
            time: self.mission_tick(),
            points,
            multiplier,
            total,
            rating: rating(total).to_string(),
            timeline,
        }
    }

    pub fn aircraft_name(&self, spec: i32) -> String {
        self.aircraft_types
            .get(spec.max(0) as usize)
            .map_or_else(|| "aircraft".into(), |t| t.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratings_rise_with_points() {
        assert_eq!(rating(-10), "Unsatisfactory");
        assert_eq!(rating(10), "Marginal");
        assert_eq!(rating(100), "Satisfactory");
        assert_eq!(rating(200), "Excellent");
        assert_eq!(rating(500), "Outstanding");
    }

    #[test]
    fn harder_missions_score_more() {
        assert_eq!(multiplier(0, 0, false), 1.0);
        assert_eq!(multiplier(3, 2, false), 2.5 * 1.5);
        assert_eq!(multiplier(1, 1, true), 1.5 * 1.25 * 0.5);
        assert!(multiplier(2, 1, false) > multiplier(1, 1, false));
    }

    #[test]
    fn outcomes_from_mission_end() {
        let end = |outcome, landing_type| {
            Some(MissionEnd { outcome, landing_type, map_x: 0, map_y: 0, damage: 0, hits: 0 })
        };
        assert_eq!(Outcome::from_end(end(0, 3)), Outcome::Landed);
        assert_eq!(Outcome::from_end(end(0, 2)), Outcome::Ejected);
        assert_eq!(Outcome::from_end(end(1, 1)), Outcome::Crashed);
        assert_eq!(Outcome::from_end(end(6, 2)), Outcome::EjectionFailed);
        assert_eq!(Outcome::from_end(end(7, 1)), Outcome::Aborted);
        assert_eq!(Outcome::from_end(None), Outcome::Aborted);
    }
}
