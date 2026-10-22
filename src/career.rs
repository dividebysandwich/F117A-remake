//! Pilot careers (not in the original game): the service record, ranks and medals, and the
//! tour of duty, a run of sorties in one theater where what the pilot destroys stays destroyed.
//! Pilots are saved in `<data dir>/stealth-mission-89/pilots/<name>.json`.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};

use stealth_mission::{Mission, MissionKind, Seed};
use stealth_sim::WorldDamage;
use stealth_sim::score::{MissionReport, Outcome};

use crate::menu::{AMBER, BRIGHT, DIM, GREEN, MenuState, THEATERS, big_button, section};
use crate::mission::MissionData;
use crate::settings::Settings;

/// Rank, abbreviation, career points needed.
pub const RANKS: [(&str, &str, i32); 6] = [
    ("Second Lieutenant", "2nd Lt.", 0),
    ("First Lieutenant", "1st Lt.", 300),
    ("Captain", "Capt.", 800),
    ("Major", "Maj.", 1800),
    ("Lieutenant Colonel", "Lt. Col.", 3500),
    ("Colonel", "Col.", 6000),
];
/// Decorations for one mission's points (the highest earned is awarded).
pub const MEDALS: [(&str, i32); 5] = [
    ("Air Medal", 150),
    ("Distinguished Flying Cross", 300),
    ("Silver Star", 500),
    ("Air Force Cross", 800),
    ("Medal of Honor", 1200),
];
pub const TOUR_LENGTH: u32 = 12;
/// Points for finishing a tour.
pub const TOUR_BONUS: i32 = 200;
/// Seeds tried for a sortie whose targets still stand.
const SORTIE_TRIES: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Active,
    /// Killed in action.
    Kia,
    /// Prisoner of war.
    Pow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortieRecord {
    pub date: String,
    pub theater: String,
    pub orders: String,
    pub outcome: String,
    pub points: i32,
    pub rating: String,
    pub medal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tour {
    pub theater: usize,
    pub roe: u8,
    pub opponents: u8,
    pub lockheed: bool,
    /// Sorties flown so far.
    pub sortie: u32,
    pub length: u32,
    pub points: i32,
    /// Everything destroyed in this tour.
    pub damage: WorldDamage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pilot {
    pub name: String,
    pub points: i32,
    pub sorties: u32,
    /// Decorations and how often each was awarded.
    pub medals: Vec<(String, u32)>,
    pub status: Status,
    pub tour: Option<Tour>,
    pub tours_completed: u32,
    pub history: Vec<SortieRecord>,
}

/// The flight is a tour sortie of this pilot (file key).
#[derive(Resource, Debug, Clone)]
pub struct CareerFlight(pub String);

fn pilots_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("stealth-mission-89").join("pilots"))
}

