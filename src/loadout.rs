//! Loadout editor (otherwise the mission generator picks the stores): the stores on each
//! hardpoint, the F-117A's bays (two real ones, two more inside the wings without realistic
//! weapons) or the F-16's nine pylon stations.

use bevy::prelude::*;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};
use stealth_mission::{Mission, Plane};

use crate::aircraft::AircraftKind;
use crate::menu::{AMBER, BRIGHT, DIM, GREEN, MenuState, big_button, facts, section};

/// The weapons (index = weapon type): name, what it is for.
pub const WEAPONS: [(&str, &str); 19] = [
    ("AIM-9M Sidewinder", "Air-to-air, infrared homing, short range"),
    ("AIM-120A AMRAAM", "Air-to-air, radar homing, medium range"),
    ("AGM-88A HARM", "Anti-radiation missile: radars and SAM sites"),
    ("Penguin 3 ASM", "Anti-ship missile"),
    ("AGM-84A Harpoon", "Long range anti-ship missile"),
    ("AGM-65 Maverick", "TV guided missile: vehicles, bunkers, buildings"),
    ("GBU-12 Paveway", "Laser guided bomb: hard targets"),
    ("Mk 20 Rockeye", "Cluster bomb: soft and area targets"),
    ("Durandal", "Runway cratering bomb"),
    ("Mk 82 Slick", "500 lb free-fall bomb"),
    ("Mk 82 Snakeye", "500 lb retarded bomb for low releases"),
    ("Mk 20 Rockeye II", "Cluster bomb: armour and area targets"),
    ("Mk 122 Fireye", "Incendiary bomb"),
    ("CBU-72 FAE", "Fuel-air explosive: area targets"),
    ("Mk 35 IN Cluster", "Incendiary cluster bomb"),
    ("ISCB-1 Minelets", "Area denial mines"),
    ("135 mm IR camera", "Reconnaissance photos of the target"),
    ("1900 lbs fuel", "Extra fuel, transferred when released"),
    ("Special equipment", "Supplies or cargo for the mission"),
];
/// Weapon types kept out of the real F-117A's bays (realistic weapons).
const AIR_TO_AIR: [u8; 2] = [0, 1];
/// Rounds of each weapon a bay holds.
const BAY_QTY: [u8; 19] = [4, 3, 1, 2, 1, 2, 2, 2, 2, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1];
/// Rounds on a pylon (with a triple ejector rack for the 500 lb bombs).
const PYLON_QTY: [u8; 19] = [1, 1, 1, 1, 1, 1, 1, 2, 2, 3, 3, 2, 2, 1, 2, 1, 1, 1, 1];

/// A bay or a pylon.
pub struct Hardpoint {
    pub name: &'static str,
    /// Short label on the drawing.
    pub label: &'static str,
    /// Position seen from above: metres right of the centreline, metres aft of the nose.
    pub pos: (f32, f32),
    pub bay: bool,
    /// Not on the real aircraft.
    pub fantasy: bool,
    /// The same station on the other side.
    pub mirror: Option<usize>,
    pub loads: &'static [u8],
}

impl Hardpoint {
    pub fn rounds(&self, w: u8) -> u8 {
        let t = if self.bay { &BAY_QTY } else { &PYLON_QTY };
        t.get(w as usize).copied().unwrap_or(0)
    }
}

pub struct Layout {
    /// The exterior model's outline seen from above (metres: x right of the centreline, y aft
    /// of the nose).
    pub outline: &'static [(f32, f32)],
    pub half_span: f32,
    pub length: f32,
    pub hardpoints: &'static [Hardpoint],
}

const ALL: [u8; 19] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
const NO_AIR_TO_AIR: [u8; 17] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];

