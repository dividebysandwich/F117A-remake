//! The air picture the friendly side has, for the war map and the briefing map: its own
//! aircraft, the enemy fighters its radars see, and the enemy air defences it knows of (from
//! recon, or because they gave themselves away by painting the player or firing).

use bevy_egui::egui;
use stealth_campaign::{CampaignState, Side, SiteKind};
use stealth_sim::aaa::AaaKind;
use stealth_sim::{AircraftKind, Sim, fflags, oflags};

use crate::map_draw::{self, FRIENDLY_BLUE, MISSILE_RED, MapView};
use crate::menu::{BRIGHT, GREEN};

/// Radar cover: friendly places (ground radars), friendly aircraft, the player (map units).
const GROUND_RADAR: i32 = 0x1400;
const AIR_RADAR: i32 = 0xA00;
const PLAYER_RADAR: i32 = 0x1400;
/// Guns this close to a reconned place are known (6 km).
const RECON_GUNS: i32 = 0x180;

pub struct Aircraft {
    /// Flight unit and its type (the unit view checks the slot still holds it).
    pub unit: usize,
    pub spec: i32,
    pub x: i32,
    pub y: i32,
    pub heading: i32,
    pub helicopter: bool,
}

pub struct SamSite {
    pub object: usize,
    pub x: i32,
    pub y: i32,
    pub name: String,
    /// Missile reach (map units).
    pub reach: f32,
    pub reconned: bool,
}

pub struct Gun {
    /// AAA unit index.
    pub index: usize,
    pub x: i32,
    pub y: i32,
    pub kind: AaaKind,
}

#[derive(Default)]
pub struct Picture {
    pub friendly: Vec<Aircraft>,
    pub fighters: Vec<Aircraft>,
    pub sams: Vec<SamSite>,
    pub guns: Vec<Gun>,
    /// The player in the air.
    pub player: Option<(i32, i32)>,
}

fn near(ax: i32, ay: i32, bx: i32, by: i32, r: i32) -> bool {
    stealth_campaign::dist(ax - bx, ay - by) < r
}

/// What the friendly side knows now.
pub fn picture(state: &CampaignState, sim: &Sim) -> Picture {
    let mut pic = Picture::default();
    let mut eyes: Vec<(i32, i32, i32)> = state
        .places
        .iter()
        .filter(|p| p.owner == Side::Blue)
        .map(|p| (p.x, p.y, GROUND_RADAR))
        .collect();
    if !sim.player_absent() {
        let p = sim.player();
        pic.player = Some((p.mx, p.my));
        eyes.push((p.mx, p.my, PLAYER_RADAR));
    }
    let mut enemies = Vec::new();
    for (i, u) in sim.flights().iter().enumerate() {
        if u.flags & (fflags::EXISTS | fflags::AIRBORNE) != fflags::EXISTS | fflags::AIRBORNE
            || u.flags & fflags::DESTROYED != 0
            || u.speed == 0
        {
            continue;
        }
        let blue = sim.friendly_unit(i, u.spec);
        let a = Aircraft {
            unit: i,
            spec: u.spec,
            x: u.x,
            y: u.y,
            heading: u.heading,
            helicopter: sim.aircraft_kind(u.spec) == AircraftKind::Helicopter,
        };
        if blue {
            eyes.push((u.x, u.y, AIR_RADAR));
            pic.friendly.push(a);
        } else {
            enemies.push(a);
        }
    }
    pic.fighters = enemies
        .into_iter()
        .filter(|a| eyes.iter().any(|&(x, y, r)| near(a.x, a.y, x, y, r)))
        .collect();
    // SAM sites: reconned, or once detected by a friendly aircraft (`CampaignState` spotted).
    let objects = sim.objects();
    for s in state.sites.iter().filter(|s| s.kind == SiteKind::Sam && s.alive()) {
        let i = s.object as usize;
        let Some(o) = objects.get(i) else { continue };
        if o.unit_type == 0 || o.flags & (oflags::FRIENDLY | oflags::NEUTRAL) != 0 {
            continue;
        }
        let reconned = sim.has_intel(i);
        if reconned || s.spotted || sim.site_engaging(i) {
            pic.sams.push(SamSite {
                object: i,
                x: o.x,
                y: o.y,
                name: sim.threat_name(o.unit_type).to_string(),
                reach: (sim.site_lock_range(o.unit_type) * 64) as f32,
                reconned,
            });
        }
    }
    // Guns and MANPADS: around reconned places, or engaging.
    let reconned: Vec<(i32, i32)> =
        state.places.iter().filter(|p| p.intel(state.clock)).map(|p| (p.x, p.y)).collect();
    for (k, u) in sim.aaa_units().iter().enumerate() {
        if u.destroyed || sim.aaa_blue(k) {
            continue;
        }
        if sim.aaa_engaging(k) || reconned.iter().any(|&(x, y)| near(u.x, u.y, x, y, RECON_GUNS)) {
            pic.guns.push(Gun { index: k, x: u.x, y: u.y, kind: u.kind });
        }
    }
    pic
}

