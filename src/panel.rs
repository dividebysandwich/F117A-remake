//! Instrument panel state and faces: indicator lamps, LED digit readouts, EMV/EGR/EAR gauge,
//! attitude ball and the approach indexer. The faces are drawn into canvas slots that the
//! modelled panel (`cockpit3d`) shows on its lamp caps and instrument windows.
//! docs/re/cockpit.md section 3 ("Lights", "LED digit readouts", "EMV / EGR / EAR gauge",
//! "ADI") and VGAME `1A18:D83E`, `1A18:D92D`, `1A18:8316`, `1A18:17A3`, `1A18:D9A3`.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::canvas::Canvas;
use crate::cockpit::CockpitArt;
use crate::flight::{Aircraft, Player};
use crate::font::{Fonts, upper};
use crate::hud::{HudMode, HudState};
use crate::mfd::{CamDir, LeftMfd, MfdState, RightMfd};

pub struct PanelPlugin;

impl Plugin for PanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Emv>()
            .init_resource::<Countermeasures>()
            .init_resource::<Warnings>()
            .init_resource::<Approach>()
            .add_systems(OnEnter(crate::settings::AppState::Flight), reset_panel);
    }
}

/// Electromagnetic visibility and threat signals for the EMV/EGR/EAR gauge, 0..1
/// (driven by the combat simulation).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Emv {
    /// Own emission level (`DS:3D5C`/100).
    pub emv: f32,
    /// Ground radar signal (EGR).
    pub egr: f32,
    /// Airborne radar signal (EAR).
    pub ear: f32,
}

/// Countermeasure stores and jammer switches (mirrored from the combat simulation, which
/// handles the keys 1..5).
#[derive(Resource, Debug, Clone)]
pub struct Countermeasures {
    pub chaff: u32,
    pub flares: u32,
    pub decoys: u32,
    pub ecm: bool,
    pub ir_jammer: bool,
    /// Seconds the "released" lights stay lit (chaff, flare, decoy).
    pub active: [f32; 3],
}

impl Default for Countermeasures {
    fn default() -> Self {
        Self { chaff: 18, flares: 12, decoys: 3, ecm: false, ir_jammer: false, active: [0.0; 3] }
    }
}

/// Warning and damage lights (set by the combat simulation).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Warnings {
    /// RAD / IR: radar or IR missile in flight; TRAK: tracked by radar; LOCK: weapon lock.
    pub radar_missile: bool,
    pub ir_missile: bool,
    pub tracked: bool,
    pub lock: bool,
    /// MW (missile warning) and damage lights ENG, FC, JAM, FIRE, FUEL, BD, AV.
    pub missile_warning: bool,
    pub engine: bool,
    pub fire_control: bool,
    pub jammer: bool,
    pub fire: bool,
    pub fuel: bool,
    pub bay_doors: bool,
    pub avionics: bool,
    /// EMV warning light (on for 2 s after a detection).
    pub emv_warning: bool,
}

/// Approach indexer state (`DS:46C2`): 0 off, 1 top chevron, 2 circle, 3 bottom chevron.
/// STUB: set by the ILS / landing guidance.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Approach(pub u8);

fn reset_panel(mut cm: ResMut<Countermeasures>, mut emv: ResMut<Emv>, mut w: ResMut<Warnings>, mut ap: ResMut<Approach>) {
    *cm = Countermeasures::default();
    *emv = Emv::default();
    *w = Warnings::default();
    *ap = Approach::default();
}

/// Panel lamps (the original's lights, table `DS:46D8`). The index is the original light number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LampId {
    Nav,
    Air,
    Gnd,
    Lock,
    Stall,
    Mw,
    Eng,
    Fc,
    Jam,
    Fire,
    Fuel,
    Bd,
    Av,
    Blank,
    ChaffAct,
    FlareAct,
    DecoyAct,
    Ecm,
    Irj,
    Rad,
    Ir,
    Trak,
    Map,
    Tac,
    EmvWarn,
    Flir,
    Wpn,
    CamDir,
    Way,
    Ord,
    Tc,
    Bay,
    Auto,
    Flaps,
    Gear,
    Brake,
    LowFuel,
}

pub const LAMP_COUNT: usize = 37;

/// Lamp colours, (text and border, fill) unlit and lit: the original's colour sets (`DS:4672`)
/// as they recolour the panel art.
#[derive(Debug, Clone, Copy)]
pub struct LampStyle {
    pub off: (u8, u8),
    pub on: (u8, u8),
}