// tools/models/planform.py
/// stflt_024 seen from above: half span 4.86 m, length 15.62 m.
const F117_OUTLINE: &[(f32, f32)] = &[
    (0.00, 0.00),
    (0.02, 0.32),
    (0.18, 0.34),
    (0.22, 0.88),
    (0.42, 0.90),
    (0.46, 1.48),
    (0.96, 2.58),
    (4.86, 12.34),
    (4.86, 12.86),
    (4.38, 13.32),
    (2.08, 10.86),
    (1.88, 10.86),
    (1.82, 11.38),
    (0.66, 12.62),
    (0.66, 12.90),
    (1.08, 14.52),
    (1.08, 15.62),
    (0.12, 13.58),
    (0.00, 13.56),
    (-0.12, 13.58),
    (-1.08, 15.62),
    (-1.08, 14.52),
    (-0.66, 12.90),
    (-0.66, 12.62),
    (-1.82, 11.38),
    (-1.88, 10.86),
    (-2.08, 10.86),
    (-4.38, 13.32),
    (-4.86, 12.86),
    (-4.86, 12.34),
    (-0.96, 2.58),
    (-0.46, 1.48),
    (-0.42, 0.90),
    (-0.22, 0.88),
    (-0.18, 0.34),
    (-0.02, 0.32),
];
/// stflt_025 (tools/models/uh60.py) seen from above, without its rotors: half span (the
/// stabilator) 2.16 m, length 15.1 m.
const UH60_OUTLINE: &[(f32, f32)] = &[
    (0.00, 0.00),
    (0.55, 0.22),
    (0.92, 0.57),
    (1.08, 0.90),
    (1.18, 1.47),
    (1.20, 2.12),
    (1.20, 7.37),
    (0.41, 8.17),
    (0.32, 10.02),
    (0.21, 12.32),
    (2.16, 13.32),
    (2.16, 14.20),
    (0.17, 13.90),
    (0.10, 15.10),
    (-0.10, 15.10),
    (-0.17, 13.90),
    (-2.16, 14.20),
    (-2.16, 13.32),
    (-0.21, 12.32),
    (-0.32, 10.02),
    (-0.41, 8.17),
    (-1.20, 7.37),
    (-1.20, 2.12),
    (-1.18, 1.47),
    (-1.08, 0.90),
    (-0.92, 0.57),
    (-0.55, 0.22),
];
/// stflt_023 seen from above: half span 5.12 m, length 17.12 m.
const F16_OUTLINE: &[(f32, f32)] = &[
    (0.00, 0.00),
    (0.32, 0.88),
    (0.64, 3.76),
    (1.26, 7.22),
    (4.80, 10.32),
    (5.02, 10.30),
    (5.02, 8.86),
    (5.06, 8.56),
    (5.12, 8.94),
    (5.12, 12.12),
    (5.02, 12.12),
    (5.00, 11.70),
    (0.76, 11.72),
    (0.74, 12.54),
    (3.04, 14.24),
    (3.04, 15.28),
    (0.54, 14.98),
    (0.48, 15.60),
    (0.30, 15.66),
    (0.00, 17.12),
    (-0.30, 15.66),
    (-0.48, 15.60),
    (-0.54, 14.98),
    (-3.04, 15.28),
    (-3.04, 14.24),
    (-0.74, 12.54),
    (-0.76, 11.72),
    (-5.00, 11.70),
    (-5.02, 12.12),
    (-5.12, 12.12),
    (-5.12, 8.94),
    (-5.06, 8.56),
    (-5.02, 8.86),
    (-5.02, 10.30),
    (-4.80, 10.32),
    (-1.26, 7.22),
    (-0.64, 3.76),
    (-0.32, 0.88),
];

const fn bay(
    name: &'static str,
    label: &'static str,
    x: f32,
    y: f32,
    fantasy: bool,
    mirror: usize,
    loads: &'static [u8],
) -> Hardpoint {
    Hardpoint { name, label, pos: (x, y), bay: true, fantasy, mirror: Some(mirror), loads }
}

const fn pylon(
    name: &'static str,
    label: &'static str,
    x: f32,
    y: f32,
    mirror: Option<usize>,
    loads: &'static [u8],
) -> Hardpoint {
    Hardpoint { name, label, pos: (x, y), bay: false, fantasy: false, mirror, loads }
}

