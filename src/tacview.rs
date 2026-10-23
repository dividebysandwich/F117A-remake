//! Tacview export (`--tacview`, the menu's "Tacview export"): the recorded flight as an ACMI 2.2
//! text file in `<data dir>/stealth-mission-89/tacview`. The theater is laid on the globe at
//! game scale around one real airbase ([`crate::world::geo_origin`]). The same text goes to
//! Tacview while the flight is flown with the real-time telemetry (tacview_real_time.rs).

use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;

use bevy::log::{info, warn};
use stealth_mission::Mission;
use stealth_mission::world::flags as F;
use stealth_sim::SimData;

use crate::recorder::{FS, FlightRecord, MarkKind};
use crate::world::WU;

const FT: f64 = 0.3048;
const PLAYER: u32 = 0x100;
const AIRCRAFT: u32 = 0x1000;
const MISSILES: u32 = 0x2000;
const SITES: u32 = 0x8000;
const AIRFIELDS: u32 = 0x9000;
const AAA: u32 = 0xA000;

/// Degrees of a 16-bit angle (signed for pitch and roll).
fn deg(bam: i32, signed: bool) -> f64 {
    let v = if signed { bam as i16 as f64 } else { bam as u16 as f64 };
    v / 65536.0 * 360.0
}

/// The altitude (ft) of a viewZ (`stealth_sim::math::view_z` inverted).
fn true_altitude(z: f64) -> f64 {
    if z < 8192.0 {
        z
    } else if z < 12288.0 {
        8192.0 + (z - 8192.0) * 2.0
    } else {
        16384.0 + (z - 12288.0) * 4.0
    }
}

/// The theater on the globe: offsets (degrees) from its south-west corner.
struct Geo {
    lat0: f64,
}

impl Geo {
    /// Longitude and latitude offsets of a point (metres east and north of the corner).
    fn offset(&self, east: f64, north: f64) -> (f64, f64) {
        let lat = north / 111_200.0;
        let lon = east / (111_320.0 * (self.lat0 + lat).to_radians().cos());
        (lon, lat)
    }

    /// `T=` value of a point in map units (x east, y south) at an altitude (ft).
    fn map_t(&self, x: f64, y: f64, alt_ft: f64) -> String {
        let (lon, lat) = self.offset(x * 32.0 * WU, (0x8000 as f64 - y) * 32.0 * WU);
        format!("{lon:.6}|{lat:.6}|{:.1}", alt_ft * FT)
    }
}

/// Days since 1970-01-01 of a civil date, and back (Howard Hinnant's algorithms).
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// The briefing date ("June 5, 1991"), else 1 January 1990.
pub(crate) fn briefing_date(text: &str) -> (i64, i64, i64) {
    parse_briefing_date(text).unwrap_or((1990, 1, 1))
}

/// The briefing's date (year, month, day), if it has one.
pub(crate) fn parse_briefing_date(text: &str) -> Option<(i64, i64, i64)> {
    const MONTHS: [&str; 12] =
        ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let lower = text.to_ascii_lowercase();
    let words: Vec<&str> = lower.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    let first = words.first()?;
    let m = MONTHS.iter().position(|m| first.starts_with(m))?;
    Some((words.get(2)?.parse().ok()?, m as i64 + 1, words.get(1)?.parse().ok()?))
}