const GREEN: LampStyle = LampStyle { off: (45, 44), on: (47, 46) };
const EMV: LampStyle = LampStyle { off: (38, 55), on: (41, 42) };
const RED: LampStyle = LampStyle { off: (54, 55), on: (41, 42) };
const DARK_RED: LampStyle = LampStyle { off: (16, 43), on: (41, 42) };
const AMBER: LampStyle = LampStyle { off: (16, 43), on: (40, 39) };
/// Button caps of the modern panel: green text on dark green unlit, dark text on bright green
/// lit (like the lit MFD bezel buttons).
pub const BUTTON: LampStyle = LampStyle { off: (47, 44), on: (38, 47) };

/// Label and colours of every lamp, indexed by `LampId`.
pub const LAMPS: [(LampId, &str, LampStyle); LAMP_COUNT] = {
    use LampId::*;
    [
        (Nav, "NAV", GREEN),
        (Air, "AIR", GREEN),
        (Gnd, "GND", GREEN),
        (Lock, "LOCK", RED),
        (Stall, "STALL", DARK_RED),
        (Mw, "MW", AMBER),
        (Eng, "ENG", RED),
        (Fc, "FC", RED),
        (Jam, "JAM", RED),
        (Fire, "FIRE", RED),
        (Fuel, "FUEL", RED),
        (Bd, "BD", RED),
        (Av, "AV", RED),
        (Blank, "", RED),
        (ChaffAct, "", RED),
        (FlareAct, "", RED),
        (DecoyAct, "", RED),
        (Ecm, "ECM", GREEN),
        (Irj, "IRJ", GREEN),
        (Rad, "RAD", RED),
        (Ir, "IR", RED),
        (Trak, "TRAK", RED),
        (Map, "MAP", GREEN),
        (Tac, "TAC", GREEN),
        (EmvWarn, "", EMV),
        (Flir, "FLIR", RED),
        (Wpn, "WPN", GREEN),
        (CamDir, "", GREEN),
        (Way, "WAY", GREEN),
        (Ord, "ORD", GREEN),
        (Tc, "TC", GREEN),
        (Bay, "BAY", DARK_RED),
        (Auto, "AUTO", RED),
        (Flaps, "FLAPS", AMBER),
        (Gear, "GEAR", RED),
        (Brake, "BRAKE", RED),
        (LowFuel, "", RED),
    ]
};

/// LED digit readouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedId {
    Chaff,
    Flares,
    Decoys,
    /// Fuel / 10.
    Fuel,
    /// Thrust %.
    Thrust,
}

/// Draws a lamp face into the rectangle (x, y, w, h) of a canvas at `k` canvas pixels per
/// panel pixel: fill, a one-pixel border and the centred label in the text colour at up to
/// `text` canvas pixels per font pixel.
pub fn draw_lamp(c: &mut Canvas, fonts: &Fonts, r: [i32; 4], label: &str, colours: (u8, u8), k: i32, text: i32) {
    let (fg, bg) = colours;
    let [x, y, w, h] = r;
    c.fill_rect(x, y, w, h, bg);
    c.fill_rect(x, y, w, k, fg);
    c.fill_rect(x, y + h - k, w, k, fg);
    c.fill_rect(x, y, k, h, fg);
    c.fill_rect(x + w - k, y, k, h, fg);
    text_in(c, fonts, [x + k, y + k, w - 2 * k, h - 2 * k], label, fg, text);
}

/// Draws a label face: fill and centred text (lines separated by '\n').
pub fn draw_label(c: &mut Canvas, fonts: &Fonts, r: [i32; 4], label: &str, fg: u8, bg: u8, k: i32) {
    c.fill_rect(r[0], r[1], r[2], r[3], bg);
    text_in(c, fonts, r, label, fg, k);
}

/// Centred text at up to `k` times the font size, smaller if it does not fit with a margin.
fn text_in(c: &mut Canvas, fonts: &Fonts, r: [i32; 4], label: &str, fg: u8, k: i32) {
    let lines: Vec<Vec<u8>> = label.split('\n').filter(|l| !l.is_empty()).map(upper).collect();
    if lines.is_empty() {
        return;
    }
    let n = lines.len() as i32;
    let widest = lines.iter().map(|l| fonts.width(l) - 1).max().unwrap_or(0).max(1);
    let line_h = fonts.height() + 1;
    // At least one (scaled) pixel of space around the text.
    let mut s = k;
    while s > 1 && (widest * s > r[2] - 2 * s || (n * line_h - 1) * s > r[3] - 2 * s) {
        s -= 1;
    }
    let total = (n * line_h - 1) * s;
    let mut y = r[1] + (r[3] - total + 1) / 2;
    for l in &lines {
        let w = (fonts.width(l) - 1) * s;
        fonts.text_scaled(c, r[0] + (r[2] - w + 1) / 2, y, l, fg, s);
        y += line_h * s;
    }
}