/// File key of a pilot name (letters, digits, '-').
pub fn file_key(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

impl Pilot {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.trim().to_string(),
            points: 0,
            sorties: 0,
            medals: Vec::new(),
            status: Status::Active,
            tour: None,
            tours_completed: 0,
            history: Vec::new(),
        }
    }

    pub fn key(&self) -> String {
        file_key(&self.name)
    }

    pub fn rank(&self) -> usize {
        RANKS.iter().rposition(|r| self.points >= r.2).unwrap_or(0)
    }

    /// Rank and name, e.g. "Capt. Smith".
    pub fn title(&self) -> String {
        format!("{} {}", RANKS[self.rank()].1, self.name)
    }

    pub fn load(key: &str) -> Option<Self> {
        let path = pilots_dir()?.join(format!("{key}.json"));
        serde_json::from_str(&std::fs::read_to_string(&path).ok()?)
            .map_err(|e| warn!("{}: {e}", path.display()))
            .ok()
    }

    pub fn load_all() -> Vec<Self> {
        let Some(dir) = pilots_dir() else { return Vec::new() };
        let mut v: Vec<Self> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).and_then(Self::load))
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn save(&self) {
        let Some(dir) = pilots_dir() else { return };
        let text = match serde_json::to_string_pretty(self) {
            Ok(t) => t,
            Err(e) => return warn!("saving pilot {}: {e}", self.name),
        };
        if let Err(e) = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join(format!("{}.json", self.key())), text))
        {
            warn!("saving pilot {}: {e}", self.name);
        }
    }

    /// Records a tour sortie; returns the news for the debrief.
    pub fn record(
        &mut self,
        report: &MissionReport,
        damage: Option<&WorldDamage>,
        m: &Mission,
    ) -> Vec<String> {
        let mut news = Vec::new();
        let rank = self.rank();
        self.sorties += 1;
        let points = if report.outcome == Outcome::Aborted { 0 } else { report.total };
        self.points += points;
        let medal = (report.primary_done || report.drones_killed > 0 || report.air_kills > 0)
            .then(|| {
                MEDALS
                    .iter()
                    .rev()
                    .find(|(_, need)| report.total >= *need)
                    .map(|(n, _)| n.to_string())
            })
            .flatten();
        let mut award = |name: &str, news: &mut Vec<String>| {
            match self.medals.iter_mut().find(|(n, _)| n == name) {
                Some((_, k)) => *k += 1,
                None => self.medals.push((name.to_string(), 1)),
            }
            news.push(format!("Awarded the {name}"));
        };
        if let Some(n) = &medal {
            award(n, &mut news);
        }
        if report.hits_taken > 0 && !report.outcome.fatal() {
            award("Purple Heart", &mut news);
        }
        match report.outcome {
            Outcome::Crashed | Outcome::EjectionFailed => {
                self.status = Status::Kia;
                news.push("Killed in action".into());
            }
            Outcome::Captured => {
                self.status = Status::Pow;
                award("Prisoner of War Medal", &mut news);
                news.push("Taken prisoner: the career is over".into());
            }
            _ => {}
        }
        if let Some(t) = self.tour.as_mut() {
            t.sortie += 1;
            t.points += points;
            if let Some(d) = damage {
                t.damage.merge(d);
            }
            news.push(format!("Tour of duty: sortie {} of {}", t.sortie, t.length));
            if self.status == Status::Active && t.sortie >= t.length {
                news.push(format!("Tour of duty complete: {} points (+{TOUR_BONUS})", t.points));
                self.points += TOUR_BONUS;
                self.tours_completed += 1;
                self.tour = None;
            }
        }
        if self.status != Status::Active {
            self.tour = None;
        }
        if self.rank() > rank {
            news.push(format!("Promoted to {}", RANKS[self.rank()].0));
        }
        self.history.push(SortieRecord {
            date: m.briefing.date.clone(),
            theater: m.theater_name.clone(),
            orders: m.orders.primary.clone(),
            outcome: report.outcome.text().into(),
            points,
            rating: report.rating.clone(),
            medal,
        });
        news
    }
}

/// Generates the next sortie of a pilot's tour: the first seed from the menu's on whose
/// targets still stand, in the menu's weather. Mostly strikes, some intercepts, in Ukraine also
/// drone defense.
pub fn next_sortie(
    data: &MissionData,
    pilot: &Pilot,
    menu: &MenuState,
    aircraft: crate::aircraft::AircraftKind,
) -> Option<(u16, Mission)> {
    let t = pilot.tour.as_ref()?;
    let drones = THEATERS.get(t.theater).is_some_and(|(id, _)| *id == "ua");
    for k in 0..SORTIE_TRIES {
        let s = menu.seed.wrapping_add(k as u16 * 7919);
        let kind = match s % 20 {
            0..=11 => MissionKind::Strike,
            12..=15 if drones => MissionKind::DroneDefense,
            _ => MissionKind::Air,
        };
        let mut settings = crate::mission::settings(
            t.theater,
            t.roe,
            kind,
            t.opponents,
            t.lockheed && aircraft == crate::aircraft::AircraftKind::F117,
            menu.weather,
            menu.cloud_cover,
        );
        settings.pilot = pilot.title();
        settings.sorties = pilot.sorties as u16;
        let Ok(m) = stealth_mission::generate(&data.0, &settings, Seed::Srand(s)) else { continue };
        let standing = |tg: &stealth_mission::Target| {
            !t.damage.objects.contains(&(tg.object_index as u16))
                && !t.damage.near(tg.position.x, tg.position.y, 0x20)
        };
        if standing(&m.primary) && standing(&m.secondary) {
            return Some((s, m));
        }
    }
    None
}

/// The career window: pilots, their records, and the tours.
#[derive(Resource, Default)]
pub struct CareerWindow {
    pub open: bool,
    pilots: Vec<Pilot>,
    selected: Option<String>,
    new_name: String,
}

impl CareerWindow {
    pub fn opened(open: bool) -> Self {
        let mut w = Self::default();
        if open {
            w.show();
            w.selected = w.pilots.first().map(Pilot::key);
        }
        w
    }

    pub fn show(&mut self) {
        self.open = true;
        self.pilots = Pilot::load_all();
    }
}