/// The real F-117A: two bays, no air-to-air missiles.
static F117_REAL: [Hardpoint; 2] = [
    bay("Left bay", "L", -0.5, 8.0, false, 1, &NO_AIR_TO_AIR),
    bay("Right bay", "R", 0.5, 8.0, false, 0, &NO_AIR_TO_AIR),
];
/// The fantasy F-117A: two more bays, inside the wing roots.
static F117_FANTASY: [Hardpoint; 4] = [
    bay("Left bay", "L", -0.5, 8.0, false, 1, &ALL),
    bay("Right bay", "R", 0.5, 8.0, false, 0, &ALL),
    bay("Left wing bay", "LW", -2.3, 9.0, true, 3, &ALL),
    bay("Right wing bay", "RW", 2.3, 9.0, true, 2, &ALL),
];
const TIP: [u8; 2] = [0, 1];
const WING_MID: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const WING_INNER: [u8; 13] = [2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17];
const CENTRE: [u8; 3] = [16, 17, 18];
/// The F-16's stations 1 (left wingtip) to 9 (right wingtip).
static F16_STATIONS: [Hardpoint; 9] = [
    pylon("Station 1, left wingtip", "1", -5.06, 10.6, Some(8), &TIP),
    pylon("Station 2, left outer wing", "2", -4.1, 10.7, Some(7), &TIP),
    pylon("Station 3, left wing", "3", -3.1, 10.3, Some(6), &WING_MID),
    pylon("Station 4, left inner wing", "4", -2.0, 9.9, Some(5), &WING_INNER),
    pylon("Station 5, centreline", "5", 0.0, 9.5, None, &CENTRE),
    pylon("Station 6, right inner wing", "6", 2.0, 9.9, Some(3), &WING_INNER),
    pylon("Station 7, right wing", "7", 3.1, 10.3, Some(2), &WING_MID),
    pylon("Station 8, right outer wing", "8", 4.1, 10.7, Some(1), &TIP),
    pylon("Station 9, right wingtip", "9", 5.06, 10.6, Some(0), &TIP),
];

static F117_REAL_LAYOUT: Layout =
    Layout { outline: F117_OUTLINE, half_span: 4.86, length: 15.62, hardpoints: &F117_REAL };
static F117_FANTASY_LAYOUT: Layout =
    Layout { outline: F117_OUTLINE, half_span: 4.86, length: 15.62, hardpoints: &F117_FANTASY };
static F16_LAYOUT: Layout =
    Layout { outline: F16_OUTLINE, half_span: 5.12, length: 17.12, hardpoints: &F16_STATIONS };
/// The UH-60A carries troops, no stores.
static UH60_LAYOUT: Layout =
    Layout { outline: UH60_OUTLINE, half_span: 2.16, length: 15.1, hardpoints: &[] };

/// The hardpoints of an aircraft (`realistic`: the F-117A's real two bays).
pub fn layout(aircraft: AircraftKind, realistic: bool) -> &'static Layout {
    match aircraft {
        AircraftKind::F117 if realistic => &F117_REAL_LAYOUT,
        AircraftKind::F117 => &F117_FANTASY_LAYOUT,
        AircraftKind::F16 => &F16_LAYOUT,
        AircraftKind::UH60 => &UH60_LAYOUT,
    }
}

/// Weapon types the mission has none of (random shortages).
fn out_of_stock(m: &Mission, w: u8) -> bool {
    !AIR_TO_AIR.contains(&w) && m.weapon_unavailable.get(w as usize).is_some_and(|&u| u != 0)
}

/// The stores flown: a weapon type (or nothing) on each hardpoint.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedLoadout {
    /// The mission it belongs to (`PlannedRoute::key`).
    pub mission: (String, u8, u32),
    pub aircraft: AircraftKind,
    pub realistic: bool,
    pub stores: Vec<Option<u8>>,
}

/// File the menu hands the loadout to the VR process in (`--loadout`).
pub const LOADOUT_FILE: &str = "loadout.json";