/// LED readout (`1A18:D92D`: atlas digits (67+5d, 31) 4x7, 5 px advance; non-digits show "0"),
/// centred in the rectangle at up to `k` canvas pixels per panel pixel (smaller if it does not
/// fit), over unlit "8" segments.
pub fn draw_led(c: &mut Canvas, sprites: &Canvas, r: [i32; 4], s: &str, k: i32) {
    c.fill_rect(r[0], r[1], r[2], r[3], 38);
    let n = s.len() as i32;
    let k = (1..=k).rev().find(|&k| (5 * n - 1) * k <= r[2] && 7 * k <= r[3]).unwrap_or(1);
    let x0 = r[0] + (r[2] - (5 * n - 1) * k) / 2;
    let y0 = r[1] + (r[3] - 7 * k) / 2;
    for (i, ch) in s.bytes().enumerate() {
        let d = if ch.is_ascii_digit() { (ch - b'0') as i32 } else { 0 };
        let x = x0 + 5 * k * i as i32;
        c.blit_scaled(sprites, 67 + 5 * 8, 31, 4, 7, x, y0, k, Some(0), Some(44));
        c.blit_scaled(sprites, 67 + 5 * d, 31, 4, 7, x, y0, k, Some(0), None);
    }
}

/// EMV / EGR / EAR gauge (`1A18:8316`, originally 51x11 at (86, 187)) scaled into the
/// rectangle: EMV blue from the left and red from the right, EGR (upper) and EAR (lower) bars.
pub fn draw_gauge(c: &mut Canvas, r: [i32; 4], emv: i32, egr: i32, ear: i32) {
    let (sx, sy) = (r[2] as f32 / 51.0, r[3] as f32 / 11.0);
    // Value > 0 grows from the left edge, < 0 from the right edge, length |value|/2 (value 0
    // still fills the rightmost column).
    let mut bar = |value: i32, colour: u8, y0: i32, y1: i32| {
        let (a, b) = if value > 0 { (0, value / 2 + 1) } else { (50 + value / 2, 51) };
        let (px0, px1) = (r[0] + (a as f32 * sx).round() as i32, r[0] + (b as f32 * sx).round() as i32);
        let (py0, py1) = (r[1] + (y0 as f32 * sy).round() as i32, r[1] + ((y1 + 1) as f32 * sy).round() as i32);
        c.fill_rect(px0, py0, px1 - px0, py1 - py0, colour);
    };
    bar(100 - emv, 1, 0, 10);
    bar(-emv, 42, 0, 10);
    if egr > 0 {
        let colour = if emv + egr > 100 { 17 } else if emv + egr > 50 { 41 } else { 42 };
        bar(egr, colour, 1, 4);
    }
    if ear > 0 {
        bar(ear, if emv + ear > 100 { 15 } else { 11 }, 6, 9);
    }
}

/// LED bar graph modules per strip of the modern panel's EMV gauge, and segments per module.
const LED_MODULES: i32 = 4;
const LED_SEGMENTS: i32 = 10;

/// The modern panel's EMV gauge: two strips of LED bar graph modules, EGR (upper) and EAR
/// (lower). The radar signal lights segments from the left in yellow, EMV from the right in
/// green, and segments lit by both are red. Each segment is either lit or dark; lit ones are
/// also drawn into `glow`.
pub fn draw_led_bars(c: &mut Canvas, glow: &mut Canvas, r: [i32; 4], emv: i32, egr: i32, ear: i32) {
    let [x, y, w, h] = r;
    c.fill_rect(x, y, w, h, 38);
    let n = LED_MODULES * LED_SEGMENTS;
    // Lit segment counts on the 0..99 scale (any signal lights one; EMV 0 still lights the
    // rightmost segment, as the original's gauge fills its rightmost column).
    let count = |v: i32| if v > 0 { ((v * n + 50) / 99).clamp(1, n) } else { 0 };
    let right = count(emv).max(1);
    let gap = (h / 12).max(1);
    let strip_h = (h - 3 * gap) / 2;
    let module_w = (w - (LED_MODULES + 1) * gap) as f32 / LED_MODULES as f32;
    let pitch = module_w / LED_SEGMENTS as f32;
    let margin = (strip_h / 6).max(1);
    for (row, signal) in [egr, ear].into_iter().enumerate() {
        let left = count(signal);
        let sy = y + gap + row as i32 * (strip_h + gap);
        for m in 0..LED_MODULES {
            let mx = x as f32 + (gap + m * gap) as f32 + m as f32 * module_w;
            c.fill_rect(mx.round() as i32, sy, (mx + module_w).round() as i32 - mx.round() as i32, strip_h, 0);
            for s in 0..LED_SEGMENTS {
                let i = m * LED_SEGMENTS + s;
                let colour = match (i < left, i >= n - right) {
                    (true, true) => Some(13),
                    (true, false) => Some(17),
                    (false, true) => Some(10),
                    (false, false) => None,
                };
                let x0 = (mx + s as f32 * pitch + pitch * 0.15).round() as i32;
                let x1 = (mx + (s + 1) as f32 * pitch - pitch * 0.15).round() as i32;
                let (y0, sh) = (sy + margin, strip_h - 2 * margin);
                c.fill_rect(x0, y0, x1 - x0, sh, colour.unwrap_or(35));
                if let Some(colour) = colour {
                    glow.fill_rect(x0, y0, x1 - x0, sh, colour);
                }
            }
        }
    }
}