/// The career window in place of the main menu: the tour options, the pilots and their
/// records.
pub fn career_ui(
    ctx: &egui::Context,
    w: &mut CareerWindow,
    state: &mut MenuState,
    settings: &mut ResMut<Settings>,
    head: Option<crate::headtrack::Source>,
) {
    if !w.open {
        return;
    }
    let mut close = false;
    // A tour flies in its own theater, situation and opponents.
    let tour = w
        .selected
        .as_ref()
        .and_then(|k| w.pilots.iter().find(|p| p.key() == *k))
        .filter(|p| p.status == Status::Active)
        .and_then(|p| p.tour.clone());
    if let Some(t) = &tour {
        (state.theater, state.roe, state.opponents, state.lockheed) =
            (t.theater, t.roe, t.opponents, t.lockheed);
    }
    let width = crate::menu::wide_width(ctx);
    crate::menu::menu_window(ctx, "Career", width).show(ctx, |ui| {
        ui.label(egui::RichText::new("PILOT RECORDS").heading().color(BRIGHT));
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(210.0);
                ui.add_enabled_ui(tour.is_none(), |ui| {
                    crate::menu::theater_options(ui, &mut state.theater);
                    crate::menu::opponent_options(ui, &mut state.opponents);
                    crate::menu::situation_options(ui, &mut state.roe);
                });
                if tour.is_some() {
                    ui.label(egui::RichText::new("Set by the pilot's tour").small().color(DIM));
                }
            });
            ui.vertical(|ui| {
                ui.set_width(190.0);
                let lockheed = tour.is_none().then_some(&mut state.lockheed);
                crate::menu::aircraft_options(ui, settings, lockheed);
                crate::menu::realism_options(ui, settings);
            });
            ui.vertical(|ui| {
                ui.set_width(200.0);
                crate::menu::weather_options(ui, &mut state.weather, &mut state.cloud_cover);
                crate::menu::classic_modern_options(ui, settings, head);
            });
            ui.vertical(|ui| {
                ui.set_width(260.0);
                section(ui, "Pilots");
                for p in &w.pilots {
                    let status = match p.status {
                        Status::Active => "",
                        Status::Kia => " (KIA)",
                        Status::Pow => " (POW)",
                    };
                    let text = format!("{}{status}", p.title());
                    if ui.selectable_label(w.selected.as_deref() == Some(&p.key()), text).clicked()
                    {
                        w.selected = Some(p.key());
                    }
                }
                section(ui, "New pilot");
                crate::text_entry::text_entry(
                    ui,
                    "new_pilot",
                    &mut w.new_name,
                    24,
                    crate::text_entry::Charset::Name,
                    settings.vr,
                    "Name",
                );
                let name = w.new_name.trim().to_string();
                let taken = w.pilots.iter().any(|p| p.key() == file_key(&name));
                if ui
                    .add_enabled(
                        !name.is_empty() && !taken && name.len() <= 24,
                        egui::Button::new("ENLIST"),
                    )
                    .clicked()
                {
                    let p = Pilot::new(&name);
                    p.save();
                    w.selected = Some(p.key());
                    w.pilots = Pilot::load_all();
                    w.new_name.clear();
                }
            });
            ui.vertical(|ui| {
                let Some(p) =
                    w.selected.as_ref().and_then(|k| w.pilots.iter_mut().find(|p| p.key() == *k))
                else {
                    ui.label(egui::RichText::new("Select or enlist a pilot").color(DIM));
                    return;
                };
                section(ui, "Service record");
                egui::Grid::new("record").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                    let rows = [
                        ("RANK", RANKS[p.rank()].0.to_string()),
                        ("POINTS", p.points.to_string()),
                        ("SORTIES", p.sorties.to_string()),
                        ("TOURS", p.tours_completed.to_string()),
                        (
                            "STATUS",
                            match p.status {
                                Status::Active => "Active duty",
                                Status::Kia => "Killed in action",
                                Status::Pow => "Prisoner of war",
                            }
                            .to_string(),
                        ),
                    ];
                    for (k, v) in rows {
                        ui.label(egui::RichText::new(k).size(14.0).color(DIM));
                        ui.label(
                            egui::RichText::new(v.to_ascii_uppercase()).size(18.0).color(GREEN),
                        );
                        ui.end_row();
                    }
                });
                if !p.medals.is_empty() {
                    section(ui, "Decorations");
                    for (m, n) in &p.medals {
                        ui.label(if *n > 1 { format!("{m} x{n}") } else { m.clone() });
                    }
                }
                section(ui, "Tour of duty");
                match (&p.tour, p.status) {
                    (Some(t), Status::Active) => {
                        ui.label(format!(
                            "{}  {}  sortie {} of {}  {} points",
                            THEATERS[t.theater].1,
                            stealth_mission::ROE_NAMES[t.roe as usize],
                            t.sortie + 1,
                            t.length,
                            t.points
                        ));
                        ui.label(
                            egui::RichText::new(format!(
                                "{} targets destroyed so far",
                                t.damage.positions.len()
                            ))
                            .small()
                            .color(DIM),
                        );
                        if ui.add(big_button("FLY NEXT SORTIE").fill(DIM)).clicked() {
                            state.start_career(p, t);
                            state.mission_screen = true;
                            close = true;
                        }
                    }
                    (None, Status::Active) => {
                        ui.label(egui::RichText::new(format!(
                            "{TOUR_LENGTH} sorties in {}, {}, against {} opponents",
                            THEATERS[state.theater].1,
                            stealth_mission::ROE_NAMES[state.roe as usize],
                            stealth_mission::OPPONENT_NAMES[state.opponents as usize]
                                .to_ascii_lowercase()
                        )))
                        .on_hover_text("What you destroy stays destroyed for the rest of the tour");
                        if ui.add(big_button("START TOUR").fill(DIM)).clicked() {
                            let t = Tour {
                                theater: state.theater,
                                roe: state.roe,
                                opponents: state.opponents,
                                lockheed: state.lockheed,
                                sortie: 0,
                                length: TOUR_LENGTH,
                                points: 0,
                                damage: WorldDamage::default(),
                            };
                            p.tour = Some(t.clone());
                            p.save();
                            state.start_career(p, &t);
                            state.mission_screen = true;
                            close = true;
                        }
                    }
                    _ => {
                        ui.label(egui::RichText::new("Retired from active duty").color(AMBER));
                    }
                }
                section(ui, "Last sorties");
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for s in p.history.iter().rev().take(20) {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}  {}  {:+}  {}",
                                s.date,
                                s.outcome,
                                s.points,
                                s.medal.clone().unwrap_or_default()
                            ))
                            .small(),
                        );
                        ui.label(
                            egui::RichText::new(format!("   {}", s.orders)).small().color(DIM),
                        );
                    }
                });
            });
        });
        ui.add_space(8.0);
        ui.separator();
        if ui.add(big_button("BACK")).on_hover_text("Back to the main menu").clicked() {
            close = true;
        }
    });
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        w.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission() -> Mission {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/mission");
        let data = stealth_mission::GameData::load_json(&dir).unwrap();
        let s = stealth_mission::MissionSettings { theater: 3, ..Default::default() };
        stealth_mission::generate(&data, &s, Seed::Srand(77)).unwrap()
    }

    fn report(outcome: Outcome, total: i32) -> MissionReport {
        MissionReport {
            outcome,
            training: false,
            primary_done: true,
            secondary_done: false,
            air_kills: 0,
            ground_kills: 1,
            unseen_kills: 0,
            weapons_fired: 2,
            hits_taken: 0,
            drones_killed: 0,
            drones_leaked: 0,
            nuclear_hit: false,
            radar_ids: 0,
            visual_ids: 0,
            time: 600,
            points: Vec::new(),
            multiplier: 1.0,
            total,
            rating: stealth_sim::score::rating(total).into(),
            timeline: Vec::new(),
        }
    }

    #[test]
    fn ranks_medals_and_the_tour() {
        let m = mission();
        let mut p = Pilot::new("Test Pilot");
        assert_eq!((p.title().as_str(), p.key().as_str()), ("2nd Lt. Test Pilot", "test-pilot"));
        p.tour = Some(Tour {
            theater: 3,
            roe: 1,
            opponents: 1,
            lockheed: false,
            sortie: 0,
            length: 2,
            points: 0,
            damage: WorldDamage::default(),
        });
        let hit = WorldDamage { tiles: Vec::new(), objects: vec![40], positions: vec![(100, 200)] };
        let news = p.record(&report(Outcome::Landed, 350), Some(&hit), &m);
        assert!(news.contains(&"Awarded the Distinguished Flying Cross".to_string()), "{news:?}");
        assert!(news.contains(&"Promoted to First Lieutenant".to_string()), "{news:?}");
        assert_eq!(p.tour.as_ref().unwrap().damage, hit);
        // The second sortie ends the tour with the bonus.
        let news = p.record(&report(Outcome::Landed, 100), None, &m);
        assert!(p.tour.is_none() && p.tours_completed == 1, "{news:?}");
        assert_eq!(p.points, 350 + 100 + TOUR_BONUS);
        assert_eq!(p.history.len(), 2);
        // Captured: the career is over.
        p.tour = Some(Tour {
            theater: 3,
            roe: 1,
            opponents: 1,
            lockheed: false,
            sortie: 0,
            length: 12,
            points: 0,
            damage: WorldDamage::default(),
        });
        p.record(&report(Outcome::Captured, -80), None, &m);
        assert_eq!(p.status, Status::Pow);
        assert!(p.tour.is_none());
        assert!(p.medals.iter().any(|(n, _)| n == "Prisoner of War Medal"));
    }
}
