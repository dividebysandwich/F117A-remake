use serde::{Deserialize, Serialize};

use crate::generator::{GenResult, s16};
use crate::tables::StartTables;

pub const KIND_TITLES: [&str; 4] =
    ["Air Combat Mission Orders", "Ground Strike Mission Orders", "Air Training Orders", "Ground Strike Training Orders"];

/// The briefing as START prints it (paragraph layout is not reproduced). Strings are the raw
/// START texts decoded as latin-1, so the paragraphs keep START's in-text control bytes
/// (e.g. 0x8C / 0x80 around highlighted words, trailing CR).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Briefing {
    pub header: String,
    pub title: String,
    pub pilot_line: String,
    /// `"<Mon d, yyyy>"`.
    pub date: String,
    /// `"<takeoff base> - <date>"`.
    pub base_date_line: String,
    /// PRIMARY TARGET INTELLIGENCE SUMMARY paragraph (mission row text, DS:1F24).
    pub primary_summary: String,
    pub primary_objective: String,
    pub secondary_summary: String,
    pub secondary_objective: String,
    pub depart_line: String,
    pub land_line: String,
    pub roe_text: String,
    /// One weapon name per bay (2 bays on the Lockheed).
    pub armament: Vec<String>,
    pub fuel_line: String,
    /// Not part of the original briefing (and not in `lines`).
    #[serde(default)]
    pub weather_line: String,
    /// All non-empty lines in display order.
    pub lines: Vec<String>,
}

/// One line on the orders MFD page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrdersLine {
    pub text: String,
    /// Palette colour index used by VGAME.
    pub color: u8,
}

/// The in-flight orders MFD page (V:97A4) at mission start (no objective completed yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orders {
    /// The lines exactly as VGAME builds them (mixed case strings from VGAME.EXE).
    pub lines: Vec<OrdersLine>,
    /// Primary order as one upper-case line, e.g. "DESTROY ENEMY SS MISSILE AT LIBEREC".
    pub primary: String,
    pub secondary: String,
    /// "RETURN TO <landing base>".
    pub return_to: String,
}

/// Context for the text builders.
pub(crate) struct TextCtx<'a> {
    pub x: &'a StartTables,
    pub g: &'a GenResult,
    pub theater: usize,
    /// Start date (month, day, year) replacing the START table's.
    pub date: Option<(u8, u8, u16)>,
    pub kind: usize,
    pub roe: usize,
    pub lockheed: bool,
}

