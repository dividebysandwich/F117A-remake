//! Modern avionics (not in the original; on unless `--classic-avionics`): F-16 style MFD
//! formats on either MFD instead of the original's fixed pages.
//!
//! - Formats: HSD (horizontal situation display: route, threat rings, air and ground contacts),
//!   HARM (the radars painting the aircraft, placed by danger like the RWR), TGP (the tracking
//!   camera), MAP (the moving map in muted colours, or the waypoint table) and SMS (stores: the
//!   WPN and ORD pages). The bottom bezel buttons pick the format; picking the one the other MFD
//!   shows swaps the two. The top buttons are the format's own, the top right one makes the
//!   format the sensor of interest (SOI). The buttons' functions are drawn on the display next
//!   to them, like the real ones.
//! - SOI: HSD, HARM (and the targeting pod) can be the sensor of interest: the "sensor/cursor
//!   slew" controls move its cursor, "lock cursor target" locks what is under it (an aircraft or
//!   a ground object on the HSD, a radar on the HARM display).

use bevy::math::DVec2;
use bevy::prelude::*;

use crate::canvas::Canvas;
use crate::cockpit::CockpitArt;
use crate::flight::{Aircraft, Player};
use crate::font::{Align, Fonts, stroke_text_at, upper};
use crate::hud::{HudMode, HudState};
use crate::input::{Action, ActionEvent, FlightControls};
use crate::mfd::{ContactKind, ContactRef, Contacts, HIRES, MapRaster, MfdCmd, MfdParams, MfdSide, MfdState, OsbFn};
use crate::rwr::{EmitterView, Emitters};
use crate::world::{WU, WorldPos, bevy_to_wu};

pub struct AvionicsPlugin;

impl Plugin for AvionicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LockRequest>().add_systems(
            Update,
            slew_and_lock
                .after(crate::input::InputSystemsSet)
                .after(crate::flight::FlightSystems)
                .before(crate::combat::CombatSystems)
                .run_if(in_state(crate::settings::AppState::Flight)),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Hsd,
    Harm,
    Tgp,
    Map,
    Sms,
}

/// The formats on the bottom bezel buttons, left to right.
pub const FORMATS: [Format; 5] = [Format::Hsd, Format::Harm, Format::Tgp, Format::Map, Format::Sms];

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Hsd => "HSD",
            Format::Harm => "HARM",
            Format::Tgp => "TGP",
            Format::Map => "MAP",
            Format::Sms => "SMS",
        }
    }

    /// Formats with a cursor (or a pod) the slew controls move.
    pub fn soi_capable(self) -> bool {
        matches!(self, Format::Hsd | Format::Harm)
    }

    /// The key that brings the format up (script `press=` lookups).
    pub fn action(self) -> Action {
        match self {
            Format::Hsd | Format::Harm | Format::Map => Action::Maps,
            Format::Tgp => Action::CamAhead,
            Format::Sms => Action::Ordnance,
        }
    }
}