impl PlannedLoadout {
    /// The generator's stores: in the F-117A's bays as they are, on the F-16 each on the
    /// first free pair of stations that carries it, and Sidewinders on the free wingtips.
    pub fn of(m: &Mission, aircraft: AircraftKind) -> Self {
        let realistic = m.settings.plane == Plane::Lockheed;
        let lay = layout(aircraft, realistic);
        let briefed = m.loadout.bays.iter().take(m.loadout.usable_bays).map(|b| b.weapon as u8);
        let mut stores = vec![None; lay.hardpoints.len()];
        match aircraft {
            AircraftKind::F117 => {
                for (s, w) in stores.iter_mut().zip(briefed) {
                    *s = Some(w);
                }
            }
            AircraftKind::UH60 => {}
            AircraftKind::F16 => {
                for w in briefed.chain([0]) {
                    // Stations preferred for each kind of store (pairs by their left station).
                    let order: &[usize] = match w {
                        0 => &[0, 1, 2],
                        1 => &[1, 0, 2],
                        16 | 18 => &[4],
                        17 => &[4, 3],
                        _ => &[2, 3],
                    };
                    if w == 0 && stores.contains(&Some(0)) {
                        continue;
                    }
                    if let Some(&i) = order
                        .iter()
                        .find(|&&i| stores[i].is_none() && lay.hardpoints[i].loads.contains(&w))
                    {
                        stores[i] = Some(w);
                        if let Some(j) = lay.hardpoints[i].mirror {
                            stores[j] = Some(w);
                        }
                    }
                }
            }
        }
        Self { mission: crate::mission::PlannedRoute::key(m), aircraft, realistic, stores }
    }

    pub fn fits(&self, m: &Mission, aircraft: AircraftKind) -> bool {
        self.mission == crate::mission::PlannedRoute::key(m)
            && self.aircraft == aircraft
            && self.realistic == (m.settings.plane == Plane::Lockheed)
            && self.stores.len() == self.layout().hardpoints.len()
    }

    pub fn layout(&self) -> &'static Layout {
        layout(self.aircraft, self.realistic)
    }

    /// The simulation's stations: (weapon type, rounds), -1 empty.
    pub fn stations(&self) -> Vec<(i32, i32)> {
        self.layout()
            .hardpoints
            .iter()
            .zip(&self.stores)
            .map(|(h, s)| s.map_or((-1, 0), |w| (w as i32, h.rounds(w) as i32)))
            .collect()
    }

    /// The station selected at the start: the one with the briefed main weapon, else the first
    /// loaded one.
    pub fn first_choice(&self, m: &Mission) -> usize {
        let main = m.loadout.bays.first().map(|b| b.weapon as u8);
        self.stores
            .iter()
            .position(|s| s.is_some() && *s == main)
            .or_else(|| self.stores.iter().position(Option::is_some))
            .unwrap_or(0)
    }

    /// The stores in words, e.g. "4 AIM-9M Sidewinder, 2 GBU-12 Paveway".
    pub fn summary(&self) -> String {
        let mut totals: Vec<(u8, u32)> = Vec::new();
        for (h, s) in self.layout().hardpoints.iter().zip(&self.stores) {
            if let Some(w) = *s {
                match totals.iter_mut().find(|t| t.0 == w) {
                    Some(t) => t.1 += h.rounds(w) as u32,
                    None => totals.push((w, h.rounds(w) as u32)),
                }
            }
        }
        if totals.is_empty() {
            return "None".into();
        }
        totals
            .iter()
            .map(|(w, n)| format!("{n} {}", WEAPONS[*w as usize].0))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The loadout window.
#[derive(Resource, Default)]
pub struct LoadoutWindow {
    pub open: bool,
    selected: usize,
    mirror: Option<bool>,
}

impl LoadoutWindow {
    pub fn opened(open: bool) -> Self {
        Self { open, ..default() }
    }
}

pub fn loadout_ui(
    ctx: &egui::Context,
    w: &mut LoadoutWindow,
    state: &mut MenuState,
    aircraft: AircraftKind,
) {
    if !w.open {
        return;
    }
    let Some(Ok(m)) = state.mission.clone() else {
        w.open = false;
        return;
    };
    let lo = state.loadout.get_or_insert_with(|| PlannedLoadout::of(&m, aircraft));
    if !lo.fits(&m, aircraft) {
        *lo = PlannedLoadout::of(&m, aircraft);
    }
    let screen = ctx.content_rect();
    let size = egui::vec2(
        (screen.width() - 32.0).clamp(600.0, 1100.0),
        (screen.height() - 32.0).clamp(400.0, 760.0),
    );
    egui::Window::new("Loadout")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .fixed_size(size)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(12.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("LOADOUT").heading().color(BRIGHT));
                ui.label(
                    egui::RichText::new(format!("  {}", aircraft.name().to_ascii_uppercase()))
                        .color(DIM),
                );
            });
            ui.separator();
            editor(ui, w, lo, &m, size.y);
            ui.separator();
            ui.horizontal(|ui| {
                if facts(
                    ui.add(big_button("BRIEFED")),
                    &["The stores the mission was briefed with"],
                )
                .clicked()
                {
                    *lo = PlannedLoadout::of(&m, aircraft);
                }
                if facts(ui.add(big_button("CLEAR")), &["Every hardpoint empty"]).clicked() {
                    lo.stores.iter_mut().for_each(|s| *s = None);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(big_button("DONE").fill(DIM)).clicked() {
                        w.open = false;
                    }
                });
            });
        });
}

