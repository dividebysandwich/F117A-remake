//! Flight recorder: the player's track with his radar exposure, the spans in which each site
//! tracked him, the enemy aircraft and missiles and the main events of the flight. It feeds the
//! debrief map and the Tacview export; it only reads the simulation.

use stealth_sim::{FlightUnit, Launcher, Projectile, Sim, SimEvent, oflags};

/// Sim frames per second.
pub const FS: u32 = crate::combat::FS as u32;
/// Aircraft and missiles are kept every this many frames (5 Hz).
const CONTACT_EVERY: u32 = 3;

/// The player in one sim frame.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub frame: u32,
    /// World units (x east, y north).
    pub wx: i32,
    pub wy: i32,
    /// True altitude (ft).
    pub alt: i32,
    /// 16-bit angles.
    pub heading: i32,
    pub pitch: i32,
    pub roll: i32,
    pub knots: i32,
    pub emv: i32,
    /// Best site score + EMV - 100: detected when > 0.
    pub margin: i32,
    /// The site with that score.
    pub site: Option<usize>,
}

impl Sample {
    /// Map position (x east, y south, map units).
    pub fn map(&self) -> (f32, f32) {
        (self.wx as f32 / 32.0, 0x8000 as f32 - self.wy as f32 / 32.0)
    }
}

/// Enemy aircraft and live missiles in one frame.
#[derive(Debug, Clone)]
pub struct Contacts {
    pub frame: u32,
    pub aircraft: Vec<(usize, FlightUnit)>,
    pub missiles: Vec<(usize, Projectile)>,
}

/// Something that happened, marked on the map where the player was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkKind {
    /// A site started tracking the player.
    Tracked(usize),
    /// A missile was fired at the player (by a site if known).
    Launch {
        site: Option<usize>,
        spec: i32,
    },
    Hit,
    AirKill {
        spec: i32,
    },
    GroundKill {
        object: usize,
    },
    /// A low-level air defence unit (`Sim::aaa_units` index) was destroyed.
    AaaKill(usize),
    Photo,
    Objective {
        primary: bool,
    },
    Ejected,
}

#[derive(Debug, Clone, Copy)]
pub struct Mark {
    pub frame: u32,
    /// Map units (x east, y south).
    pub x: f32,
    pub y: f32,
    pub kind: MarkKind,
}

/// A span of frames in which a site tracked the player (`end` None: until the end).
#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub site: usize,
    pub start: u32,
    pub end: Option<u32>,
}

impl Span {
    pub fn covers(&self, frame: u32) -> bool {
        frame >= self.start && self.end.is_none_or(|e| frame < e)
    }
}

/// A radar site as it was at takeoff.
#[derive(Debug, Clone)]
pub struct SiteInfo {
    /// Map units (x east, y south).
    pub x: f32,
    pub y: f32,
    /// Radar type ("SA-6").
    pub kind: String,
    /// Where it is ("Brest").
    pub place: String,
    /// Missile lock range (map units).
    pub lock: f32,
}

/// The whole flight.
#[derive(Debug, Clone, Default)]
pub struct FlightRecord {
    pub samples: Vec<Sample>,
    pub contacts: Vec<Contacts>,
    pub marks: Vec<Mark>,
    pub spans: Vec<Span>,
    /// The ground units by object index (empty names: not a radar).
    pub sites: Vec<SiteInfo>,
    /// The low-level air defences: what, where (map units) and whether friendly.
    pub aaa: Vec<(stealth_sim::aaa::AaaKind, i32, i32, bool)>,
    /// Open span per site.
    open: Vec<Option<usize>>,
    frame: u32,
}

/// Exposure totals for the debrief.
#[derive(Debug, Clone, Copy, Default)]
pub struct Exposure {
    /// Seconds in which at least one site tracked the player.
    pub tracked_s: f32,
    pub peak_margin: i32,
    /// The site that tracked the player longest, and for how many seconds.
    pub worst: Option<(usize, f32)>,
}

/// Best detection margin of all live sites against the player now, and its site.
pub fn margin(sim: &Sim) -> (i32, Option<usize>) {
    let emv = sim.emv();
    (3..sim.ground_unit_count())
        .filter(|&i| sim.objects()[i].unit_type > 0)
        .map(|i| (sim.site_score(i), i))
        .filter(|&(s, _)| s > 0)
        .max_by_key(|&(s, _)| s)
        .map_or((-100, None), |(s, i)| (s + emv - 100, Some(i)))
}

impl FlightRecord {
    pub fn seconds(frame: u32) -> f32 {
        frame as f32 / FS as f32
    }

    /// Frame of the last sample.
    pub fn last_frame(&self) -> u32 {
        self.samples.last().map_or(0, |s| s.frame)
    }

    /// The sample at or just before `frame`.
    pub fn at(&self, frame: u32) -> Option<&Sample> {
        let i = self.samples.partition_point(|s| s.frame <= frame);
        self.samples.get(i.saturating_sub(1))
    }

    fn here(&self) -> (f32, f32) {
        self.samples.last().map_or((0.0, 0.0), Sample::map)
    }

    fn mark(&mut self, kind: MarkKind) {
        let (x, y) = self.here();
        self.marks.push(Mark { frame: self.frame, x, y, kind });
    }

