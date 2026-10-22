//! Instrument panel state and faces: indicator lamps, LED digit readouts, EMV/EGR/EAR gauge,
//! attitude ball and the approach indexer. The faces are drawn into canvas slots that the
//! modelled panel (`cockpit3d`) shows on its lamp caps and instrument windows.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::canvas::Canvas;
use crate::cockpit3d::layout::Dial;
use crate::flight::{Aircraft, Player};
use crate::font::{Fonts, upper};
use crate::hud::{HudMode, HudState};
use crate::mfd::{CamDir, LeftMfd, MfdState, MissionClock, RightMfd};

pub struct PanelPlugin;

impl Plugin for PanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Emv>()
            .init_resource::<Countermeasures>()
            .init_resource::<Warnings>()
            .add_systems(OnEnter(crate::settings::AppState::Flight), reset_panel);
    }
}

/// Electromagnetic visibility and threat signals for the EMV/EGR/EAR gauge, 0..1
/// (driven by the combat simulation).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Emv {
    /// Own emission level (/100).
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

fn reset_panel(mut cm: ResMut<Countermeasures>, mut emv: ResMut<Emv>, mut w: ResMut<Warnings>) {
    *cm = Countermeasures::default();
    *emv = Emv::default();
    *w = Warnings::default();
}

/// The approach indexer's canvas (a 5:14 strip: chevron, circle, chevron).
pub const INDEXER_CANVAS: (usize, usize) = (40, 112);

/// Panel lamps (the classic panel's lights). The index is the classic light number.
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

/// Lamp colours, (text and border, fill) unlit and lit, as they recolour the panel art.
#[derive(Debug, Clone, Copy)]
pub struct LampStyle {
    pub off: (u8, u8),
    pub on: (u8, u8),
}