/// The editor: the aircraft's hardpoints and the stores of the selected one (the menu's
/// loadout window, the campaign's briefing). `height`: the space it may take.
pub fn editor(
    ui: &mut egui::Ui,
    w: &mut LoadoutWindow,
    lo: &mut PlannedLoadout,
    m: &Mission,
    height: f32,
) {
    let lay = lo.layout();
    w.selected = w.selected.min(lay.hardpoints.len() - 1);
    let mirror = w.mirror.get_or_insert(true);
    let size = egui::vec2(ui.available_width(), height);
    ui.horizontal_top(|ui| {
        let side = 380.0;
        let draw = egui::vec2(ui.available_width() - side - 12.0, ui.available_height() - 60.0);
        if let Some(i) = diagram(ui, lay, &lo.stores, w.selected, draw) {
            w.selected = i;
        }
        ui.vertical(|ui| {
            ui.set_width(side);
            let hp = &lay.hardpoints[w.selected];
            section(ui, hp.name);
            if hp.fantasy {
                ui.label(egui::RichText::new("Not on the real aircraft").small().color(AMBER));
            }
            let mut pick = lo.stores[w.selected];
            egui::ScrollArea::vertical().max_height(size.y - 260.0).show(ui, |ui| {
                ui.radio_value(&mut pick, None, "Empty");
                for &wpn in hp.loads {
                    let (name, role) = WEAPONS[wpn as usize];
                    let stock = !out_of_stock(m, wpn);
                    let r = ui.add_enabled(
                        stock,
                        egui::RadioButton::new(
                            pick == Some(wpn),
                            format!("{}x {name}", hp.rounds(wpn)),
                        ),
                    );
                    let r = r.on_disabled_hover_text("None in stock for this mission");
                    if facts(r, &[role]).clicked() {
                        pick = Some(wpn);
                    }
                }
            });
            if let Some(j) = hp.mirror {
                ui.add_space(4.0);
                facts(
                    ui.checkbox(mirror, "Same on both sides"),
                    &[&format!("Loads {} too", lay.hardpoints[j].name.to_ascii_lowercase())],
                );
            }
            if pick != lo.stores[w.selected] {
                lo.stores[w.selected] = pick;
                if *mirror && let Some(j) = hp.mirror {
                    lo.stores[j] = pick;
                }
            }
            section(ui, "Stores");
            ui.label(egui::RichText::new(lo.summary()).color(GREEN));
            if m.loadout.bays.iter().any(|b| b.weapon == 16 || b.weapon == 18) {
                ui.label(
                    egui::RichText::new("The mission needs its camera or special equipment")
                        .small()
                        .color(AMBER),
                );
            }
        });
    });
}