/// Attitude ball (`1A18:17A3`: sky 9, ground 1) as a disc filling the canvas, index 0 outside;
/// a fixed aircraft symbol (15) in the middle.
pub fn adi(c: &mut Canvas, pitch: f32, roll: f32) {
    let (s, co) = roll.sin_cos();
    let n = c.w.min(c.h) as f32;
    // Ball units: the original's 32 px wide window is 16 units.
    let unit = n / 16.0;
    let off = 16.0 * pitch.clamp(-1.5, 1.5).tan();
    let r2 = (n / 2.0) * (n / 2.0);
    for py in 0..c.h {
        for px in 0..c.w {
            let (dx, dy) = (px as f32 + 0.5 - c.w as f32 / 2.0, py as f32 + 0.5 - c.h as f32 / 2.0);
            if dx * dx + dy * dy > r2 {
                c.set(px as i32, py as i32, 0);
                continue;
            }
            let (x, y) = (dx / unit, dy / unit);
            // Ground side of the horizon (rotated with the bank, shifted by pitch).
            let ground = x * s + y * co > -off;
            c.set(px as i32, py as i32, if ground { 1 } else { 9 });
        }
    }
    // Aircraft symbol: wings and a centre chevron (the original bezel's "W").
    let (cx, cy, u) = (c.w as i32 / 2, c.h as i32 / 2, (n / 16.0).max(1.0) as i32);
    c.fill_rect(cx - 6 * u, cy - u / 2, 4 * u, u.max(1), 15);
    c.fill_rect(cx + 2 * u, cy - u / 2, 4 * u, u.max(1), 15);
    for i in 0..2 * u {
        c.fill_rect(cx - 2 * u + i, cy + i / 2, u.max(1), u.max(1), 15);
        c.fill_rect(cx + 2 * u - i - u, cy + i / 2, u.max(1), u.max(1), 15);
    }
}

#[derive(SystemParam)]
pub struct PanelParams<'w, 's> {
    pub aircraft: Query<'w, 's, &'static Aircraft, With<Player>>,
    pub hud: Res<'w, HudState>,
    pub mfd: Res<'w, MfdState>,
    pub emv: Res<'w, Emv>,
    pub cm: Res<'w, Countermeasures>,
    pub warnings: Res<'w, Warnings>,
    pub approach: Res<'w, Approach>,
    pub time: Res<'w, Time>,
}

/// Fuel (lbs) below which the low-fuel light blinks.
const LOW_FUEL: f32 = 1500.0;