/// An aircraft symbol: a dot for a helicopter, an arrowhead along the heading for a plane.
fn aircraft(painter: &egui::Painter, p: egui::Pos2, a: &Aircraft, col: egui::Color32) {
    if a.helicopter {
        painter.circle_filled(p, 3.0, col);
        painter.circle_stroke(p, 5.0, egui::Stroke::new(1.0, col));
        return;
    }
    let h = a.heading as f32 / 65536.0 * std::f32::consts::TAU;
    let fwd = egui::vec2(h.sin(), -h.cos());
    let side = egui::vec2(fwd.y, -fwd.x);
    let pts = vec![p + fwd * 6.0, p - fwd * 4.0 + side * 4.0, p - fwd * 4.0 - side * 4.0];
    painter.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
}

/// A gun: a small cross (a ring for MANPADS).
fn gun(painter: &egui::Painter, p: egui::Pos2, kind: AaaKind) {
    let stroke = egui::Stroke::new(1.5, MISSILE_RED);
    if kind == AaaKind::Manpads {
        painter.circle_stroke(p, 2.5, stroke);
    } else {
        painter.line_segment([p + egui::vec2(-3.0, -3.0), p + egui::vec2(3.0, 3.0)], stroke);
        painter.line_segment([p + egui::vec2(-3.0, 3.0), p + egui::vec2(3.0, -3.0)], stroke);
    }
}

/// Draws the picture: known defences (with their missile reach), enemy fighters, friendly
/// aircraft, the player.
pub fn draw(painter: &egui::Painter, view: &MapView, pic: &Picture, rings: bool) {
    let at = |x: i32, y: i32| view.to_screen(x as f32, y as f32);
    for s in &pic.sams {
        let c = at(s.x, s.y);
        if rings {
            painter.extend(map_draw::missile_circle(c, s.reach / view.scale, 0.7));
        }
        map_draw::site(painter, view, c, &s.name, 1.0);
        if s.reconned {
            painter.circle_stroke(c, 6.0, egui::Stroke::new(1.0, BRIGHT));
        }
    }
    for g in &pic.guns {
        gun(painter, at(g.x, g.y), g.kind);
    }
    for a in &pic.fighters {
        aircraft(painter, at(a.x, a.y), a, MISSILE_RED);
    }
    for a in &pic.friendly {
        aircraft(painter, at(a.x, a.y), a, FRIENDLY_BLUE);
    }
    if let Some((x, y)) = pic.player {
        painter.circle_stroke(at(x, y), 6.0, egui::Stroke::new(2.0, GREEN));
    }
}

/// The legend lines of the picture.
pub fn legend(ui: &mut egui::Ui) {
    map_draw::legend(ui, "Friendly aircraft", |p, c0| {
        let a = Aircraft { unit: 0, spec: 0, x: 0, y: 0, heading: 0, helicopter: false };
        aircraft(p, c0, &a, FRIENDLY_BLUE);
    });
    map_draw::legend(ui, "Friendly helicopter", |p, c0| {
        let a = Aircraft { unit: 0, spec: 0, x: 0, y: 0, heading: 0, helicopter: true };
        aircraft(p, c0, &a, FRIENDLY_BLUE);
    });
    map_draw::legend(ui, "Enemy fighter (on radar)", |p, c0| {
        let a = Aircraft { unit: 0, spec: 0, x: 0, y: 0, heading: 0, helicopter: false };
        aircraft(p, c0, &a, MISSILE_RED);
    });
    map_draw::legend(ui, "SAM site (known)", |p, c0| {
        p.circle_filled(c0, 3.0, MISSILE_RED);
    });
    map_draw::legend(ui, "Guns / MANPADS (known)", |p, c0| {
        gun(p, c0 - egui::vec2(4.0, 0.0), AaaKind::Zsu);
        gun(p, c0 + egui::vec2(5.0, 0.0), AaaKind::Manpads);
    });
    map_draw::legend(ui, "Reconned", |p, c0| {
        p.circle_stroke(c0, 6.0, egui::Stroke::new(1.0, BRIGHT));
    });
    map_draw::legend(ui, "You", |p, c0| {
        p.circle_stroke(c0, 5.0, egui::Stroke::new(2.0, GREEN));
    });
}

/// The unit nearest a screen point on the map (within a few points), and its name.
pub fn pick(
    view: &MapView,
    pic: &Picture,
    sim: &Sim,
    at: egui::Pos2,
) -> Option<(super::unit_view::UnitRef, String)> {
    use super::unit_view::UnitRef;
    let screen = |x: i32, y: i32| view.to_screen(x as f32, y as f32);
    let mut best: Option<(f32, UnitRef, String)> = None;
    let mut consider = |p: egui::Pos2, r: UnitRef, name: String| {
        let d = p.distance(at);
        if d < 12.0 && best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, r, name));
        }
    };
    for a in pic.friendly.iter().chain(&pic.fighters) {
        consider(
            screen(a.x, a.y),
            UnitRef::Flight { index: a.unit, spec: a.spec },
            sim.aircraft_name(a.spec),
        );
    }
    for s in &pic.sams {
        consider(screen(s.x, s.y), UnitRef::Site(s.object), format!("{} site", s.name));
    }
    for g in &pic.guns {
        consider(screen(g.x, g.y), UnitRef::Aaa(g.index), g.kind.name().to_string());
    }
    best.map(|b| (b.1, b.2))
}