/// The aircraft from above with its hardpoints; returns the one clicked.
fn diagram(
    ui: &mut egui::Ui,
    lay: &Layout,
    stores: &[Option<u8>],
    selected: usize,
    size: egui::Vec2,
) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let s = (rect.width() / (2.0 * lay.half_span + 3.0)).min(rect.height() / (lay.length + 2.0));
    let origin = egui::pos2(rect.center().x, rect.center().y - lay.length * s / 2.0);
    let at = |x: f32, y: f32| origin + egui::vec2(x * s, y * s);
    let p = ui.painter_at(rect);
    let outline: Vec<egui::Pos2> = lay.outline.iter().map(|&(x, y)| at(x, y)).collect();
    p.add(egui::Shape::closed_line(outline, egui::Stroke::new(1.5, DIM)));
    let mut clicked = None;
    for (i, hp) in lay.hardpoints.iter().enumerate() {
        let c = at(hp.pos.0, hp.pos.1);
        let r = if hp.bay {
            egui::Rect::from_center_size(c, egui::vec2(0.8 * s, 3.4 * s))
        } else {
            egui::Rect::from_center_size(c, egui::vec2(16.0, 16.0))
        };
        let resp = ui.interact(r.expand(4.0), ui.id().with(("hardpoint", i)), egui::Sense::click());
        if resp.clicked() {
            clicked = Some(i);
        }
        let colour = if i == selected {
            BRIGHT
        } else if resp.hovered() {
            GREEN
        } else if hp.fantasy {
            AMBER
        } else {
            DIM
        };
        let fill = if stores[i].is_some() && !hp.bay {
            DIM.gamma_multiply(0.6)
        } else {
            egui::Color32::TRANSPARENT
        };
        let width = if i == selected { 2.5 } else { 1.5 };
        if hp.bay {
            p.rect(r, 0.0, fill, egui::Stroke::new(width, colour), egui::StrokeKind::Inside);
        } else {
            p.circle(c, 7.0, fill, egui::Stroke::new(width, colour));
        }
        let label_at = if hp.bay {
            r.center_bottom() + egui::vec2(0.0, 10.0)
        } else {
            c - egui::vec2(0.0, 18.0)
        };
        p.text(
            label_at,
            egui::Align2::CENTER_CENTER,
            hp.label,
            egui::FontId::monospace(12.0),
            colour,
        );
        // The store's name turned along the bay, or down from the pylon.
        let Some(w) = stores[i] else { continue };
        let name = WEAPONS[w as usize].0.split(' ').next().unwrap_or("").to_string();
        let galley =
            ui.ctx().fonts_mut(|f| f.layout_no_wrap(name, egui::FontId::monospace(11.0), colour));
        let size = galley.size();
        let quarter = std::f32::consts::FRAC_PI_2;
        let shape = if hp.bay {
            egui::epaint::TextShape::new(r.center() - size / 2.0, galley, colour)
                .with_angle_and_anchor(quarter, egui::Align2::CENTER_CENTER)
        } else {
            let start = c + egui::vec2(0.0, 12.0);
            egui::epaint::TextShape::new(start - egui::vec2(0.0, size.y / 2.0), galley, colour)
                .with_angle_and_anchor(quarter, egui::Align2::LEFT_CENTER)
        };
        p.add(shape);
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission(seed: u16, realistic: bool) -> Mission {
        let data = crate::mission::MissionData::load().expect("mission data");
        crate::mission::generate(
            &data,
            &crate::mission::settings(
                3,
                1,
                stealth_mission::MissionKind::Strike,
                1,
                realistic,
                None,
                None,
            ),
            seed,
        )
        .unwrap()
    }

    #[test]
    fn weapon_tables_match_the_sim_data() {
        let data = crate::mission::MissionData::load().expect("mission data");
        let t = &data.0.tables;
        for (w, &qty) in BAY_QTY.iter().enumerate() {
            assert_eq!(t.weapon_qty[w], qty, "{}", t.weapon_names[w]);
        }
    }

    #[test]
    fn the_f117_flies_the_briefed_bays() {
        for realistic in [false, true] {
            let m = mission(7, realistic);
            let lo = PlannedLoadout::of(&m, AircraftKind::F117);
            assert!(lo.fits(&m, AircraftKind::F117));
            let bays: Vec<(i32, i32)> = m
                .loadout
                .bays
                .iter()
                .take(m.loadout.usable_bays)
                .map(|b| (b.weapon as i32, b.count as i32))
                .collect();
            assert_eq!(lo.stations(), bays);
            assert_eq!(lo.first_choice(&m), 0);
        }
    }

    #[test]
    fn the_f16_carries_the_briefed_stores_in_pairs() {
        for seed in 0..40 {
            let m = mission(seed, false);
            let lo = PlannedLoadout::of(&m, AircraftKind::F16);
            for (i, (h, s)) in F16_STATIONS.iter().zip(&lo.stores).enumerate() {
                if let Some(w) = s {
                    assert!(h.loads.contains(w), "station {} {w}", i + 1);
                    if let Some(j) = h.mirror {
                        assert_eq!(lo.stores[j], *s);
                    }
                }
            }
            let main = m.loadout.bays[0].weapon as u8;
            assert_eq!(lo.stores[lo.first_choice(&m)], Some(main), "seed {seed}");
            assert_eq!(lo.stores[0], Some(0));
        }
    }
}