impl PanelParams<'_, '_> {
    /// Which lamps are lit, indexed by `LampId`.
    pub fn lamp_states(&self) -> [bool; LAMP_COUNT] {
        use LampId::*;
        let blink = crate::hud::blink(&self.time);
        let a = self.aircraft.single().ok();
        let w = &self.warnings;
        let mut lit = [false; LAMP_COUNT];
        lit[match self.hud.mode {
            HudMode::Nav => Nav,
            HudMode::Air => Air,
            HudMode::Gnd => Gnd,
        } as usize] = true;
        // LOCK blinks while the oval is up and is steady in range (the simulation blinks it).
        lit[Lock as usize] = w.lock;
        if let Some(a) = a {
            lit[Stall as usize] = !a.on_ground && a.velocity < a.stall_speed && blink;
            lit[Bay as usize] = a.bay_open;
            lit[Auto as usize] = a.autopilot();
            lit[Flaps as usize] = a.flaps;
            lit[Gear as usize] = a.gear_down;
            lit[Brake as usize] = a.brakes;
            lit[LowFuel as usize] = a.fuel < LOW_FUEL && blink;
        }
        lit[Mw as usize] = w.missile_warning;
        lit[Eng as usize] = w.engine;
        lit[Fc as usize] = w.fire_control;
        lit[Jam as usize] = w.jammer;
        lit[Fire as usize] = w.fire;
        lit[Fuel as usize] = w.fuel;
        lit[Bd as usize] = w.bay_doors;
        lit[Av as usize] = w.avionics;
        for (k, id) in [ChaffAct, FlareAct, DecoyAct].into_iter().enumerate() {
            lit[id as usize] = self.cm.active[k] > 0.0;
        }
        lit[Ecm as usize] = self.cm.ecm;
        lit[Irj as usize] = self.cm.ir_jammer;
        lit[Rad as usize] = w.radar_missile;
        lit[Ir as usize] = w.ir_missile;
        lit[Trak as usize] = w.tracked;
        lit[Map as usize] = self.mfd.left == LeftMfd::Map;
        lit[Tac as usize] = self.mfd.left == LeftMfd::Tac;
        lit[EmvWarn as usize] = w.emv_warning;
        lit[Flir as usize] = self.mfd.flir;
        lit[Wpn as usize] = self.mfd.right == RightMfd::Wpn;
        lit[CamDir as usize] = self.mfd.right == RightMfd::Camera;
        lit[Way as usize] = self.mfd.right == RightMfd::Way;
        lit[Ord as usize] = self.mfd.right == RightMfd::Ord;
        lit[Tc as usize] = self.mfd.right == RightMfd::Camera;
        lit
    }

    /// Label of a lamp; the camera direction lamp shows the direction on the camera page.
    pub fn lamp_label(&self, id: LampId) -> &'static str {
        if id == LampId::CamDir && self.mfd.right == RightMfd::Camera {
            return match self.mfd.cam_dir {
                CamDir::Ahead => "AHEAD",
                CamDir::Right => "RIGHT",
                CamDir::Rear => "REAR",
                CamDir::Left => "LEFT",
            };
        }
        LAMPS[id as usize].1
    }

    /// LED readout text: CHAFF, FLARE, DECOY (2 digits), FUEL (fuel/10, max 999), THRUST %.
    pub fn led_text(&self, id: LedId) -> String {
        let a = self.aircraft.single().ok();
        match id {
            LedId::Chaff => format!("{:02}", self.cm.chaff.min(99)),
            LedId::Flares => format!("{:02}", self.cm.flares.min(99)),
            LedId::Decoys => format!("{:02}", self.cm.decoys.min(99)),
            LedId::Fuel => format!("{:03}", a.map_or(0, |a| ((a.fuel / 10.0) as i32).clamp(0, 999))),
            LedId::Thrust => format!("{:03}", a.map_or(0, |a| (a.thrust.round() as i32).clamp(0, 999))),
        }
    }

    /// EMV, EGR, EAR on the gauge's 0..99 scale.
    pub fn gauge_values(&self) -> (i32, i32, i32) {
        let v = |x: f32| (x.clamp(0.0, 1.0) * 99.0) as i32;
        (v(self.emv.emv), v(self.emv.egr), v(self.emv.ear))
    }

    /// The attitude ball into its own canvas.
    pub fn draw_adi(&self, c: &mut Canvas) {
        let (p, r) = self.aircraft.single().map_or((0.0, 0.0), |a| {
            let (_, p, r) = a.euler();
            (p, r)
        });
        adi(c, p, r);
    }

    /// The approach indexer into its own 10x21 canvas.
    pub fn draw_approach(&self, c: &mut Canvas, art: &CockpitArt) {
        approach_indexer(c, &art.sprites, self.approach.0, 0, 0);
    }
}

/// The approach indexer (`1A18:D9A3`, atlas (251,67) 10x21) with its top-left at (x, y), the
/// lit part recoloured 30 -> 41 (top chevron), 47 (circle) or 14 (bottom chevron).
pub fn approach_indexer(c: &mut Canvas, atlas: &Canvas, approach: u8, x: i32, y: i32) {
    c.copy_rect(atlas, 251, 67, 10, 21, x, y);
    const PARTS: [(i32, i32, u8); 3] = [(2, 4, 41), (7, 7, 47), (15, 4, 14)];
    if let Some(&(dy, h, colour)) = (approach as usize).checked_sub(1).and_then(|i| PARTS.get(i)) {
        c.recolor_rect(x + 1, y + dy, x + 8, y + dy + h, 30, colour);
    }
}