impl TextCtx<'_> {
    fn name(&self, i: i32) -> &str {
        self.g.world.name(i)
    }
    fn obj(&self, i: i32) -> &crate::world::WorldObject {
        let n = self.g.world.objects.len() as i32;
        &self.g.world.objects[i.rem_euclid(n) as usize]
    }
    /// Base/landmark display name: place name, or the shape name if unnamed.
    pub fn base_name(&self, i: i32) -> String {
        let o = self.obj(i);
        let k = if o.name != 0 { o.name as i32 } else { (o.object_idx & 0xFF) as i32 };
        self.name(k).to_string()
    }

    fn onc(b: &[u8; 4]) -> String {
        b.iter().map(|&c| c as char).collect()
    }

    /// Objective text by mission type (jump table 1A18:284E).
    pub fn objective(&self, slot: usize) -> String {
        let (g, x) = (self.g, self.x);
        let t = &g.targets[slot];
        let o = self.obj(t.idx);
        let place = if o.name != 0 { self.name(o.name as i32) } else { "" };
        let m = x.missions[t.num as usize];
        let coord = Self::onc(&t.coord);
        match m.mission_type {
            1 | 2 => {
                let verb = if m.mission_type == 1 { "Photograph" } else { "Destroy" };
                let what = self.name((o.object_idx & 0xFF) as i32);
                let mut s =
                    if o.name != 0 { format!("{verb} the {what} at {place}") } else { format!("{verb} the {what}") };
                if m.code & 0x20 != 0 {
                    s += &format!(" before {} hours, while the cargo is still being unloaded.", g.deadline);
                } else if m.code & 2 != 0 {
                    s += &format!(" before {} hours.", g.deadline);
                } else {
                    if s.ends_with(',') {
                        s.pop();
                    }
                    s += &x.dot;
                }
                s
            }
            3 => format!(
                "Reach the beacon at ONC {coord} and drop the supplies before {} hours. The beacon will be on ONLY during this period.",
                g.deadline
            ),
            4 => format!(
                "Reach the secret airstrip at ONC {coord} before {} hours. The airstrip will be lighted ONLY during this period.",
                g.deadline
            ),
            5 => format!(
                "Destroy the AN-72 Coaler leaving {} airbase at {} hours and arriving at {place} airbase at {} hours.",
                self.name(self.obj(g.special_start).name as i32),
                g.depart,
                g.deadline
            ),
            6 => format!("Intercept and destroy the AN-72 Coaler leaving {place} airbase, at {} hours.  Destination unknown.", g.depart),
            7 => format!(
                "Intercept and destroy the Tu-95, last seen at ONC {} at {} hours. It is probably heading for {place} airbase, and will arrive at {} hours.",
                g.intercept_ref.as_ref().map(Self::onc).unwrap_or_default(),
                g.depart,
                g.deadline
            ),
            8 => {
                let suf = if m.special == -2 { " fighter patrol" } else { "" };
                format!("Intercept and destroy the {}{suf} at ONC {coord}.", x.plane(g.world.flights[0].plane_type).name)
            }
            _ => String::new(),
        }
    }

    /// Briefing date (1A18:25D9).
    pub fn date(&self, sorties: u16) -> String {
        let x = self.x;
        let (m, d, y) = self.date.unwrap_or((x.date_month[self.theater], x.date_day[self.theater], x.date_year[self.theater]));
        let mut mon = m as usize;
        let mut yr = y as u32;
        let mut day = d as u32 + sorties as u32;
        while mon < x.month_len.len() && (x.month_len[mon] as u32) < day {
            day -= x.month_len[mon] as u32;
            mon += 1;
        }
        if mon > 12 {
            mon -= 12;
            yr += 1;
        }
        format!("{} {day}, {yr}", x.month_names.get(mon).map_or("", String::as_str))
    }

    pub fn briefing(&self, sorties: u16, pilot: &str) -> Briefing {
        let (g, x) = (self.g, self.x);
        let t = &g.targets;
        let date = self.date(sorties);
        let nbays = if self.lockheed { 2 } else { 4 };
        let b = Briefing {
            header: "416th Tactical Fighter Squadron".into(),
            title: KIND_TITLES[self.kind].into(),
            pilot_line: format!("Pilot: {pilot}."),
            base_date_line: format!("{} - {date}", self.base_name(t[0].base)),
            date,
            primary_summary: x.mission_text[t[0].num as usize].clone(),
            primary_objective: self.objective(0),
            secondary_summary: x.mission_text[t[1].num as usize].clone(),
            secondary_objective: self.objective(1),
            depart_line: format!("Depart from {} at {} hours.", self.base_name(t[0].base), g.depart),
            land_line: format!("Land at {}.", self.base_name(t[1].base)),
            roe_text: x.roe_text[self.roe].clone(),
            armament: g.weapons[..nbays].iter().map(|&w| x.weapon_name(w as i16).to_string()).collect(),
            fuel_line: format!("Est. Fuel required: {} lbs.", g.fuel),
            weather_line: String::new(),
            lines: Vec::new(),
        };
        let mut lines: Vec<String> = vec![
            b.header.clone(),
            b.title.clone(),
            b.pilot_line.clone(),
            b.base_date_line.clone(),
            "PRIMARY TARGET INTELLIGENCE SUMMARY:".into(),
            b.primary_summary.clone(),
            "OBJECTIVE:".into(),
            b.primary_objective.clone(),
            "SECONDARY TARGET INTELLIGENCE SUMMARY:".into(),
            b.secondary_summary.clone(),
            "OBJECTIVE:".into(),
            b.secondary_objective.clone(),
            "FLIGHT PLAN:".into(),
            b.depart_line.clone(),
            b.land_line.clone(),
            "RULES OF ENGAGEMENT:".into(),
            b.roe_text.clone(),
            "SUGGESTED ARMAMENT:".into(),
        ];
        lines.extend(b.armament.iter().cloned());
        lines.push(b.fuel_line.clone());
        lines.retain(|l| !l.is_empty());
        Briefing { lines, ..b }
    }

    // --- VGAME orders page ------------------------------------------------------------------

    /// VGAME `objName` (V:4A73): place name, or the shape name if unnamed; longer than 18
    /// characters is cut to 18 + ".".
    fn vgame_obj_name(&self, i: i32) -> String {
        let o = self.obj(i);
        let place = self.name(o.name as i32);
        let mut s: String = if !place.is_empty() { place.into() } else { self.name(o.shape() as i32).into() };
        if s.chars().count() > 18 {
            s = s.chars().take(18).collect::<String>() + ".";
        }
        s
    }

    /// VGAME clock formatter (V:95C4): `t + tod` as "HH:MM:SS" from 06:00 / 18:00.
    pub fn vgame_time(&self, t: i32) -> String {
        fn two(buf: &mut Vec<u8>, v: i32) {
            let r = s16(v) % 60;
            if r < 10 {
                buf.push(b'0');
            }
            buf.extend(r.to_string().bytes());
        }
        let t = (t + self.g.tod) as u16;
        let mut buf = Vec::new();
        two(&mut buf, (t / 1800) as i32);
        let night = self.g.night as u8;
        buf[0] = buf[0].wrapping_add(night);
        if buf.len() > 1 {
            buf[1] = buf[1].wrapping_add(if night != 0 { 8 } else { 6 });
            if buf[1] > b'9' {
                buf[1] -= 10;
                buf[0] += 1;
            }
        }
        buf.push(b':');
        two(&mut buf, (t / 30) as i32);
        buf.push(b':');
        two(&mut buf, (t as i32) << 1);
        buf.into_iter().map(|c| c as char).collect()
    }

    /// Orders MFD page (V:97A4).
    pub fn orders(&self) -> Orders {
        const VERBS: [&str; 8] = [
            "Photograph enemy",
            "Destroy enemy",
            "Drop supplies over",
            "Land supplies at",
            "Hit AN72 flying to",
            "Hit AN72 leaving",
            "Plane returning to",
            "Destroy plane over",
        ];
        let g = self.g;
        let mut lines = Vec::new();
        let mut summary = [String::new(), String::new()];
        for (k, t) in g.targets.iter().enumerate() {
            let color = if k == 0 { 0x11 } else { 0x29 };
            let verb = VERBS.get((t.ty - 1) as usize).copied().unwrap_or("");
            lines.push(OrdersLine { text: verb.into(), color });
            let o = self.obj(t.idx);
            let mut what = self.name(o.shape() as i32).to_string();
            let named = !self.name(o.name as i32).is_empty();
            let mut one = format!("{verb} {what}");
            if named {
                what += " at ";
                lines.push(OrdersLine { text: what, color });
                let place = self.vgame_obj_name(t.idx);
                one = format!("{one} at {place}");
                lines.push(OrdersLine { text: place, color });
            } else {
                lines.push(OrdersLine { text: what, color });
            }
            if t.code & 2 != 0 {
                let before = format!("Before {}", self.vgame_time(g.mtime));
                one = format!("{one} {before}");
                lines.push(OrdersLine { text: before, color: 7 });
            }
            summary[k] = one.to_uppercase();
        }
        lines.push(OrdersLine { text: "Return to".into(), color: 0xA });
        let base = self.vgame_obj_name(g.targets[1].base);
        let return_to = format!("RETURN TO {}", base.to_uppercase());
        lines.push(OrdersLine { text: base, color: 0xA });
        let [primary, secondary] = summary;
        Orders { lines, primary, secondary, return_to }
    }
}