    /// Records the state after a sim step (call once per step, after its events).
    pub fn sample(&mut self, sim: &Sim) {
        if self.sites.is_empty() {
            self.sites = (0..sim.ground_unit_count())
                .map(|i| {
                    let o = sim.objects()[i];
                    let radar = i >= 3 && o.unit_type > 0;
                    SiteInfo {
                        x: o.x as f32,
                        y: o.y as f32,
                        kind: if radar {
                            sim.threat_name(o.unit_type).into()
                        } else {
                            String::new()
                        },
                        place: if radar { sim.place_name(i) } else { String::new() },
                        lock: sim.site_lock_range(o.unit_type) as f32 * 64.0,
                    }
                })
                .collect();
            self.aaa = sim
                .aaa_units()
                .iter()
                .enumerate()
                .map(|(k, u)| (u.kind, u.x, u.y, sim.aaa_blue(k)))
                .collect();
        }
        for (i, u) in sim.aaa_units().iter().enumerate() {
            if u.destroyed && !self.marks.iter().any(|m| m.kind == MarkKind::AaaKill(i)) {
                let (x, y) = (u.x as f32, u.y as f32);
                self.marks.push(Mark { frame: self.frame, x, y, kind: MarkKind::AaaKill(i) });
            }
        }
        let p = sim.player();
        let (margin, site) = margin(sim);
        self.samples.push(Sample {
            frame: self.frame,
            wx: p.wx,
            wy: p.wy,
            alt: p.alt,
            heading: p.heading,
            pitch: p.pitch,
            roll: p.roll,
            knots: p.knots,
            emv: sim.emv(),
            margin,
            site,
        });
        let n = sim.ground_unit_count();
        self.open.resize(n, None);
        for i in 0..n {
            let tracking = sim.objects()[i].flags & oflags::TRACKING != 0;
            match (tracking, self.open[i]) {
                (true, None) => {
                    self.open[i] = Some(self.spans.len());
                    self.spans.push(Span { site: i, start: self.frame, end: None });
                    self.mark(MarkKind::Tracked(i));
                }
                (false, Some(k)) => {
                    self.spans[k].end = Some(self.frame);
                    self.open[i] = None;
                }
                _ => {}
            }
        }
        if self.frame.is_multiple_of(CONTACT_EVERY) {
            self.contacts.push(Contacts {
                frame: self.frame,
                // As drawn: not the destroyed ones nor those not yet placed.
                aircraft: sim
                    .active_aircraft()
                    .into_iter()
                    .filter(|(_, u, _)| {
                        u.flags & stealth_sim::fflags::DESTROYED == 0 && (u.wx != 0 || u.wy != 0)
                    })
                    .map(|(i, u, _)| (i, u))
                    .collect(),
                missiles: sim.active_projectiles().into_iter().map(|(s, m, _, _)| (s, m)).collect(),
            });
        }
        self.frame += 1;
    }

    /// Notes an event of the step about to be sampled.
    pub fn event(&mut self, sim: &Sim, e: &SimEvent) {
        let kind = match *e {
            SimEvent::MissileLaunched { spec, launcher, .. } => match launcher {
                Launcher::Site(i) => MarkKind::Launch { site: Some(i), spec },
                Launcher::Aircraft(_) | Launcher::Manpads(_) => {
                    MarkKind::Launch { site: None, spec }
                }
                Launcher::Player => return,
            },
            SimEvent::PlayerHit { .. } => MarkKind::Hit,
            SimEvent::AircraftDestroyed { spec, .. } => MarkKind::AirKill { spec },
            SimEvent::GroundDestroyed { object, .. } => {
                // Where the object was, not where the player was.
                if let Some(o) = sim.objects().get(object) {
                    let kind = MarkKind::GroundKill { object };
                    self.marks.push(Mark { frame: self.frame, x: o.x as f32, y: o.y as f32, kind });
                }
                return;
            }
            SimEvent::PhotoTaken { .. } => MarkKind::Photo,
            SimEvent::ObjectiveDone { primary } => MarkKind::Objective { primary },
            SimEvent::Ejected { success: true } => MarkKind::Ejected,
            _ => return,
        };
        self.mark(kind);
    }

    pub fn exposure(&self) -> Exposure {
        let end = self.frame;
        let mut per_site: Vec<u32> = Vec::new();
        for s in &self.spans {
            if per_site.len() <= s.site {
                per_site.resize(s.site + 1, 0);
            }
            per_site[s.site] += s.end.unwrap_or(end) - s.start;
        }
        let tracked = (0..end).filter(|&f| self.spans.iter().any(|s| s.covers(f))).count();
        let worst = per_site
            .iter()
            .enumerate()
            .filter(|&(_, &n)| n > 0)
            .max_by_key(|&(_, &n)| n)
            .map(|(i, &n)| (i, Self::seconds(n)));
        Exposure {
            tracked_s: Self::seconds(tracked as u32),
            peak_margin: self.samples.iter().map(|s| s.margin).max().unwrap_or(-100),
            worst,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_and_exposure() {
        let mut r = FlightRecord::default();
        r.spans = vec![
            Span { site: 4, start: 0, end: Some(30) },
            Span { site: 5, start: 15, end: Some(45) },
            Span { site: 4, start: 60, end: None },
        ];
        r.frame = 90;
        let e = r.exposure();
        // Frames 0..45 and 60..90.
        assert_eq!(e.tracked_s, 75.0 / FS as f32);
        assert_eq!(e.worst.map(|w| w.0), Some(4));
        assert!(r.spans[2].covers(89) && !r.spans[0].covers(30));
    }
}