pub const GREEN: LampStyle = LampStyle { off: (45, 44), on: (47, 46) };
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
pub fn draw_lamp(
    c: &mut Canvas,
    fonts: &Fonts,
    r: [i32; 4],
    label: &str,
    colours: (u8, u8),
    k: i32,
    text: i32,
) {
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

/// LED readout (atlas digits (67+5d, 31) 4x7, 5 px advance; non-digits show "0"),
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

/// EMV / EGR / EAR gauge (51x11 at (86, 187) on the classic panel) scaled into the
/// rectangle: EMV blue from the left and red from the right, EGR (upper) and EAR (lower) bars.
pub fn draw_gauge(c: &mut Canvas, r: [i32; 4], emv: i32, egr: i32, ear: i32) {
    let (sx, sy) = (r[2] as f32 / 51.0, r[3] as f32 / 11.0);
    // Value > 0 grows from the left edge, < 0 from the right edge, length |value|/2 (value 0
    // still fills the rightmost column).
    let mut bar = |value: i32, colour: u8, y0: i32, y1: i32| {
        let (a, b) = if value > 0 { (0, value / 2 + 1) } else { (50 + value / 2, 51) };
        let (px0, px1) =
            (r[0] + (a as f32 * sx).round() as i32, r[0] + (b as f32 * sx).round() as i32);
        let (py0, py1) =
            (r[1] + (y0 as f32 * sy).round() as i32, r[1] + ((y1 + 1) as f32 * sy).round() as i32);
        c.fill_rect(px0, py0, px1 - px0, py1 - py0, colour);
    };
    bar(100 - emv, 1, 0, 10);
    bar(-emv, 42, 0, 10);
    if egr > 0 {
        let colour = if emv + egr > 100 {
            17
        } else if emv + egr > 50 {
            41
        } else {
            42
        };
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
    // rightmost segment, as the classic gauge fills its rightmost column).
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
            c.fill_rect(
                mx.round() as i32,
                sy,
                (mx + module_w).round() as i32 - mx.round() as i32,
                strip_h,
                0,
            );
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

/// Attitude ball (sky 9, ground 1) as a disc filling the canvas, index 0 outside;
/// a fixed aircraft symbol (15) in the middle.
pub fn adi(c: &mut Canvas, pitch: f32, roll: f32) {
    let (s, co) = roll.sin_cos();
    let n = c.w.min(c.h) as f32;
    // Ball units: the classic 32 px wide window is 16 units.
    let unit = n / 16.0;
    // Nose up drops the horizon below the centre (canvas y grows downwards).
    let off = -16.0 * pitch.clamp(-1.5, 1.5).tan();
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
    // Aircraft symbol: wings and a centre chevron (the classic bezel's "W").
    let (cx, cy, u) = (c.w as i32 / 2, c.h as i32 / 2, (n / 16.0).max(1.0) as i32);
    c.fill_rect(cx - 6 * u, cy - u / 2, 4 * u, u.max(1), 15);
    c.fill_rect(cx + 2 * u, cy - u / 2, 4 * u, u.max(1), 15);
    for i in 0..2 * u {
        c.fill_rect(cx - 2 * u + i, cy + i / 2, u.max(1), u.max(1), 15);
        c.fill_rect(cx + 2 * u - i - u, cy + i / 2, u.max(1), u.max(1), 15);
    }
}

/// Pitch ladder degrees per ball radius of `adi_ball`.
const BALL_DEG: f32 = 40.0;

/// Attitude ball after the F-16's ADI (the modern cockpits) on a square canvas, index 0 outside: sky blue (9) over
/// brown ground (6) split by a white horizon, a pitch ladder every 5 degrees (black on the sky,
/// white on the ground, numbered every 10), a black case ring with the bank scale and the bank
/// pointer turning with the ball, and a small fixed yellow aircraft symbol.
pub fn adi_ball(c: &mut Canvas, pitch: f32, roll: f32) {
    use crate::font::{Align, stroke_text_at};
    let (s, co) = roll.sin_cos();
    let n = c.w.min(c.h) as f32;
    let centre = Vec2::new(c.w as f32 / 2.0, c.h as f32 / 2.0);
    let r = n / 2.0;
    let ring = r * 0.84;
    let k = ring / BALL_DEG;
    // The ball's axes on the canvas: along the horizon and towards the ground.
    let along = Vec2::new(co, -s);
    let down = Vec2::new(s, co);
    // Same sense as the classic ball (`adi`): the horizon sits at -h along `down`, so nose up
    // moves it towards the ground side.
    let h = -pitch.clamp(-1.5, 1.5).to_degrees() * k;
    for py in 0..c.h {
        for px in 0..c.w {
            let d = Vec2::new(px as f32 + 0.5, py as f32 + 0.5) - centre;
            let len = d.length();
            let colour = if len > r {
                0
            } else if len > ring {
                5
            } else if d.dot(down) > -h {
                6
            } else {
                9
            };
            c.set(px as i32, py as i32, colour);
        }
    }
    let w = (n / 110.0).max(1.0);
    // Horizon and ladder, clipped to the ball's square inside the ring (the ring covers the rest).
    let inner = ring * 0.98;
    let mid = |deg: f32| centre + down * (-h - deg * k);
    let clip = |p: Vec2, q: Vec2| {
        // The chord of the ball's circle through p..q, or nothing outside it.
        let dir = (q - p).normalize();
        let f = p - centre;
        let b = f.dot(dir);
        let disc = b * b - (f.length_squared() - inner * inner);
        if disc <= 0.0 {
            return None;
        }
        let (t0, t1) = (-b - disc.sqrt(), -b + disc.sqrt());
        let len = (q - p).length();
        let (a, z) = (t0.max(0.0), t1.min(len));
        (a < z).then(|| (p + dir * a, p + dir * z))
    };
    if let Some((a, b)) = clip(mid(0.0) - along * ring, mid(0.0) + along * ring) {
        c.stroke(a, b, w * 1.6, 15);
    }
    let mut deg = -85.0;
    while deg <= 85.0 {
        let p = mid(deg);
        if deg != 0.0 && (p - centre).length() < inner {
            let half = if (deg as i32) % 10 == 0 { ring * 0.28 } else { ring * 0.12 };
            let colour = if deg > 0.0 { 5 } else { 15 };
            if let Some((a, b)) = clip(p - along * half, p + along * half) {
                c.stroke(a, b, w, colour);
            }
            if (deg as i32) % 10 == 0 && (p - centre).length() < inner * 0.8 {
                let label = format!("{}", (deg as i32).abs());
                for side in [-1.0, 1.0] {
                    let q = p + along * side * (half + ring * 0.16);
                    if (q - centre).length() < inner * 0.9 {
                        stroke_text_at(c, q.x, q.y, &label, ring * 0.1, w, colour, Align::Centre);
                    }
                }
            }
        }
        deg += 5.0;
    }
    // Bank scale on the case: 10, 20, 30, 60 and 90 degrees either side, a triangle at 0 and 45.
    let at = |a: f32, rad: f32| centre + Vec2::new(a.sin(), -a.cos()) * rad;
    for a in [10.0f32, 20.0, 30.0, 60.0, 90.0] {
        for sign in [-1.0, 1.0] {
            let a = (sign * a).to_radians();
            let long = if a.abs() > 0.5 { 0.14 } else { 0.09 };
            c.stroke(at(a, ring * 1.02), at(a, ring * (1.0 + long)), w * 1.2, 15);
        }
    }
    for a in [-45.0f32, 0.0, 45.0] {
        let a = a.to_radians();
        let tip = at(a, ring * 1.02);
        let side = Vec2::new(a.cos(), a.sin()) * r * 0.05;
        c.triangle([tip, at(a, ring * 1.13) - side, at(a, ring * 1.13) + side], 15);
    }
    // Bank pointer on the ball's rim, towards its sky.
    let (tip, base) = (centre - down * ring * 0.99, centre - down * ring * 0.87);
    c.triangle([tip, base - along * r * 0.055, base + along * r * 0.055], 15);
    // Aircraft symbol: two short wings with drooping tips and a centre dot (yellow).
    let wing = ring * 0.34;
    let gap = ring * 0.12;
    for sx in [-1.0, 1.0] {
        let a = centre + Vec2::new(sx * gap, 0.0);
        let b = centre + Vec2::new(sx * (gap + wing), 0.0);
        c.stroke(a, b, w * 2.2, 17);
        c.stroke(a, a + Vec2::new(0.0, ring * 0.08), w * 2.2, 17);
    }
    c.stroke(centre - Vec2::X * w * 0.5, centre + Vec2::X * w * 0.5, w * 3.0, 17);
}

/// HSI on a square canvas: the compass card turning with the heading (radians), a lubber line
/// and aircraft symbol, the bearing pointer to the waypoint (radians, true) and its distance
/// (km) and number.
pub fn hsi(
    c: &mut Canvas,
    heading: f32,
    bearing: Option<f32>,
    dist_km: Option<f32>,
    waypoint: usize,
) {
    use crate::font::{Align, stroke_text_at};
    c.px.fill(0);
    let n = c.w.min(c.h) as f32;
    let centre = Vec2::new(c.w as f32 / 2.0, c.h as f32 / 2.0);
    let r = n / 2.0 - 1.0;
    for y in 0..c.h {
        for x in 0..c.w {
            if (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - centre).length() <= r {
                c.set(x as i32, y as i32, 5);
            }
        }
    }
    let w = (n / 150.0).max(1.0);
    // Direction on the card: up = the nose.
    let dir = |a: f32| Vec2::new((a - heading).sin(), -(a - heading).cos());
    let card = r * 0.78;
    for k in 0..72 {
        let a = (k as f32 * 5.0).to_radians();
        let len = if k % 6 == 0 {
            0.12
        } else if k % 2 == 0 {
            0.08
        } else {
            0.05
        };
        c.stroke(centre + dir(a) * card, centre + dir(a) * card * (1.0 - len), w, 15);
        if k % 6 == 0 {
            let deg = k * 5;
            let label = match deg {
                0 => "N".to_string(),
                90 => "E".to_string(),
                180 => "S".to_string(),
                270 => "W".to_string(),
                d => (d / 10).to_string(),
            };
            let p = centre + dir(a) * card * 0.76;
            stroke_text_at(c, p.x, p.y, &label, r * 0.11, w, 15, Align::Centre);
        }
    }
    // Lubber line.
    c.stroke(centre - Vec2::Y * (r * 0.98), centre - Vec2::Y * (card * 1.02), w * 1.5, 15);
    // Bearing pointer to the waypoint (cyan): head at the card, tail opposite.
    if let Some(b) = bearing {
        let d = dir(b);
        let head = centre + d * card * 0.98;
        let tail = centre - d * card * 0.98;
        c.stroke(tail, head, w * 1.5, 11);
        let side = Vec2::new(-d.y, d.x) * r * 0.07;
        c.polyline(&[head - d * r * 0.14 + side, head, head - d * r * 0.14 - side], w * 1.5, 11);
    }
    // Aircraft symbol.
    let s = r * 0.12;
    c.stroke(centre - Vec2::X * s, centre + Vec2::X * s, w * 1.5, 41);
    c.stroke(centre - Vec2::Y * s * 0.8, centre + Vec2::Y * s * 1.2, w * 1.5, 41);
    c.stroke(centre + Vec2::new(-s * 0.45, s), centre + Vec2::new(s * 0.45, s), w * 1.5, 41);
    // Distance (top left) and waypoint (top right).
    let th = r * 0.12;
    if let Some(km) = dist_km {
        let text = if km < 100.0 { format!("{km:.1}") } else { format!("{km:.0}") };
        stroke_text_at(
            c,
            centre.x - r * 0.62,
            centre.y - r * 0.62,
            &text,
            th,
            w,
            15,
            Align::Centre,
        );
    }
    stroke_text_at(
        c,
        centre.x + r * 0.62,
        centre.y - r * 0.62,
        &format!("WP{waypoint}"),
        th,
        w,
        15,
        Align::Centre,
    );
}

#[derive(SystemParam)]
pub struct PanelParams<'w, 's> {
    pub aircraft: Query<'w, 's, &'static Aircraft, With<Player>>,
    pub hud: Res<'w, HudState>,
    pub mfd: Res<'w, MfdState>,
    pub emv: Res<'w, Emv>,
    pub cm: Res<'w, Countermeasures>,
    pub warnings: Res<'w, Warnings>,
    pub time: Res<'w, Time>,
    pub positions: Query<'w, 's, &'static crate::world::WorldPos, With<Player>>,
    pub waypoints: Res<'w, crate::hud::Waypoints>,
    pub emitters: Res<'w, crate::rwr::Emitters>,
    pub clock: Res<'w, MissionClock>,
    pub mission: Option<Res<'w, crate::mission::ActiveMission>>,
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
            LedId::Fuel => {
                format!("{:03}", a.map_or(0, |a| ((a.fuel / 10.0) as i32).clamp(0, 999)))
            }
            LedId::Thrust => {
                format!("{:03}", a.map_or(0, |a| (a.thrust.round() as i32).clamp(0, 999)))
            }
        }
    }

    /// EMV, EGR, EAR on the gauge's 0..99 scale.
    pub fn gauge_values(&self) -> (i32, i32, i32) {
        let v = |x: f32| (x.clamp(0.0, 1.0) * 99.0) as i32;
        (v(self.emv.emv), v(self.emv.egr), v(self.emv.ear))
    }

    /// The attitude ball into its own canvas: the F-16's ADI (`ball`, the modern cockpits) or the
    /// classic ball (the classic cockpit).
    pub fn draw_adi(&self, c: &mut Canvas, ball: bool) {
        let (p, r) = self.aircraft.single().map_or((0.0, 0.0), |a| {
            let (_, p, r) = a.euler();
            (p, r)
        });
        if ball {
            adi_ball(c, p, r);
        } else {
            adi(c, p, r);
        }
    }

    /// The horizontal situation indicator: heading and the bearing and distance to the
    /// current waypoint.
    pub fn draw_hsi(&self, c: &mut Canvas) {
        let Ok(a) = self.aircraft.single() else { return };
        let heading = a.euler().0;
        let wp = self.waypoints.points.get(self.waypoints.current);
        let dist = wp.zip(self.positions.single().ok()).map(|(w, p)| {
            let (e, n, _) = crate::world::bevy_to_wu(p.0);
            ((w.east - e).hypot(w.north - n) * crate::world::WU / 1000.0) as f32
        });
        let bearing = wp.map(|_| crate::flight::bam(a.waypoint_bearing));
        hsi(c, heading, bearing, dist, self.waypoints.current);
    }

    /// The RWR scope.
    pub fn draw_rwr(&self, c: &mut Canvas) {
        crate::rwr::draw_rwr(c, &self.emitters.list, crate::hud::blink(&self.time));
    }

    /// Angle of attack (degrees) from the flight path in the body frame.
    fn aoa(a: &Aircraft) -> f32 {
        let body = a.orientation.inverse() * a.path_dir();
        (-body.y).atan2(-body.z).to_degrees()
    }

    /// The approach indexer: the symbols lit for the angle of attack (gear up or down, as in
    /// the F-16): the red top chevron 2 degrees or more above the approach AOA (slow), the
    /// green circle within 2 degrees of it, the amber bottom chevron 2 degrees or more below
    /// (fast); a chevron and the circle together in the outer degree.
    pub fn draw_approach(&self, c: &mut Canvas) {
        let lit = match self.aircraft.single().ok().filter(|a| !a.on_ground) {
            Some(a) => {
                let d = Self::aoa(a) - crate::hud::modern::APPROACH_AOA;
                (d >= 1.0, d.abs() < 2.0, d <= -1.0)
            }
            None => (false, false, false),
        };
        approach_indexer(c, lit);
    }

    /// Distance (km) and ETA (seconds of the day) to the current waypoint at the current speed.
    fn leg(&self, a: &Aircraft, t: f32) -> Option<(f32, f32)> {
        let wp = self.waypoints.target()?;
        let p = self.positions.single().ok()?;
        let (e, n, _) = crate::world::bevy_to_wu(p.0);
        let km = ((wp.east - e).hypot(wp.north - n) * crate::world::WU / 1000.0) as f32;
        let ms = a.knots() * 0.5144;
        Some((km, t + if ms > 1.0 { km * 1000.0 / ms } else { 0.0 }))
    }

    /// The F-16's data entry display: a CNI-style page with the radio channels (fixed), the
    /// steerpoint and the target, the clock, the leg to the steerpoint and the time on target
    /// or the fuel, 24 columns of 5 lines in amber.
    pub fn draw_ded(&self, c: &mut Canvas, fonts: &Fonts) {
        use crate::cockpit3d::layout::{DED_COLS, DED_SCALE};
        const AMBER: u8 = 18;
        c.clear(5);
        let a = self.aircraft.single().ok();
        let t = self.clock.start_seconds
            + self.clock.elapsed.unwrap_or_else(|| a.map_or(0.0, |a| a.time));
        let (hh, mm, ss) = crate::mfd::hms(t);
        let mission = self.mission.as_ref().map(|m| &m.0);
        let target = mission.map(|m| {
            (
                crate::mission::clean_text(&m.primary.type_name).to_uppercase(),
                crate::mission::clean_text(&m.primary.place_name).to_uppercase(),
            )
        });
        let (kind, place) = target.unwrap_or_default();
        let leg = a.and_then(|a| self.leg(a, t));
        let bearing = a.filter(|_| leg.is_some()).map_or(0, |a| {
            crate::flight::bam(a.waypoint_bearing).to_degrees().rem_euclid(360.0).round() as i32
                % 360
        });
        let dist = leg.map_or("---".to_string(), |(km, _)| {
            if km < 100.0 { format!("{km:.1}KM") } else { format!("{km:.0}KM") }
        });
        let eta = leg.map_or("--:--".to_string(), |(_, eta)| {
            let (eh, em, _) = crate::mfd::hms(eta);
            format!("{eh:02}:{em:02}")
        });
        let right = match mission.filter(|m| m.time.has_deadline) {
            Some(m) => format!("TOT {}", m.time.deadline),
            None => format!("FUEL {:>5.0}", a.map_or(0.0, |a| a.fuel)),
        };
        let steer = if self.waypoints.points.is_empty() {
            "STPT -".to_string()
        } else {
            format!("STPT {}", self.waypoints.current + 1)
        };
        let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
        let lines = [
            format!("{:<12}{:>12}", "UHF 305.00", steer),
            format!("{:<12}{:>12}", cut(&kind, 12), cut(&place, 12)),
            format!("{:<12}{:>12}", "VHF 123.45", format!("{hh:02}:{mm:02}:{ss:02}")),
            format!("BRG {bearing:03} {dist:>6} ETA {eta}"),
            format!("{:<12}{:>12}", "M3 1234", right),
        ];
        for (i, line) in lines.iter().enumerate() {
            fonts.text_scaled(
                c,
                4,
                2 + i as i32 * 8 * DED_SCALE,
                cut(line, DED_COLS as usize).as_bytes(),
                AMBER,
                DED_SCALE,
            );
        }
    }

    /// What a needle instrument shows (`draw_dial`'s units).
    pub fn dial_value(&self, dial: Dial) -> f32 {
        let Ok(a) = self.aircraft.single() else { return 0.0 };
        let thrust = a.thrust.clamp(0.0, 120.0);
        match dial {
            Dial::Airspeed => a.knots(),
            Dial::Altimeter => a.altitude.max(0.0),
            Dial::Aoa => Self::aoa(a),
            // Feet per second to feet per minute (as the HUD's VVI).
            Dial::Vvi => a.climb_rate * 60.0,
            Dial::Rpm => 60.0 + thrust * 0.4,
            Dial::Ftit => 350.0 + thrust * 6.0,
            Dial::Oil => 25.0 + thrust * 0.4,
            Dial::Nozzle => {
                if thrust > 100.0 {
                    (thrust - 100.0) * 5.0
                } else {
                    100.0 - thrust
                }
            }
            Dial::Fuel => a.fuel,
            // Seconds of the day.
            Dial::Clock => self.clock.start_seconds + self.clock.elapsed.unwrap_or(a.time),
        }
    }
}

/// A round needle instrument into the rectangle r of an instrument atlas: black face, grey
/// ring and name, white scale and needle. The altimeter has a long needle for the hundreds, a
/// short one for the thousands and a digital readout; the clock hour, minute and second hands.
pub fn draw_dial(c: &mut Canvas, r: [i32; 4], dial: Dial, value: f32) {
    use crate::font::{Align, stroke_text_at};
    if matches!(dial, Dial::Aoa | Dial::Vvi) {
        draw_tape(c, r, dial, value);
        return;
    }
    let [x, y, w, h] = r;
    c.fill_rect(x, y, w, h, 5);
    let centre = Vec2::new(x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0);
    let rad = w.min(h) as f32 / 2.0 - 1.0;
    let pen = (rad / 40.0).max(1.0);
    c.ring(centre, rad - pen, pen, 7);
    // Name, scale labels (evenly over the sweep), the needle's fraction of the sweep, and the
    // sweep: start angle (degrees clockwise from up) and extent.
    let (name, labels, t, start, sweep): (&str, &[&str], f32, f32, f32) = match dial {
        Dial::Airspeed => {
            ("KNOTS", &["0", "1", "2", "3", "4", "5", "6", "7", "8"], value / 800.0, -150.0, 300.0)
        }
        Dial::Altimeter => (
            "ALT",
            &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"],
            (value % 1000.0) / 1000.0,
            0.0,
            360.0,
        ),
        Dial::Aoa => (
            "AOA",
            &["-5", "0", "5", "10", "15", "20", "25", "30"],
            (value + 5.0) / 35.0,
            -150.0,
            300.0,
        ),
        Dial::Vvi => {
            ("VVI", &["6", "4", "2", "0", "2", "4", "6"], (value + 6000.0) / 12000.0, -240.0, 300.0)
        }
        Dial::Rpm => ("RPM", &["0", "2", "4", "6", "8", "10"], value / 100.0, -150.0, 300.0),
        Dial::Ftit => {
            ("FTIT", &["2", "4", "6", "8", "10", "12"], (value - 200.0) / 1000.0, -150.0, 300.0)
        }
        Dial::Oil => ("OIL", &["0", "25", "50", "75", "100"], value / 100.0, -150.0, 300.0),
        Dial::Nozzle => ("NOZ", &["0", "2", "4", "6", "8", "10"], value / 100.0, -150.0, 300.0),
        Dial::Fuel => {
            ("FUEL", &["0", "2", "4", "6", "8", "10"], value / crate::mfd::FULL_FUEL, -150.0, 300.0)
        }
        Dial::Clock => (
            "",
            &["12", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"],
            (value % 3600.0) / 3600.0,
            0.0,
            360.0,
        ),
    };
    let dir = |deg: f32| {
        let a = deg.to_radians();
        Vec2::new(a.sin(), -a.cos())
    };
    let n = labels.len() as f32 - 1.0;
    // The altimeter's scale goes all the way round: no label at the end.
    let last = if sweep >= 360.0 { labels.len() - 1 } else { labels.len() };
    for (i, label) in labels.iter().enumerate().take(last) {
        let a = start + sweep * i as f32 / if sweep >= 360.0 { labels.len() as f32 } else { n };
        let d = dir(a);
        c.stroke(centre + d * rad * 0.92, centre + d * rad * 0.78, pen, 15);
        let p = centre + d * rad * 0.62;
        stroke_text_at(c, p.x, p.y, label, rad * 0.18, pen, 15, Align::Centre);
        if i + 1 < last || sweep >= 360.0 {
            let d = dir(a + sweep / 2.0 / if sweep >= 360.0 { labels.len() as f32 } else { n });
            c.stroke(centre + d * rad * 0.92, centre + d * rad * 0.85, pen, 15);
        }
    }
    let name_y = if dial == Dial::Altimeter { -0.3 } else { -0.38 };
    if !name.is_empty() {
        stroke_text_at(
            c,
            centre.x,
            centre.y + rad * name_y,
            name,
            rad * 0.15,
            pen,
            7,
            Align::Centre,
        );
    }
    match dial {
        Dial::Altimeter => {
            let d = dir(start + sweep * (value % 10000.0) / 10000.0);
            c.stroke(centre, centre + d * rad * 0.5, pen * 2.2, 15);
            stroke_text_at(
                c,
                centre.x,
                centre.y + rad * 0.38,
                &format!("{:05}", value.round() as i32),
                rad * 0.17,
                pen,
                15,
                Align::Centre,
            );
        }
        Dial::Clock => {
            let d = dir(360.0 * (value % 43200.0) / 43200.0);
            c.stroke(centre, centre + d * rad * 0.5, pen * 2.2, 15);
            let d = dir(360.0 * (value % 60.0) / 60.0);
            c.stroke(centre - d * rad * 0.15, centre + d * rad * 0.85, pen * 0.8, 41);
        }
        _ => {}
    }
    let d = dir(start + sweep * t.clamp(0.0, 1.0));
    c.stroke(centre - d * rad * 0.12, centre + d * rad * 0.8, pen * 1.5, 15);
    c.circle(centre.x, centre.y, pen * 2.0, 7);
}

/// A vertical tape instrument into the rectangle r of an instrument atlas (the F-16's AOA and
/// vertical velocity indicators): the scale moving behind a fixed yellow index across the
/// middle, on black. AOA in degrees, the approach band green and from 25 degrees red; vertical
/// velocity in feet per minute, labelled in thousands, "UP" over and "DN" under the index.
fn draw_tape(c: &mut Canvas, r: [i32; 4], dial: Dial, value: f32) {
    use crate::font::{Align, stroke_text_at};
    let [x, y, w, h] = r;
    c.fill_rect(x, y, w, h, 5);
    let (x0, y0, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let mid = y0 + h / 2.0;
    let pen = (w / 24.0).max(1.0);
    // Scale step (units per tick), units per tape height, labelled every `every` ticks, the
    // label for a value and the scale's limits.
    let (step, span, every, lo, hi): (f32, f32, i32, f32, f32) = match dial {
        Dial::Aoa => (1.0, 16.0, 5, -5.0, 35.0),
        _ => (500.0, 4000.0, 2, -6000.0, 6000.0),
    };
    let v = value.clamp(lo, hi);
    let to_y = |u: f32| mid - (u - v) / span * h;
    c.set_clip(x, y, x + r[2] - 1, y + r[3] - 1);
    if dial == Dial::Aoa {
        let mut band = |a: f32, b: f32, colour: u8| {
            let (ya, yb) = (to_y(b), to_y(a));
            c.fill_rect(
                x + r[2] - (w * 0.3) as i32,
                ya as i32,
                (w * 0.3) as i32,
                (yb - ya) as i32,
                colour,
            );
        };
        let ap = crate::hud::modern::APPROACH_AOA;
        band(ap - 2.0, ap + 2.0, 47);
        band(25.0, hi, 41);
    }
    let first = ((v - span / 2.0) / step).floor() as i32;
    let last = ((v + span / 2.0) / step).ceil() as i32;
    for i in first..=last {
        let u = i as f32 * step;
        if u < lo || u > hi {
            continue;
        }
        let ty = to_y(u);
        let major = i % every == 0;
        let len = if major { w * 0.3 } else { w * 0.18 };
        c.stroke(Vec2::new(x0, ty), Vec2::new(x0 + len, ty), pen, 15);
        if major {
            let label = match dial {
                Dial::Aoa => format!("{}", u as i32),
                _ => format!("{}", (u / 1000.0).abs() as i32),
            };
            stroke_text_at(c, x0 + w * 0.55, ty, &label, w * 0.28, pen, 15, Align::Centre);
        }
    }
    if dial == Dial::Vvi {
        stroke_text_at(c, x0 + w / 2.0, y0 + h * 0.07, "UP", w * 0.22, pen, 7, Align::Centre);
        stroke_text_at(c, x0 + w / 2.0, y0 + h * 0.93, "DN", w * 0.22, pen, 7, Align::Centre);
    }
    // The index: a yellow line across with a pointer from the left.
    c.stroke(Vec2::new(x0, mid), Vec2::new(x0 + w, mid), pen * 1.4, 17);
    c.triangle(
        [
            Vec2::new(x0 + w * 0.3, mid),
            Vec2::new(x0, mid - w * 0.18),
            Vec2::new(x0, mid + w * 0.18),
        ],
        17,
    );
    c.reset_clip();
}

/// The approach indexer into its canvas (`INDEXER_CANVAS`): on black, the top chevron
/// pointing down (red when lit), the circle (green) and the bottom chevron pointing up (amber);
/// unlit symbols dim grey.
pub fn approach_indexer(c: &mut Canvas, lit: (bool, bool, bool)) {
    c.clear(5);
    let (w, h) = (c.w as f32, c.h as f32);
    let cx = w / 2.0;
    let pen = w / 12.0;
    let half = w * 0.3;
    let colour = |on: bool, lit_colour: u8| if on { lit_colour } else { 30 };
    // Top chevron: a V with its apex down.
    let y = h * 0.18;
    c.polyline(
        &[
            Vec2::new(cx - half, y - half * 0.6),
            Vec2::new(cx, y + half * 0.6),
            Vec2::new(cx + half, y - half * 0.6),
        ],
        pen,
        colour(lit.0, 41),
    );
    c.ring(Vec2::new(cx, h / 2.0), half * 0.9, pen, colour(lit.1, 47));
    // Bottom chevron: apex up.
    let y = h * 0.82;
    c.polyline(
        &[
            Vec2::new(cx - half, y + half * 0.6),
            Vec2::new(cx, y - half * 0.6),
            Vec2::new(cx + half, y + half * 0.6),
        ],
        pen,
        colour(lit.2, 18),
    );
}