/// ISO 8601 UTC time of the mission start: the briefing date and the local mission clock,
/// shifted by the longitude (local solar time).
fn reference_time(m: &Mission, lon: f64) -> String {
    let (y, mo, d) = briefing_date(&m.briefing.date);
    let local = m.time.base_hour as f64 * 3600.0 + m.time.tod_ticks as f64 * 2.0;
    let utc = (days_from_civil(y, mo, d) as f64 * 86_400.0 + local - lon / 15.0 * 3600.0) as i64;
    let (y, mo, d) = civil_from_days(utc.div_euclid(86_400));
    let s = utc.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

fn escape(s: &str) -> String {
    s.replace(',', "\\,")
}

/// The ACMI text of a recorded flight.
pub fn acmi(rec: &FlightRecord, m: &Mission, data: &SimData, aircraft: &str) -> String {
    let mut out = String::new();
    let mut writer = AcmiWriter::new(m);
    writer.advance(rec, m, data, aircraft, &mut out);
    writer.start(rec, m, &mut out);
    out
}

/// Writes a flight record as ACMI text while it grows: each call adds what was recorded since
/// the previous one (the whole flight for the export, the last frames for the real-time telemetry).
pub struct AcmiWriter {
    geo: Geo,
    lon0: f64,
    started: bool,
    /// Samples, contacts and marks already written.
    samples: usize,
    contacts: usize,
    marks: usize,
    planes: HashMap<usize, ()>,
    /// Missile slot -> (object id, spec, launcher).
    missiles: HashMap<usize, (u32, i32, i32)>,
    next_missile: u32,
}

impl AcmiWriter {
    pub fn new(m: &Mission) -> Self {
        let (lon0, lat0) = crate::world::geo_origin(&m.theater_code.to_ascii_lowercase());
        Self {
            geo: Geo { lat0 },
            lon0,
            started: false,
            samples: 0,
            contacts: 0,
            marks: 0,
            planes: HashMap::new(),
            missiles: HashMap::new(),
            next_missile: MISSILES,
        }
    }

    /// The header and the objects of the whole flight (the record has them from its first
    /// sample on), once.
    fn start(&mut self, rec: &FlightRecord, m: &Mission, w: &mut String) {
        if self.started {
            return;
        }
        self.started = true;
        let (lon0, lat0, geo) = (self.lon0, self.geo.lat0, &self.geo);
        let _ = writeln!(w, "FileType=text/acmi/tacview\nFileVersion=2.2");
        let _ = writeln!(w, "0,ReferenceTime={}", reference_time(m, lon0));
        let _ = writeln!(w, "0,ReferenceLongitude={lon0}\n0,ReferenceLatitude={lat0}");
        let _ = writeln!(w, "0,DataSource=Stealth Mission 89");
        let _ = writeln!(
            w,
            "0,Title={}",
            escape(&crate::mission::clean_text(&format!(
                "{} - {}",
                m.theater_name, m.briefing.title
            )))
        );
        let _ = writeln!(w, "#0");
        // The radar sites and the airfields, for the whole flight.
        for (i, s) in rec.sites.iter().enumerate().filter(|(_, s)| !s.kind.is_empty()) {
            let _ = writeln!(
                w,
                "{:x},T={},Type=Ground+Static+AntiAircraft,Name={},CallSign={},Coalition=Enemies,Color=Red",
                SITES + i as u32,
                geo.map_t(s.x as f64, s.y as f64, 0.0),
                escape(&s.kind),
                escape(&crate::mission::clean_text(&s.place)),
            );
        }
        for (i, o) in m.world.objects.iter().enumerate().filter(|(_, o)| o.flags & F::AIRFIELD != 0)
        {
            let name = m.world.names.get(o.name as usize).map_or("", String::as_str);
            let (coalition, colour) =
                if o.flags & F::FRIENDLY != 0 { ("Allies", "Blue") } else { ("Enemies", "Red") };
            let _ = writeln!(
                w,
                "{:x},T={},Type=Ground+Static+Aerodrome,Name={},Coalition={coalition},Color={colour}",
                AIRFIELDS + i as u32,
                geo.map_t(o.x as u16 as f64, o.y as u16 as f64, 0.0),
                escape(&crate::mission::clean_text(name)),
            );
        }

        for (i, &(kind, x, y, blue)) in rec.aaa.iter().enumerate() {
            let ty = if kind == stealth_sim::aaa::AaaKind::Manpads {
                "Ground+Light+Human+Infantry"
            } else {
                "Ground+Heavy+Armor+AntiAircraft"
            };
            let (coalition, colour) = if blue { ("Allies", "Blue") } else { ("Enemies", "Red") };
            let _ = writeln!(
                w,
                "{:x},T={},Type={ty},Name={},Coalition={coalition},Color={colour}",
                AAA + i as u32,
                geo.map_t(x as f64, y as f64, 0.0),
                kind.name_on(blue),
            );
        }
    }

    /// Whether `rec` started over since the last call (a new campaign sortie in the same flight).
    pub fn restarted(&self, rec: &FlightRecord) -> bool {
        rec.samples.len() < self.samples
    }

    /// Appends to `w` the frames recorded since the last call (nothing before the first sample).
    pub fn advance(
        &mut self,
        rec: &FlightRecord,
        m: &Mission,
        data: &SimData,
        aircraft: &str,
        w: &mut String,
    ) {
        if rec.samples.len() <= self.samples {
            return;
        }
        self.start(rec, m, w);
        let geo = &self.geo;
        for s in &rec.samples[self.samples..] {
            let _ = writeln!(w, "#{:.2}", s.frame as f64 / FS as f64);
            let (lon, lat) = geo.offset(s.wx as f64 * WU, s.wy as f64 * WU);
            let _ = write!(
                w,
                "{PLAYER:x},T={lon:.6}|{lat:.6}|{:.1}|{:.1}|{:.1}|{:.1}",
                s.alt as f64 * FT,
                deg(s.roll, true),
                deg(s.pitch, true),
                deg(s.heading, false),
            );
            if self.samples == 0 {
                let rotorcraft = crate::aircraft::AircraftKind::ALL
                    .iter()
                    .any(|k| k.name() == aircraft && k.spec().helicopter);
                let _ = write!(
                    w,
                    ",Type=Air+{},Name={},Pilot=Player,Coalition=Allies,Color=Blue",
                    if rotorcraft { "Rotorcraft" } else { "FixedWing" },
                    escape(aircraft)
                );
            }
            self.samples += 1;
            let _ = writeln!(w, ",IAS={:.1}", s.knots as f64 * 0.514_444);

            if let Some(c) = rec.contacts.get(self.contacts).filter(|c| c.frame == s.frame) {
                self.contacts += 1;
                let (planes, missiles) = (&mut self.planes, &mut self.missiles);
                let now: Vec<usize> = c.aircraft.iter().map(|(i, _)| *i).collect();
                for gone in planes.keys().copied().filter(|i| !now.contains(i)).collect::<Vec<_>>()
                {
                    planes.remove(&gone);
                    let _ = writeln!(w, "-{:x}", AIRCRAFT + gone as u32);
                }
                for (i, u) in &c.aircraft {
                    let alt = true_altitude(u.alt as f64);
                    let (lon, lat) = geo.offset(u.wx as f64 * WU, (0x100000 - u.wy) as f64 * WU);
                    let _ = write!(
                        w,
                        "{:x},T={lon:.6}|{lat:.6}|{:.1}|{:.1}|{:.1}|{:.1}",
                        AIRCRAFT + *i as u32,
                        alt * FT,
                        deg(u.bank, true),
                        deg(u.pitch, true),
                        deg(u.heading, false),
                    );
                    if planes.insert(*i, ()).is_none() {
                        let t = data.aircraft.get(u.spec.max(0) as usize);
                        let friendly = t.is_some_and(|t| t.radar == -1);
                        let (coalition, colour) =
                            if friendly { ("Allies", "Blue") } else { ("Enemies", "Red") };
                        let _ = write!(
                            w,
                            ",Type=Air+FixedWing,Name={},Coalition={coalition},Color={colour}",
                            escape(t.map_or("Aircraft", |t| t.name.as_str()))
                        );
                    }
                    let _ = writeln!(w);
                }
                let slots: Vec<usize> = c.missiles.iter().map(|(s, _)| *s).collect();
                for (slot, (id, ..)) in missiles.clone() {
                    if !slots.contains(&slot) {
                        missiles.remove(&slot);
                        let _ = writeln!(w, "-{id:x}");
                    }
                }
                for (slot, p) in &c.missiles {
                    let (x, y, z) = p.map_pos();
                    let alt = if p.flight.is_some() { z } else { true_altitude(z) };
                    let fresh = match missiles.get(slot) {
                        Some(&(id, spec, launcher)) if spec == p.spec && launcher == p.launcher => {
                            let _ = write!(w, "{id:x}");
                            false
                        }
                        old => {
                            if let Some(&(id, ..)) = old {
                                let _ = writeln!(w, "-{id:x}");
                            }
                            let id = self.next_missile;
                            self.next_missile += 1;
                            missiles.insert(*slot, (id, p.spec, p.launcher));
                            let _ = write!(w, "{id:x}");
                            true
                        }
                    };
                    let _ = write!(
                        w,
                        ",T={}|0|{:.1}|{:.1}",
                        geo.map_t(x, y, alt),
                        deg(p.pitch, true),
                        deg(p.heading, false),
                    );
                    if fresh {
                        let player = *slot >= 8;
                        let (coalition, colour) =
                            if player { ("Allies", "Blue") } else { ("Enemies", "Red") };
                        let name = data
                            .missiles
                            .get(p.spec.max(0) as usize)
                            .map_or("Missile", |t| t.name.as_str());
                        let _ = write!(
                            w,
                            ",Type=Weapon+Missile,Name={},Coalition={coalition},Color={colour}",
                            escape(name)
                        );
                    }
                    let _ = writeln!(w);
                }
            }

            while let Some(mk) = rec.marks.get(self.marks).filter(|mk| mk.frame <= s.frame) {
                self.marks += 1;
                match mk.kind {
                    MarkKind::GroundKill { object }
                        if rec.sites.get(object).is_some_and(|s| !s.kind.is_empty()) =>
                    {
                        let id = SITES + object as u32;
                        let _ = writeln!(w, "0,Event=Destroyed|{id:x}|\n-{id:x}");
                    }
                    MarkKind::Hit => {
                        let _ = writeln!(w, "0,Event=Message|{PLAYER:x}|Hit");
                    }
                    MarkKind::AaaKill(i) => {
                        let id = AAA + i as u32;
                        let _ = writeln!(w, "0,Event=Destroyed|{id:x}|\n-{id:x}");
                    }
                    MarkKind::Tracked(i) => {
                        let _ = writeln!(w, "0,Event=Message|{:x}|Tracking", SITES + i as u32);
                    }
                    MarkKind::Ejected => {
                        let _ = writeln!(w, "0,Event=Message|{PLAYER:x}|Ejected");
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Where recordings go.
fn folder() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("stealth-mission-89").join("tacview"))
}

/// Writes the recording of a flight; errors are logged.
pub fn export(rec: &FlightRecord, m: &Mission, data: &SimData, aircraft: &str) -> Option<PathBuf> {
    let dir = folder()?;
    let now = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, mo, d) = civil_from_days(now.div_euclid(86_400));
    let t = now.rem_euclid(86_400);
    let name = format!(
        "{y:04}{mo:02}{d:02}-{:02}{:02}{:02}_{}.acmi",
        t / 3600,
        t / 60 % 60,
        t % 60,
        m.theater_code.to_ascii_lowercase()
    );
    let path = dir.join(name);
    let text = acmi(rec, m, data, aircraft);
    match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, text)) {
        Ok(()) => {
            info!("tacview: {}", path.display());
            Some(path)
        }
        Err(e) => {
            warn!("tacview: {}: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        for (y, m, d) in [(1970, 1, 1), (1991, 1, 17), (2000, 2, 29), (2026, 10, 21)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(briefing_date("January 17, 1991"), (1991, 1, 17));
        assert_eq!(briefing_date("Nov 3, 1989"), (1989, 11, 3));
        assert_eq!(briefing_date(""), (1990, 1, 1));
    }

    #[test]
    fn geo_offsets() {
        let g = Geo { lat0: 60.0 };
        // 111.2 km north is a degree; at 60 degrees a degree east is half as long.
        let (lon, lat) = g.offset(55_660.0, 111_200.0);
        assert!((lat - 1.0).abs() < 1e-9);
        assert!((lon - 55_660.0 / (111_320.0 * 61f64.to_radians().cos())).abs() < 1e-9);
        assert_eq!(deg(0x4000, false), 90.0);
        assert_eq!(deg(0xC000, true), -90.0);
        assert_eq!(true_altitude(13192.0), 20000.0);
    }

    #[test]
    fn acmi_objects_spawn_and_go() {
        use crate::recorder::{Contacts, Sample};
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let data = SimData::load_json(&assets.join("sim")).unwrap();
        let gdata = stealth_mission::GameData::load_json(&assets.join("mission")).unwrap();
        let m = stealth_mission::generate(
            &gdata,
            &stealth_mission::MissionSettings { theater: 3, ..Default::default() },
            stealth_mission::Seed::Srand(1234),
        )
        .unwrap();
        let mut rec = FlightRecord::default();
        let missile =
            stealth_sim::Projectile { ttl: 10, spec: 2, launcher: 5, ..Default::default() };
        for f in 0..6 {
            rec.samples.push(Sample {
                frame: f,
                wx: 0x80000,
                wy: 0x80000,
                alt: 1000,
                heading: 0x4000,
                pitch: 0,
                roll: 0,
                knots: 400,
                emv: 0,
                margin: -100,
                site: None,
            });
        }
        rec.contacts = vec![
            Contacts { frame: 0, aircraft: vec![], missiles: vec![(1, missile)] },
            Contacts { frame: 3, aircraft: vec![], missiles: vec![] },
        ];
        rec.marks.push(crate::recorder::Mark { frame: 2, x: 0.0, y: 0.0, kind: MarkKind::Hit });
        let text = acmi(&rec, &m, &data, "F-117A");
        assert!(text.contains("\n0,Event=Message|100|Hit\n"));
        // Written while it grows, frame by frame (the real-time telemetry), the flight reads the
        // same.
        let mut writer = AcmiWriter::new(&m);
        let mut streamed = String::new();
        for n in 0..=rec.samples.len() {
            let mut part = rec.clone();
            part.samples.truncate(n);
            part.contacts.retain(|c| (c.frame as usize) < n);
            part.marks.retain(|mk| (mk.frame as usize) < n);
            writer.advance(&part, &m, &data, "F-117A", &mut streamed);
        }
        assert_eq!(streamed, text);
        assert!(text.starts_with("FileType=text/acmi/tacview\nFileVersion=2.2\n"));
        assert!(text.contains("0,ReferenceTime="));
        assert!(text.contains(",Type=Air+FixedWing,Name=F-117A,"));
        // The player flies east at 1000 ft (304.8 m), heading 90.
        assert!(text.contains("|304.8|0.0|0.0|90.0"));
        assert_eq!(text.matches("2000,T=").count(), 1);
        assert!(text.contains("2000,T=") && text.contains("Type=Weapon+Missile"));
        assert!(text.contains("\n-2000\n"));
    }
}