/// F3 on the left MFD: HSD -> MAP -> HARM -> HSD.
pub fn next_left_format(f: Format) -> Format {
    match f {
        Format::Hsd => Format::Map,
        Format::Map => Format::Harm,
        _ => Format::Hsd,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HarmFilter {
    #[default]
    All,
    Sam,
    Air,
}

/// HSD ranges (km from the aircraft to the outer ring).
pub const HSD_RANGES: [f32; 6] = [10.0, 20.0, 40.0, 80.0, 160.0, 320.0];
const HSD_RANGE_LABELS: [&str; 6] = ["10", "20", "40", "80", "160", "320"];

#[derive(Debug, Clone)]
pub struct AvionicsState {
    /// Format on the left and on the right MFD (never the same).
    pub formats: [Format; 2],
    /// The sensor of interest (a format on one of the MFDs).
    pub soi: Option<Format>,
    pub hsd_range: usize,
    /// HSD with the aircraft in the middle instead of low (depressed).
    pub hsd_centred: bool,
    /// Cursors in display pixels.
    pub hsd_cursor: Vec2,
    pub harm_cursor: Vec2,
    pub harm_filter: HarmFilter,
    /// MAP shows the waypoint table.
    pub map_way: bool,
    /// SMS shows the mission orders (else the weapons).
    pub sms_ord: bool,
    /// Right format before ILS was switched on.
    pub before_ils: Option<Format>,
}

impl Default for AvionicsState {
    fn default() -> Self {
        let centre = Vec2::new(HIRES.0 as f32 / 2.0, HIRES.1 as f32 / 2.0);
        Self {
            formats: [Format::Hsd, Format::Sms],
            soi: Some(Format::Hsd),
            hsd_range: 2,
            hsd_centred: false,
            hsd_cursor: centre - Vec2::Y * 30.0,
            harm_cursor: centre,
            harm_filter: HarmFilter::All,
            map_way: false,
            sms_ord: true,
            before_ils: None,
        }
    }
}

fn index(side: MfdSide) -> usize {
    match side {
        MfdSide::Left => 0,
        MfdSide::Right => 1,
    }
}

fn other(side: MfdSide) -> MfdSide {
    match side {
        MfdSide::Left => MfdSide::Right,
        MfdSide::Right => MfdSide::Left,
    }
}

impl AvionicsState {
    pub fn format(&self, side: MfdSide) -> Format {
        self.formats[index(side)]
    }

    /// The MFD showing a format.
    pub fn shown(&self, f: Format) -> Option<MfdSide> {
        [MfdSide::Left, MfdSide::Right].into_iter().find(|&s| self.format(s) == f)
    }

    /// The sensor of interest if it is on an MFD.
    pub fn soi_format(&self) -> Option<Format> {
        self.soi.filter(|&f| self.shown(f).is_some())
    }

    /// Puts a format on an MFD; the other MFD gets this one's format if it showed the new one.
    pub fn select(&mut self, side: MfdSide, f: Format) {
        let (i, o) = (index(side), index(other(side)));
        if self.formats[o] == f {
            self.formats.swap(i, o);
        } else {
            self.formats[i] = f;
        }
        self.fix_soi();
    }

    /// Shows a format, on the right MFD unless it is up already.
    pub fn show(&mut self, f: Format) {
        if self.shown(f).is_none() {
            self.select(MfdSide::Right, f);
        }
    }

    /// The SOI stays on a displayed format that can be one.
    fn fix_soi(&mut self) {
        if self.soi_format().is_none() {
            self.soi = self.formats.into_iter().find(|f| f.soi_capable());
        }
    }
}

/// Applies a display command with the modern avionics.
pub fn apply(state: &mut MfdState, raster: &mut MapRaster, cmd: MfdCmd) {
    let av = &mut state.av;
    match cmd {
        MfdCmd::Format(side, f) => {
            av.select(side, f);
            if f == Format::Map {
                raster.invalidate();
            }
        }
        MfdCmd::Soi(side) => {
            let f = av.format(side);
            if f.soi_capable() {
                av.soi = Some(f);
            }
        }
        MfdCmd::SwapSoi => {
            let side = av.soi_format().and_then(|f| av.shown(f)).unwrap_or(MfdSide::Right);
            let f = av.format(other(side));
            if f.soi_capable() {
                av.soi = Some(f);
            }
        }
        MfdCmd::HsdRange(d) => av.hsd_range = (av.hsd_range as i32 + d as i32).clamp(0, HSD_RANGES.len() as i32 - 1) as usize,
        MfdCmd::HsdCentre => av.hsd_centred = !av.hsd_centred,
        MfdCmd::MapZoom(d) => {
            state.map_zoom = (state.map_zoom + d as i32).clamp(2, state.max_map_zoom());
            raster.invalidate();
        }
        // Z/X: the HSD's range when it is the SOI, else the map's zoom.
        MfdCmd::ZoomIn | MfdCmd::ZoomOut => {
            let d = if cmd == MfdCmd::ZoomIn { 1 } else { -1 };
            if av.soi_format() == Some(Format::Hsd) || av.shown(Format::Map).is_none() {
                av.hsd_range = (av.hsd_range as i32 - d).clamp(0, HSD_RANGES.len() as i32 - 1) as usize;
            } else {
                state.map_zoom = (state.map_zoom + d).clamp(2, state.max_map_zoom());
                raster.invalidate();
            }
        }
        MfdCmd::ToggleOrient => state.map_heading_up = !state.map_heading_up,
        MfdCmd::MapWay => {
            av.map_way = !av.map_way;
            raster.invalidate();
        }
        MfdCmd::Sms(ord) => {
            av.sms_ord = ord;
            av.show(Format::Sms);
        }
        MfdCmd::HarmFilter => {
            av.harm_filter = match av.harm_filter {
                HarmFilter::All => HarmFilter::Sam,
                HarmFilter::Sam => HarmFilter::Air,
                HarmFilter::Air => HarmFilter::All,
            }
        }
        MfdCmd::Flir => {
            state.flir = !state.flir;
            av.show(Format::Tgp);
        }
        MfdCmd::Cam(d) => {
            state.cam_dir = d;
            av.show(Format::Tgp);
        }
        MfdCmd::Ils => {
            state.ils = !state.ils;
            if state.ils {
                av.before_ils = Some(av.format(MfdSide::Right));
                av.select(MfdSide::Right, Format::Tgp);
            } else if let Some(f) = av.before_ils.take() {
                av.select(MfdSide::Right, f);
            }
        }
        // The classic pages' keys and caps.
        MfdCmd::LeftPage(crate::mfd::LeftMfd::Map) => av.select(MfdSide::Left, Format::Map),
        MfdCmd::LeftPage(crate::mfd::LeftMfd::Tac) => av.select(MfdSide::Left, Format::Hsd),
        MfdCmd::RightPage(p) => match p {
            crate::mfd::RightMfd::Camera => av.show(Format::Tgp),
            crate::mfd::RightMfd::Way => {
                av.map_way = true;
                av.show(Format::Map);
            }
            crate::mfd::RightMfd::Wpn => {
                av.sms_ord = false;
                av.show(Format::Sms);
            }
            crate::mfd::RightMfd::Ord => {
                av.sms_ord = true;
                av.show(Format::Sms);
            }
        },
    }
}

/// Label, function and lit state of a bezel button with the modern avionics: the formats along
/// the bottom, the format's own functions along the top (SOI top right).
pub fn osb(side: MfdSide, slot: u8, s: &MfdState) -> Option<(&'static str, OsbFn, bool)> {
    use crate::input::Action as A;
    use OsbFn::*;
    let av = &s.av;
    let f = av.format(side);
    if slot >= 5 {
        let g = FORMATS[slot as usize - 5];
        return Some((g.label(), Cmd(MfdCmd::Format(side, g)), f == g));
    }
    let soi = ("SOI", Cmd(MfdCmd::Soi(side)), av.soi_format() == Some(f));
    Some(match (f, slot) {
        (Format::Hsd, 0) => (if av.hsd_centred { "CEN" } else { "DEP" }, Cmd(MfdCmd::HsdCentre), false),
        (Format::Hsd, 1) => ("RNG-", Cmd(MfdCmd::HsdRange(-1)), false),
        (Format::Hsd, 2) => (HSD_RANGE_LABELS[av.hsd_range], Readout, false),
        (Format::Hsd, 3) => ("RNG+", Cmd(MfdCmd::HsdRange(1)), false),
        (Format::Harm, 0) => (
            match av.harm_filter {
                HarmFilter::All => "ALL",
                HarmFilter::Sam => "SAM",
                HarmFilter::Air => "AIR",
            },
            Cmd(MfdCmd::HarmFilter),
            false,
        ),
        (Format::Hsd | Format::Harm, 4) => soi,
        (Format::Tgp, 0) => ("FLIR", Cmd(MfdCmd::Flir), s.flir),
        (Format::Tgp, 1..=4) => {
            use crate::mfd::CamDir;
            let (label, dir) = [("AHD", CamDir::Ahead), ("RGT", CamDir::Right), ("REAR", CamDir::Rear), ("LFT", CamDir::Left)][slot as usize - 1];
            (label, Cmd(MfdCmd::Cam(dir)), s.cam_dir == dir)
        }
        (Format::Map, 0) if av.map_way => ("WP-", Action(A::LastWaypoint), false),
        (Format::Map, 1) if av.map_way => ("WP+", Action(A::NextWaypoint), false),
        (Format::Map, 2) if av.map_way => ("RST", Action(A::ResetWaypoint), false),
        (Format::Map, 0) => (if s.map_heading_up { "HUP" } else { "NUP" }, Cmd(MfdCmd::ToggleOrient), false),
        (Format::Map, 1) => ("ZM-", Cmd(MfdCmd::MapZoom(-1)), false),
        (Format::Map, 2) => ("ZM+", Cmd(MfdCmd::MapZoom(1)), false),
        (Format::Map, 3) => ("WAY", Cmd(MfdCmd::MapWay), av.map_way),
        (Format::Sms, 0) => ("WPN", Cmd(MfdCmd::Sms(false)), !av.sms_ord),
        (Format::Sms, 1) => ("ORD", Cmd(MfdCmd::Sms(true)), av.sms_ord),
        (Format::Sms, 2) if !av.sms_ord => ("SEL", Action(A::SelectWeapon), false),
        _ => return None,
    })
}

// ------------------------------------------------------------------------------------------
// Drawing

/// Height of the bezel label bands at the top and bottom of the display.
const BAND: i32 = 16;
/// Colours (palette 0..15, the same at every time of day).
const BLACK: u8 = 5;
const DARK: u8 = 8;
const GREY: u8 = 7;
const NAVY: u8 = 2;
const GREEN: u8 = 10;
const CYAN: u8 = 11;
const RED: u8 = 13;
const WHITE: u8 = 15;

/// Draws an MFD with the modern avionics.
pub fn draw(p: &MfdParams, side: MfdSide, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
    let s = &p.state;
    let av = &s.av;
    let f = av.format(side);
    let ac = p.aircraft.single().ok();
    let soi = av.soi_format() == Some(f);
    match f {
        Format::Hsd => draw_hsd(p, c, ac, soi),
        Format::Harm => draw_harm(p, c, soi),
        _ => {
            // The original pages, between the label bands.
            let mut page = Canvas::new(c.w, c.h - 2 * BAND as usize);
            match f {
                Format::Tgp => p.draw_camera_hires(&mut page, fonts),
                Format::Map if av.map_way => p.draw_way_hires(&mut page, fonts, ac),
                Format::Map => p.draw_map_hires(&mut page, art, fonts, ac, true),
                _ if av.sms_ord => p.draw_ord_hires(&mut page, fonts),
                _ => p.draw_wpn_hires(&mut page, art, fonts),
            }
            // See-through around the pod picture, black elsewhere.
            c.clear(if f == Format::Tgp && p.cam.live { 0 } else { BLACK });
            c.copy_rect(&page, 0, 0, page.w as i32, page.h as i32, 0, BAND);
        }
    }
    draw_labels(c, fonts, side, s);
    if soi {
        c.frame(Vec2::splat(1.0), Vec2::new(c.w as f32 - 2.0, c.h as f32 - 2.0), 2.0, WHITE);
    }
}

/// The bezel button labels next to their buttons: lit ones inverted.
fn draw_labels(c: &mut Canvas, fonts: &Fonts, side: MfdSide, s: &MfdState) {
    for slot in 0..crate::mfd::OSB_COUNT {
        let Some((label, _, lit)) = osb(side, slot, s) else { continue };
        let col = (slot % 5) as i32;
        let text = upper(label);
        let w = 2 * fonts.width(&text) - 2;
        let x = (c.w as i32 * (2 * col + 1)) / 10 - w / 2;
        let y = if slot < 5 { 3 } else { c.h as i32 - 13 };
        if lit {
            c.fill_rect(x - 2, y - 2, w + 4, 14, GREEN);
        }
        fonts.text_scaled(c, x, y, &text, if lit { BLACK } else { GREEN }, 2);
    }
}

/// The SOI's cursor: two short vertical lines.
fn draw_cursor(c: &mut Canvas, p: Vec2) {
    for dx in [-3.0, 3.0] {
        c.stroke(p + Vec2::new(dx, -6.0), p + Vec2::new(dx, 6.0), 1.5, WHITE);
    }
}

/// The HSD's projection: heading up, the aircraft low in the display (or centred).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HsdView {
    /// Aircraft position (wu east, north), heading (radians clockwise from north).
    pub own: DVec2,
    pub heading: f64,
    /// Aircraft on the display and display pixels per world unit.
    pub origin: Vec2,
    pub scale: f64,
    /// Outer ring radius (pixels).
    pub radius: f32,
}

impl HsdView {
    pub fn new(own: DVec2, heading: f64, range_km: f32, centred: bool, size: Vec2) -> Self {
        let (origin, radius) = if centred {
            let o = Vec2::new(size.x / 2.0, size.y / 2.0);
            (o, (size.y / 2.0 - BAND as f32 - 6.0).min(size.x / 2.0 - 6.0))
        } else {
            let o = Vec2::new(size.x / 2.0, size.y * 0.72);
            (o, o.y - BAND as f32 - 6.0)
        };
        let scale = radius as f64 / (range_km as f64 * 1000.0 / WU);
        Self { own, heading, origin, scale, radius }
    }

    pub fn to_screen(self, east: f64, north: f64) -> Vec2 {
        let (de, dn) = (east - self.own.x, north - self.own.y);
        let (s, c) = self.heading.sin_cos();
        let right = de * c - dn * s;
        let ahead = de * s + dn * c;
        self.origin + Vec2::new((right * self.scale) as f32, (-ahead * self.scale) as f32)
    }

    #[cfg(test)]
    pub fn to_world(self, p: Vec2) -> DVec2 {
        let d = (p - self.origin).as_dvec2() / self.scale;
        let (right, ahead) = (d.x, -d.y);
        let (s, c) = self.heading.sin_cos();
        self.own + DVec2::new(right * c + ahead * s, -right * s + ahead * c)
    }

    /// Direction on the display of a heading (radians clockwise from north).
    pub fn dir(self, h: f64) -> Vec2 {
        let a = h - self.heading;
        Vec2::new(a.sin() as f32, -a.cos() as f32)
    }
}

/// The HSD view for the aircraft (None without one).
pub fn hsd_view(av: &AvionicsState, ac: Option<(&Aircraft, &WorldPos)>) -> Option<HsdView> {
    let (a, pos) = ac?;
    let (e, n, _) = bevy_to_wu(pos.0);
    let size = Vec2::new(HIRES.0 as f32, HIRES.1 as f32);
    Some(HsdView::new(DVec2::new(e, n), a.euler().0 as f64, HSD_RANGES[av.hsd_range], av.hsd_centred, size))
}

/// The HARM display's scope: centre and radius.
fn harm_scope() -> (Vec2, f32) {
    let (w, h) = (HIRES.0 as f32, HIRES.1 as f32);
    (Vec2::new(w / 2.0, h / 2.0), h / 2.0 - BAND as f32 - 4.0)
}

fn draw_hsd(p: &MfdParams, c: &mut Canvas, ac: Option<(&Aircraft, &WorldPos)>, soi: bool) {
    c.clear(BLACK);
    let av = &p.state.av;
    let Some(view) = hsd_view(av, ac) else { return };
    let o = view.origin;
    // Range rings (the inner ones dashed) and the compass rose on the outer one.
    for k in 1..3 {
        let r = view.radius * k as f32 / 3.0;
        let steps = 36;
        for i in (0..steps).step_by(2) {
            let a0 = i as f32 / steps as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / steps as f32 * std::f32::consts::TAU;
            c.arc(o, r, a0, a1, 1.0, DARK);
        }
    }
    c.ring(o, view.radius, 1.0, GREY);
    for k in 0..36 {
        let h = (k as f64 * 10.0).to_radians();
        let d = view.dir(h);
        let len = if k % 3 == 0 { 7.0 } else { 4.0 };
        c.stroke(o + d * view.radius, o + d * (view.radius - len), 1.0, GREY);
        if k % 9 == 0 {
            let label = ["N", "E", "S", "W"][k / 9];
            let q = o + d * (view.radius - 14.0);
            stroke_text_at(c, q.x, q.y, label, 9.0, 1.2, GREY, Align::Centre);
        }
    }
    // Route: the steerpoints as small squares, the current one ringed.
    let wps: Vec<Vec2> = p.waypoints.points.iter().map(|w| view.to_screen(w.east, w.north)).collect();
    for pair in wps.windows(2) {
        c.stroke(pair[0], pair[1], 1.0, WHITE);
    }
    for (i, &q) in wps.iter().enumerate() {
        c.frame(q - Vec2::splat(2.5), q + Vec2::splat(2.5), 1.0, WHITE);
        if i == p.waypoints.current {
            c.ring(q, 6.0, 1.2, WHITE);
        }
    }
    // Threat rings, ground targets, aircraft, missiles.
    let px_per_km = (view.scale * 1000.0 / WU) as f32;
    for k in &p.contacts.list {
        let q = view.to_screen(k.east, k.north);
        match k.kind {
            ContactKind::GroundRadar => {
                if let Some((km, label)) = k.threat {
                    c.ring(q, km * px_per_km, 1.0, RED);
                    let text = String::from_utf8_lossy(&label).trim_end_matches('\0').to_string();
                    stroke_text_at(c, q.x, q.y, &text, 9.0, 1.3, RED, Align::Centre);
                } else {
                    c.fill_rect(q.x as i32 - 2, q.y as i32 - 2, 4, 4, RED);
                }
            }
            ContactKind::Target => {
                c.polyline(&[q + Vec2::new(0.0, -5.0), q + Vec2::new(5.0, 0.0), q + Vec2::new(0.0, 5.0), q + Vec2::new(-5.0, 0.0), q + Vec2::new(0.0, -5.0)], 1.3, CYAN);
            }
            ContactKind::Aircraft => {
                let d = view.dir(k.heading as f64 / 65536.0 * std::f64::consts::TAU);
                if k.friendly {
                    c.ring(q, 4.0, 1.3, GREEN);
                    c.stroke(q + d * 4.0, q + d * 9.0, 1.3, GREEN);
                } else {
                    let side = Vec2::new(-d.y, d.x);
                    c.polyline(&[q + d * 5.0, q - d * 3.0 + side * 4.0, q - d * 3.0 - side * 4.0, q + d * 5.0], 1.3, RED);
                    c.stroke(q + d * 5.0, q + d * 10.0, 1.3, RED);
                }
            }
            ContactKind::Drone => {
                c.stroke(q - Vec2::splat(3.0), q + Vec2::splat(3.0), 1.2, RED);
                c.stroke(q + Vec2::new(-3.0, 3.0), q + Vec2::new(3.0, -3.0), 1.2, RED);
            }
            ContactKind::Missile => {
                let d = view.dir(k.heading as f64 / 65536.0 * std::f64::consts::TAU);
                c.stroke(q - d * 3.0, q + d * 3.0, 1.5, RED);
            }
        }
        if k.target.is_some() && k.target == p.contacts.locked {
            c.frame(q - Vec2::splat(8.0), q + Vec2::splat(8.0), 1.3, WHITE);
        }
    }
    // Own aircraft.
    c.stroke(o + Vec2::new(0.0, -6.0), o + Vec2::new(0.0, 6.0), 1.6, CYAN);
    c.stroke(o + Vec2::new(-6.0, -1.0), o + Vec2::new(6.0, -1.0), 1.6, CYAN);
    c.stroke(o + Vec2::new(-3.0, 5.0), o + Vec2::new(3.0, 5.0), 1.6, CYAN);
    if soi {
        draw_cursor(c, av.hsd_cursor);
    }
}

/// HARM display: the radars painting the aircraft (filtered), placed by danger like the RWR.
fn draw_harm(p: &MfdParams, c: &mut Canvas, soi: bool) {
    c.clear(BLACK);
    let av = &p.state.av;
    let (centre, r) = harm_scope();
    c.ring(centre, r, 1.0, GREY);
    for f in [0.33, 0.66] {
        c.ring(centre, r * f, 1.0, DARK);
    }
    for k in 0..12 {
        let a = k as f32 * std::f32::consts::TAU / 12.0;
        let d = Vec2::new(a.sin(), -a.cos());
        c.stroke(centre + d * r, centre + d * (r - 5.0), 1.0, GREY);
    }
    c.stroke(centre - Vec2::X * 4.0, centre + Vec2::X * 4.0, 1.0, GREY);
    c.stroke(centre - Vec2::Y * 4.0, centre + Vec2::Y * 4.0, 1.0, GREY);
    let locked = p.contacts.locked.map(emitter_source);
    let filter = av.harm_filter;
    crate::rwr::draw_symbols(c, &p.emitters.list, centre, r, crate::hud::blink(&p.time), locked, |e| harm_shows(filter, e));
    if p.emitters.list.iter().all(|e| !harm_shows(filter, e)) {
        stroke_text_at(c, centre.x, centre.y + r * 0.5, "NO EMITTERS", 8.0, 1.0, NAVY.max(DARK), Align::Centre);
    }
    if soi {
        draw_cursor(c, av.harm_cursor);
    }
}

fn harm_shows(filter: HarmFilter, e: &EmitterView) -> bool {
    match filter {
        HarmFilter::All => true,
        HarmFilter::Sam => !e.air,
        HarmFilter::Air => e.air,
    }
}

fn emitter_source(r: ContactRef) -> stealth_sim::EmitterSource {
    match r {
        ContactRef::Unit(i) => stealth_sim::EmitterSource::Aircraft(i),
        ContactRef::Object(i) => stealth_sim::EmitterSource::Site(i),
    }
}

// ------------------------------------------------------------------------------------------
// Cursor slew and lock

/// A lock on a simulation object picked on a display (`combat` passes it to the simulation).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockRequest(pub ContactRef);

/// Cursor speed at full slew (display pixels per second).
const SLEW_SPEED: f32 = 110.0;
/// A lock picks the nearest symbol within this many pixels of the cursor.
const PICK_RADIUS: f32 = 14.0;

/// Moves the SOI's cursor with the slew controls; "lock cursor target" locks the nearest
/// symbol under it, switching the HUD to the matching mode.
#[allow(clippy::too_many_arguments)]
fn slew_and_lock(
    time: Res<Time>,
    controls: Res<FlightControls>,
    mut actions: MessageReader<ActionEvent>,
    mut state: ResMut<MfdState>,
    contacts: Res<Contacts>,
    emitters: Res<Emitters>,
    mut hud: ResMut<HudState>,
    aircraft: Query<(&Aircraft, &WorldPos), With<Player>>,
    mut locks: MessageWriter<LockRequest>,
) {
    let lock = actions.read().any(|a| a.0 == Action::LockCursor);
    if !state.modern {
        return;
    }
    let Some(f) = state.av.soi_format() else { return };
    let size = Vec2::new(HIRES.0 as f32, HIRES.1 as f32);
    let lo = Vec2::new(4.0, BAND as f32 + 4.0);
    let hi = size - lo;
    let delta = Vec2::new(controls.slew.x, -controls.slew.y) * SLEW_SPEED * time.delta_secs();
    let av = &mut state.av;
    let cursor = match f {
        Format::Hsd => &mut av.hsd_cursor,
        Format::Harm => &mut av.harm_cursor,
        _ => return,
    };
    if delta != Vec2::ZERO {
        *cursor = (*cursor + delta).clamp(lo, hi);
    }
    if !lock {
        return;
    }
    let cursor = *cursor;
    let picked = match f {
        Format::Hsd => hsd_view(av, aircraft.single().ok()).and_then(|view| pick_contact(&contacts, &view, cursor)),
        _ => {
            let (centre, r) = harm_scope();
            let filter = av.harm_filter;
            nearest(emitters.list.iter().filter(|e| harm_shows(filter, e)).map(|e| (crate::rwr::scope_point(e, centre, r), emitter_ref(e))), cursor)
        }
    };
    if let Some(target) = picked {
        hud.mode = match target {
            ContactRef::Unit(_) => HudMode::Air,
            ContactRef::Object(_) => HudMode::Gnd,
        };
        locks.write(LockRequest(target));
    }
}

fn emitter_ref(e: &EmitterView) -> ContactRef {
    match e.source {
        stealth_sim::EmitterSource::Site(i) => ContactRef::Object(i),
        stealth_sim::EmitterSource::Aircraft(i) => ContactRef::Unit(i),
    }
}

/// The lockable contact nearest the cursor on the HSD.
pub fn pick_contact(contacts: &Contacts, view: &HsdView, cursor: Vec2) -> Option<ContactRef> {
    nearest(contacts.list.iter().filter_map(|k| Some((view.to_screen(k.east, k.north), k.target?))), cursor)
}

fn nearest(candidates: impl Iterator<Item = (Vec2, ContactRef)>, cursor: Vec2) -> Option<ContactRef> {
    candidates
        .map(|(q, t)| (q.distance(cursor), t))
        .filter(|(d, _)| *d <= PICK_RADIUS)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfd::Contact;

    #[test]
    fn selecting_the_other_mfds_format_swaps() {
        let mut av = AvionicsState::default();
        assert_eq!(av.formats, [Format::Hsd, Format::Sms]);
        av.select(MfdSide::Right, Format::Hsd);
        assert_eq!(av.formats, [Format::Sms, Format::Hsd]);
        av.select(MfdSide::Left, Format::Harm);
        assert_eq!(av.formats, [Format::Harm, Format::Hsd]);
        // The SOI stays with its format.
        assert_eq!(av.soi_format(), Some(Format::Hsd));
        av.select(MfdSide::Right, Format::Map);
        assert_eq!(av.soi_format(), Some(Format::Harm), "SOI moves to the remaining capable format");
        av.select(MfdSide::Left, Format::Tgp);
        assert_eq!(av.soi_format(), None);
    }

    #[test]
    fn soi_commands() {
        let mut s = MfdState { modern: true, ..default() };
        let mut raster = MapRaster::default();
        s.av.select(MfdSide::Right, Format::Harm);
        apply(&mut s, &mut raster, MfdCmd::Soi(MfdSide::Right));
        assert_eq!(s.av.soi_format(), Some(Format::Harm));
        apply(&mut s, &mut raster, MfdCmd::SwapSoi);
        assert_eq!(s.av.soi_format(), Some(Format::Hsd));
        // Not on a format without a cursor.
        s.av.select(MfdSide::Right, Format::Sms);
        apply(&mut s, &mut raster, MfdCmd::Soi(MfdSide::Right));
        assert_eq!(s.av.soi_format(), Some(Format::Hsd));
    }

    #[test]
    fn every_button_on_every_format() {
        for f in FORMATS {
            for (way, ord) in [(false, false), (true, true)] {
                let mut s = MfdState { modern: true, ..default() };
                s.av.select(MfdSide::Left, f);
                s.av.map_way = way;
                s.av.sms_ord = ord;
                for slot in 0..crate::mfd::OSB_COUNT {
                    if let Some((label, _, _)) = osb(MfdSide::Left, slot, &s) {
                        assert!(label.len() <= 5, "{f:?} {slot}: {label}");
                    }
                }
                assert!(osb(MfdSide::Left, 5, &s).is_some());
            }
        }
    }

    #[test]
    fn hsd_projection_round_trips() {
        let v = HsdView::new(DVec2::new(1000.0, 2000.0), 0.7, 40.0, false, Vec2::new(231.0, 180.0));
        let ahead = DVec2::new(1000.0 + 0.7f64.sin() * 5000.0, 2000.0 + 0.7f64.cos() * 5000.0);
        let p = v.to_screen(ahead.x, ahead.y);
        assert!((p.x - v.origin.x).abs() < 1e-3 && p.y < v.origin.y, "ahead is up: {p}");
        let back = v.to_world(Vec2::new(150.0, 40.0));
        assert!((v.to_screen(back.x, back.y) - Vec2::new(150.0, 40.0)).length() < 1e-3);
    }

    #[test]
    fn lock_picks_the_nearest_lockable() {
        let v = HsdView::new(DVec2::ZERO, 0.0, 20.0, true, Vec2::new(231.0, 180.0));
        let at = |e: f64, n: f64, target| Contact { kind: ContactKind::Aircraft, east: e, north: n, alt: 0.0, heading: 0, friendly: false, target, threat: None };
        let contacts = Contacts {
            list: vec![at(0.0, 1000.0, Some(ContactRef::Unit(3))), at(0.0, 1200.0, Some(ContactRef::Object(7))), at(0.0, 1050.0, None)],
            ..default()
        };
        let p = v.to_screen(0.0, 1040.0);
        assert_eq!(pick_contact(&contacts, &v, p), Some(ContactRef::Unit(3)));
        assert_eq!(pick_contact(&contacts, &v, p + Vec2::X * 40.0), None);
    }
}
