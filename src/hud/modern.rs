//! High-resolution HUD in the style of the F-16's: NAV, AA (air-to-air) and AG
//! (air-to-ground) symbology drawn with thin strokes into a large monochrome canvas. The
//! combiner glass (assets/shaders/hud_glass.wgsl) maps it to directions from the eye and draws
//! every set pixel in HUD green.
//!
//! Canvas pixels are angular: (x, y) is the direction (x - CX, CY - y) / scale in the
//! tangent plane one unit in front of the eye (aircraft frame), so symbols placed by
//! projecting a direction sit on that direction in the world. The scale is the aircraft's
//! (`HudFrame::scale`): a smaller combiner shows the same symbology over a smaller angle.

use bevy::prelude::*;

use crate::canvas::Canvas;
use crate::font::{Align, stroke_text, stroke_text_at, stroke_width};

use super::{HudFrame, HudMode};

/// Canvas size.
pub const WIDTH: usize = 1280;
pub const HEIGHT: usize = 512;
/// Boresight (canvas pixels) and pixels per unit of the tangent plane.
pub const CX: f32 = 640.0;
pub const CY: f32 = 280.0;
/// The F-117A's pixels per unit (its combiner's size).
pub const SCALE: f32 = 1024.0;

/// Ink (any colour but the transparent 0; the glass draws all ink in HUD green).
const INK: u8 = 15;
/// Pen widths and text heights (pixels).
const PEN: f32 = 2.4;
const THIN: f32 = 1.8;
const TEXT: f32 = 15.0;
const SMALL: f32 = 12.0;
/// Area symbols are kept inside (the combiner is roughly x 182..1098, y 14..483).
const FIELD: (Vec2, Vec2) = (Vec2::new(250.0, 75.0), Vec2::new(1030.0, 465.0));

/// Pixel of a direction in the aircraft frame (x right, y up, -z forward) at `scale` pixels
/// per unit; None behind.
pub fn project(d: Vec3, scale: f32) -> Option<Vec2> {
    let z = -d.z;
    (z > 1e-3).then(|| Vec2::new(CX + scale * d.x / z, CY - scale * d.y / z))
}

fn inside(p: Vec2) -> bool {
    p.cmpge(FIELD.0).all() && p.cmple(FIELD.1).all()
}

fn text(c: &mut Canvas, x: f32, y: f32, s: &str, align: Align) {
    stroke_text_at(c, x, y, s, TEXT, PEN * 0.85, INK, align);
}

fn small(c: &mut Canvas, x: f32, y: f32, s: &str, align: Align) {
    stroke_text_at(c, x, y, s, SMALL, THIN, INK, align);
}

/// Text in a box (readouts).
fn boxed(c: &mut Canvas, x: f32, y: f32, s: &str, min_chars: usize, align: Align) {
    let w = stroke_width(s, TEXT).max(stroke_width(&"0".repeat(min_chars), TEXT));
    let x0 = match align {
        Align::Left => x,
        Align::Centre => x - w / 2.0,
        Align::Right => x - w,
    };
    stroke_text(c, x0 + (w - stroke_width(s, TEXT)), y - TEXT / 2.0, s, TEXT, PEN * 0.85, INK);
    c.frame(
        Vec2::new(x0 - 6.0, y - TEXT / 2.0 - 6.0),
        Vec2::new(x0 + w + 6.0, y + TEXT / 2.0 + 6.0),
        THIN,
        INK,
    );
}

fn diamond(c: &mut Canvas, p: Vec2, r: f32) {
    c.polyline(
        &[
            p + Vec2::new(0.0, -r),
            p + Vec2::new(r, 0.0),
            p + Vec2::new(0.0, r),
            p + Vec2::new(-r, 0.0),
            p + Vec2::new(0.0, -r),
        ],
        PEN,
        INK,
    );
}

/// The steerpoint diamond (direction in the aircraft frame); off the field it waits at the
/// field's edge in the steerpoint's direction, crossed out.
fn steerpoint_cue(c: &mut Canvas, d: Vec3, scale: f32) {
    const R: f32 = 9.0;
    let p = project(d, scale);
    if let Some(p) = p.filter(|&p| inside(p)) {
        diamond(c, p, R);
        return;
    }
    let centre = Vec2::new(CX, CY);
    let Some(dir) = p.map_or(Vec2::new(d.x, -d.y), |p| p - centre).try_normalize() else {
        return;
    };
    // Inside the tapes and above the readouts.
    let (lo, hi) = (Vec2::new(420.0, FIELD.0.y + R), Vec2::new(860.0, 365.0));
    let reach = |d: f32, lo: f32, hi: f32, c: f32| match d {
        d if d > 1e-6 => (hi - c) / d,
        d if d < -1e-6 => (lo - c) / d,
        _ => f32::INFINITY,
    };
    let t = reach(dir.x, lo.x, hi.x, CX).min(reach(dir.y, lo.y, hi.y, CY));
    let q = centre + dir * t;
    diamond(c, q, R);
    let k = R * 0.7;
    c.stroke(q + Vec2::new(-k, -k), q + Vec2::new(k, k), THIN, INK);
    c.stroke(q + Vec2::new(-k, k), q + Vec2::new(k, -k), THIN, INK);
}

/// Screen directions of the horizon (to the right) and of world "down", from the attitude.
fn horizon_axes(f: &HudFrame) -> (Vec2, Vec2) {
    let inv = f.orientation.inverse();
    let fwd = f.orientation * Vec3::NEG_Z;
    let level = Vec3::new(fwd.x, 0.0, fwd.z).try_normalize().unwrap_or(Vec3::NEG_Z);
    let right = Vec3::new(-level.z, 0.0, level.x);
    let a = project(inv * level, f.scale);
    let b = project(inv * (level + right * 0.01), f.scale);
    let h = match (a, b) {
        (Some(a), Some(b)) => (b - a).try_normalize().unwrap_or(Vec2::X),
        _ => {
            let (_, _, roll) = euler(f);
            Vec2::new(roll.cos(), -roll.sin())
        }
    };
    (h, Vec2::new(-h.y, h.x))
}

fn euler(f: &HudFrame) -> (f32, f32, f32) {
    let o = f.orientation;
    let fw = o * Vec3::NEG_Z;
    let r = o * Vec3::X;
    let u = o * Vec3::Y;
    (
        fw.x.atan2(-fw.z).rem_euclid(std::f32::consts::TAU),
        fw.y.clamp(-1.0, 1.0).asin(),
        (-r.y).atan2(u.y),
    )
}

/// Draws the whole HUD.
pub fn draw(c: &mut Canvas, f: &HudFrame) {
    c.reset_clip();
    let st = &f.state;
    let inv = f.orientation.inverse();
    let (h, down) = horizon_axes(f);

    // Flight path marker (caged to the field with an X when it would leave it).
    // A slow helicopter has no meaningful flight path: the ladder centres on the boresight,
    // the drift vector shows where it goes.
    let hover = f.heli.is_some() && f.knots < HOVER_KNOTS;
    let fpm_raw = if hover { Some(Vec2::new(CX, CY)) } else { project(inv * f.path_dir, f.scale) };
    let fpm = fpm_raw.map_or(Vec2::new(CX, FIELD.1.y), |p| p.clamp(FIELD.0, FIELD.1));
    let caged = fpm_raw.is_none_or(|p| !inside(p));

    if st.declutter < 2 || st.mode == HudMode::Nav {
        pitch_ladder(c, f, fpm, h, down);
    }
    if !hover {
        flight_path_marker(c, fpm, h, down, caged);
    }
    // Gun cross at the boresight.
    for d in [Vec2::X, Vec2::Y] {
        c.stroke(Vec2::new(CX, CY) + d * 5.0, Vec2::new(CX, CY) + d * 15.0, PEN, INK);
        c.stroke(Vec2::new(CX, CY) - d * 5.0, Vec2::new(CX, CY) - d * 15.0, PEN, INK);
    }
    if f.camera_sight {
        let p = Vec2::new(CX, CY + 176.0);
        c.stroke(p - Vec2::X * 14.0, p + Vec2::X * 14.0, THIN, INK);
        c.stroke(p - Vec2::Y * 10.0, p + Vec2::Y * 10.0, THIN, INK);
    }

    airspeed(c, f);
    altitude(c, f);
    heading(c, f);
    if st.declutter == 0 {
        bank_scale(c, down, f);
    }
    if let Some(heli) = &f.heli {
        helicopter(c, f, heli);
    } else {
        if f.gear_down {
            aoa_bracket(c, fpm, f.aoa);
        }
        load_and_mach(c, f);
    }

    // Lower left: mode, stores, gun.
    let bomb = f.cues.bomb.filter(|_| st.mode == HudMode::Gnd);
    let mode = match st.mode {
        HudMode::Nav if f.ils.is_some() => "ILS",
        HudMode::Nav => "NAV",
        HudMode::Air => "AA",
        HudMode::Gnd => match bomb {
            Some(b) if b.ccrp => "CCRP",
            Some(_) => "CCIP",
            None => "AG",
        },
    };
    text(c, 270.0, 392.0, mode, Align::Left);
    // A turret gun with what it is slaved to.
    let gun = |rounds: u32| match f.gun_slave {
        Some(s) => format!("GUN {rounds} {s}"),
        None => format!("GUN {rounds}"),
    };
    if f.gun_selected {
        text(c, 270.0, 418.0, &gun(f.gun_rounds.unwrap_or(0)), Align::Left);
    } else {
        if let Some((count, name)) = &f.weapon {
            text(c, 270.0, 418.0, &format!("{count} {name}"), Align::Left);
        }
        if let Some(rounds) = f.gun_rounds {
            text(c, 270.0, 444.0, &gun(rounds), Align::Left);
        }
    }

    // Lower right: steerpoint number, distance and time to go; with a bomb the release data.
    if let Some(b) = bomb {
        bomb_readouts(c, &b);
    } else if let Some((n, km, secs)) = f.steerpoint {
        text(c, 1010.0, 392.0, &format!("STPT {n}"), Align::Right);
        text(c, 1010.0, 418.0, &format!("{km:.1} KM"), Align::Right);
        let s = secs.clamp(0.0, 359_999.0) as i32;
        let t = if s >= 3600 {
            format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
        } else {
            format!("{:02}:{:02}", s / 60, s % 60)
        };
        text(c, 1010.0, 444.0, &t, Align::Right);
    }
    if st.mode == HudMode::Nav
        && let Some(d) = f.steerpoint_dir
    {
        steerpoint_cue(c, inv * d, f.scale);
    }
    // Targeting pod line of sight: a small circle, in every mode.
    if let Some(p) = f.tgp_dir.and_then(|d| project(inv * d, f.scale)).filter(|&p| inside(p)) {
        c.ring(p, 8.0, THIN, INK);
        c.circle(p.x, p.y, 1.5, INK);
    }

    match st.mode {
        HudMode::Nav => {}
        HudMode::Air => air_to_air(c, f),
        HudMode::Gnd => air_to_ground(c, f, fpm, down),
    }
    gun_sight(c, f);
    if let Some((loc, gs)) = f.ils {
        ils_bars(c, fpm, h, down, loc, gs);
    }

    // Messages and status words.
    if let Some(m) = &f.message {
        text(c, CX, 105.0, m, Align::Centre);
    }
    let status: Vec<&str> = [(f.accel, "ACCEL"), (f.training, "TRAINING")]
        .iter()
        .filter(|s| s.0)
        .map(|s| s.1)
        .collect();
    if !status.is_empty() {
        text(c, CX, 131.0, &status.join("  "), Align::Centre);
    }
    if let Some(r) = &f.cues.recon {
        text(c, CX, 157.0, &r.text("TGP"), Align::Centre);
    }
}

/// Upper left: load factor, Mach, maximum load factor.
fn load_and_mach(c: &mut Canvas, f: &HudFrame) {
    let g = f.gees as f32 / 16.0;
    text(c, 300.0, 100.0, &format!("{g:.1}"), Align::Left);
    let mach = if f.mach < 1.0 {
        format!(".{:02}", (f.mach * 100.0).round().min(99.0) as i32)
    } else {
        format!("{:.2}", f.mach)
    };
    text(c, 300.0, 126.0, &mach, Align::Left);
    text(c, 300.0, 152.0, &format!("{:.1}", f.state.max_gees as f32 / 16.0), Align::Left);
}

/// Below this airspeed a helicopter's HUD shows the hover drift vector, not the flight path.
const HOVER_KNOTS: i32 = 40;
/// Drift vector: pixels per knot, from its circle below the boresight.
const DRIFT_SCALE: f32 = 4.0;
const DRIFT_CENTRE: Vec2 = Vec2::new(CX, CY + 110.0);

/// A helicopter's readouts: torque and rotor speed (upper left), the radar altitude (under the
/// altitude scale) and, slow, the drift over the ground from a circle below the boresight.
fn helicopter(c: &mut Canvas, f: &HudFrame, h: &super::HeliHud) {
    text(c, 300.0, 100.0, &format!("TRQ {:.0}", h.torque), Align::Left);
    text(c, 300.0, 126.0, &format!("NR {:.0}", h.rotor), Align::Left);
    if h.rotor < 95.0 && f.blink {
        text(c, 300.0, 152.0, "LOW ROTOR", Align::Left);
    }
    if f.knots < HOVER_KNOTS {
        c.ring(DRIFT_CENTRE, 6.0, PEN, INK);
        let end =
            (DRIFT_CENTRE + Vec2::new(h.drift.x, -h.drift.y) * DRIFT_SCALE).clamp(FIELD.0, FIELD.1);
        if end.distance(DRIFT_CENTRE) > 8.0 {
            c.stroke(DRIFT_CENTRE, end, PEN, INK);
            c.ring(end, 4.0, THIN, INK);
        }
    }
}

fn flight_path_marker(c: &mut Canvas, p: Vec2, h: Vec2, down: Vec2, caged: bool) {
    let r = 9.0;
    c.ring(p, r, PEN, INK);
    c.stroke(p + h * r, p + h * (r + 17.0), PEN, INK);
    c.stroke(p - h * r, p - h * (r + 17.0), PEN, INK);
    c.stroke(p - down * r, p - down * (r + 9.0), PEN, INK);
    if caged {
        c.stroke(p + Vec2::new(-r, -r), p + Vec2::new(r, r), THIN, INK);
        c.stroke(p + Vec2::new(-r, r), p + Vec2::new(r, -r), THIN, INK);
    }
}

/// Pitch ladder centred on the flight path: a rung every 5 degrees, solid above the horizon
/// with tips pointing down to it, dashed below with tips up; the horizon line is longer.
fn pitch_ladder(c: &mut Canvas, f: &HudFrame, fpm: Vec2, h: Vec2, down: Vec2) {
    let inv = f.orientation.inverse();
    let track = Vec3::new(f.path_dir.x, 0.0, f.path_dir.z)
        .try_normalize()
        .or_else(|| {
            let n = f.orientation * Vec3::NEG_Z;
            Vec3::new(n.x, 0.0, n.z).try_normalize()
        })
        .unwrap_or(Vec3::NEG_Z);
    let gamma = f.path_dir.y.clamp(-1.0, 1.0).asin().to_degrees();
    c.set_clip(
        FIELD.0.x as i32 - 20,
        FIELD.0.y as i32 - 10,
        FIELD.1.x as i32 + 20,
        FIELD.1.y as i32 + 10,
    );
    for k in -18..=18 {
        let deg = k as f32 * 5.0;
        if (deg - gamma).abs() > 22.0 || deg.abs() > 89.0 {
            continue;
        }
        let rad = deg.to_radians();
        let dir = track * rad.cos() + Vec3::Y * rad.sin();
        let Some(p) = project(inv * dir, f.scale) else { continue };
        // Centre the rung laterally on the flight path marker (along the horizon direction).
        let p = p + h * (fpm - p).dot(h);
        if k == 0 {
            c.stroke(p - h * 330.0, p - h * 40.0, PEN, INK);
            c.stroke(p + h * 40.0, p + h * 330.0, PEN, INK);
            continue;
        }
        let (gap, len, tip) = (28.0, 72.0, 12.0);
        let tip_dir = if k > 0 { down } else { -down };
        for side in [-1.0f32, 1.0] {
            let a = p + h * side * gap;
            let b = p + h * side * (gap + len);
            if k > 0 {
                c.stroke(a, b, PEN, INK);
            } else {
                // Below the horizon the rungs bend away from it by half the angle (F-16).
                let bend = (deg.abs() * 0.5).to_radians().tan() * len;
                c.dashed(a, b + tip_dir * bend, 12.0, 8.0, PEN, INK);
            }
            let end =
                if k > 0 { b } else { b + tip_dir * (deg.abs() * 0.5).to_radians().tan() * len };
            c.stroke(a, a + tip_dir * tip, PEN, INK);
            let label = format!("{}", deg.abs() as i32);
            let lp = end + h * side * 26.0 + tip_dir * 2.0;
            small(c, lp.x, lp.y, &label, Align::Centre);
        }
    }
    c.reset_clip();
}

/// Airspeed scale (left): 2 pixels per knot, ticks every 10 kt, labels every 50 kt, and the
/// boxed readout inside the scale.
fn airspeed(c: &mut Canvas, f: &HudFrame) {
    let kt = f.knots.max(0) as f32;
    let (x, half) = (300.0, 120.0);
    if f.state.declutter == 0 {
        c.set_clip(0, (CY - half) as i32, WIDTH as i32 - 1, (CY + half) as i32);
        let first = ((kt - 60.0) / 10.0).floor() as i32 * 10;
        for v in (first..first + 130).step_by(10) {
            if v < 0 {
                continue;
            }
            let y = CY - (v as f32 - kt) * 2.0;
            let long = v % 50 == 0;
            c.stroke(Vec2::new(x, y), Vec2::new(x - if long { 14.0 } else { 8.0 }, y), THIN, INK);
            if long {
                small(c, x - 20.0, y, &v.to_string(), Align::Right);
            }
        }
        c.reset_clip();
        c.polyline(
            &[
                Vec2::new(x + 4.0, CY),
                Vec2::new(x + 12.0, CY - 6.0),
                Vec2::new(x + 12.0, CY + 6.0),
                Vec2::new(x + 4.0, CY),
            ],
            THIN,
            INK,
        );
    }
    boxed(c, x + 22.0, CY, &format!("{}", kt.round() as i32), 3, Align::Left);
}

/// Altitude scale (right): 0.2 pixels per foot, ticks every 100 ft, labels every 500 ft
/// ("12,5" = 12,500), the boxed readout and the radar altitude below it when low.
fn altitude(c: &mut Canvas, f: &HudFrame) {
    let alt = f.alt.max(0) as f32;
    let (x, half) = (980.0, 120.0);
    if f.state.declutter == 0 {
        c.set_clip(0, (CY - half) as i32, WIDTH as i32 - 1, (CY + half) as i32);
        let first = ((alt - 600.0) / 100.0).floor() as i32 * 100;
        for v in (first..first + 1300).step_by(100) {
            if v < 0 {
                continue;
            }
            let y = CY - (v as f32 - alt) * 0.2;
            let long = v % 500 == 0;
            c.stroke(Vec2::new(x, y), Vec2::new(x + if long { 14.0 } else { 8.0 }, y), THIN, INK);
            if long {
                small(c, x + 20.0, y, &format!("{},{}", v / 1000, v % 1000 / 100), Align::Left);
            }
        }
        c.reset_clip();
        c.polyline(
            &[
                Vec2::new(x - 4.0, CY),
                Vec2::new(x - 12.0, CY - 6.0),
                Vec2::new(x - 12.0, CY + 6.0),
                Vec2::new(x - 4.0, CY),
            ],
            THIN,
            INK,
        );
    }
    let a = (alt / 10.0).round() as i32 * 10;
    let s = if a >= 1000 { format!("{},{:03}", a / 1000, a % 1000) } else { a.to_string() };
    boxed(c, x - 22.0, CY, &s, 6, Align::Right);
    if f.radar_alt < 5000.0 {
        text(
            c,
            x - 22.0,
            CY + 40.0,
            &format!("R {:05}", (f.radar_alt / 10.0).round() as i32 * 10),
            Align::Right,
        );
    }
    // Climb rate (feet per minute) above the readout.
    let vvi = (f.climb as f32 * 60.0 / 10.0).round() as i32 * 10;
    if vvi.abs() >= 100 {
        small(c, x - 22.0, CY - 36.0, &format!("{vvi:+}"), Align::Right);
    }
}

/// Heading tape along the top: 6 pixels per degree, labels every 10 degrees in tens.
fn heading(c: &mut Canvas, f: &HudFrame) {
    let hdg = f.heading as f32 / 65536.0 * 360.0;
    let (y, half) = (58.0, 170.0);
    if f.state.declutter == 2 {
        boxed(c, CX, y - 10.0, &format!("{:03}", hdg.round() as i32 % 360), 3, Align::Centre);
        return;
    }
    c.set_clip((CX - half) as i32, 0, (CX + half) as i32, HEIGHT as i32 - 1);
    let first = ((hdg - 32.0) / 5.0).floor() as i32 * 5;
    for v in (first..first + 70).step_by(5) {
        let x = CX + (v as f32 - hdg) * 6.0;
        let long = v.rem_euclid(10) == 0;
        c.stroke(Vec2::new(x, y), Vec2::new(x, y - if long { 12.0 } else { 6.0 }), THIN, INK);
        if long {
            small(c, x, y - 24.0, &format!("{:02}", v.rem_euclid(360) / 10), Align::Centre);
        }
    }
    // Steerpoint bearing caret.
    if let Some(brg) = f.waypoint_bearing {
        let d = ((brg as f32 / 65536.0 * 360.0 - hdg + 540.0).rem_euclid(360.0)) - 180.0;
        let x = CX + d.clamp(-28.0, 28.0) * 6.0;
        c.polyline(
            &[Vec2::new(x - 6.0, y + 12.0), Vec2::new(x, y + 4.0), Vec2::new(x + 6.0, y + 12.0)],
            PEN,
            INK,
        );
    }
    c.reset_clip();
    c.stroke(Vec2::new(CX, y + 2.0), Vec2::new(CX, y + 14.0), PEN, INK);
}

/// Bank angle scale under the boresight: ticks at 10, 20, 30 and 45 degrees each way and a
/// pointer that stays towards the ground.
fn bank_scale(c: &mut Canvas, down: Vec2, f: &HudFrame) {
    let centre = Vec2::new(CX, CY);
    let r = 165.0;
    for deg in [0.0f32, 10.0, 20.0, 30.0, 45.0] {
        for s in [-1.0f32, 1.0] {
            let a = (90.0 + s * deg).to_radians();
            let d = Vec2::new(a.cos(), a.sin());
            let len = if deg == 0.0 || deg == 30.0 { 12.0 } else { 7.0 };
            c.stroke(centre + d * r, centre + d * (r + len), THIN, INK);
        }
    }
    let (_, _, roll) = euler(f);
    if roll.abs() < 1.2 {
        let p = centre + down * (r - 4.0);
        let side = Vec2::new(-down.y, down.x);
        c.polyline(&[p, p - down * 10.0 + side * 6.0, p - down * 10.0 - side * 6.0, p], THIN, INK);
    }
}

/// Approach (on speed) angle of attack in degrees: the middle of the HUD's AOA bracket and
/// the approach indexer's green circle.
pub const APPROACH_AOA: f32 = 10.0;

/// Approach angle-of-attack bracket left of the flight path marker (gear down): centred on
/// the marker at the approach angle of attack, marker high in the bracket = fast.
fn aoa_bracket(c: &mut Canvas, fpm: Vec2, aoa: f32) {
    let x = fpm.x - 48.0;
    let y = fpm.y - (aoa - APPROACH_AOA) * 8.0;
    let half = 18.0;
    c.polyline(
        &[
            Vec2::new(x + 6.0, y - half),
            Vec2::new(x, y - half),
            Vec2::new(x, y + half),
            Vec2::new(x + 6.0, y + half),
        ],
        PEN,
        INK,
    );
}

/// AA: target designator box with the missile diamond when locked, SHOOT cue in range,
/// allowable steering error circle, launch zone scale and the gun pipper.
fn air_to_air(c: &mut Canvas, f: &HudFrame) {
    let cues = &f.cues;
    let target = cues.target_dir.and_then(|d| project(d, f.scale));
    if let Some(p) = target {
        let q = p.clamp(FIELD.0, FIELD.1);
        let s = 20.0;
        c.frame(q - Vec2::splat(s), q + Vec2::splat(s), PEN, INK);
        if q != p {
            c.stroke(q - Vec2::splat(s), q + Vec2::splat(s), THIN, INK);
            c.stroke(q + Vec2::new(-s, s), q + Vec2::new(s, -s), THIN, INK);
        }
        if cues.lock {
            diamond(c, q, 10.0);
        }
        if let Some(r) = f.target_range_km {
            small(c, q.x, q.y + s + 16.0, &format!("{r:.1}"), Align::Centre);
        }
    }
    if f.weapon_air_to_air {
        // Allowable steering error circle and the steering dot towards the target.
        let ase = 95.0;
        c.ring(Vec2::new(CX, CY), ase, THIN, INK);
        if let Some(p) = target {
            let d = p - Vec2::new(CX, CY);
            let dot = Vec2::new(CX, CY) + d.clamp_length_max(ase);
            c.ring(dot, 4.0, PEN * 1.5, INK);
        }
        let cue = match cues.launch_zone {
            Some(z) if z.shoot => f.blink.then_some("SHOOT"),
            Some(z) if z.too_close => Some("TOO CLOSE"),
            Some(_) => None,
            None => (cues.lock && cues.in_range && f.blink).then_some("SHOOT"),
        };
        if let Some(s) = cue {
            text(c, CX, CY - ase - 22.0, s, Align::Centre);
        }
        match &cues.launch_zone {
            Some(z) => dynamic_launch_zone(c, z),
            None => launch_zone(c, f),
        }
    }
}

/// ILS deviations at full scale (degrees): localizer and glideslope.
const ILS_FULL_SCALE: (f32, f32) = (2.5, 0.7);
/// Bar offset at full scale, bar half length, and the ladder ticks on each bar: their distance
/// from its middle and half length (pixels; the F-16's bars are about 5.5 flight path marker
/// radii long either side, ticks at 60 %).
const ILS_DEFLECTION: f32 = 60.0;
const ILS_BAR: f32 = 50.0;
const ILS_TICK: (f32, f32) = (30.0, 7.0);

/// ILS steering, as the F-16's: the localizer deviation bar (upright) and the glideslope
/// deviation bar (level), each with two short ladder ticks, referenced to the flight path
/// marker. Fly the marker to where they cross; each stops at full-scale deflection.
fn ils_bars(c: &mut Canvas, fpm: Vec2, h: Vec2, down: Vec2, loc: f32, gs: f32) {
    // Right of the centreline: the course is left. Above the glide path: it is below.
    let x = -(loc / ILS_FULL_SCALE.0).clamp(-1.0, 1.0) * ILS_DEFLECTION;
    let y = (gs / ILS_FULL_SCALE.1).clamp(-1.0, 1.0) * ILS_DEFLECTION;
    for (mid, along, across) in [(fpm + h * x, down, h), (fpm + down * y, h, down)] {
        c.stroke(mid - along * ILS_BAR, mid + along * ILS_BAR, PEN, INK);
        for side in [-1.0, 1.0] {
            let t = mid + along * ILS_TICK.0 * side;
            c.stroke(t - across * ILS_TICK.1, t + across * ILS_TICK.1, PEN, INK);
        }
    }
}

/// Gun selected: the pipper (where the target has to be for the rounds fired now to hit it),
/// with a locked target's range as an arc unwinding from 12 o'clock (a full circle is
/// 12,000 ft), the range in km under it and SHOOT within the gun's range.
fn gun_sight(c: &mut Canvas, f: &HudFrame) {
    let Some(g) = f.cues.gun else { return };
    let Some(p) = project(g.dir, f.scale) else { return };
    let q = p.clamp(FIELD.0, FIELD.1);
    let r = 28.0;
    c.ring(q, r, PEN, INK);
    c.ring(q, 1.5, PEN * 1.4, INK);
    if q != p {
        c.stroke(q - Vec2::splat(r * 0.7), q + Vec2::splat(r * 0.7), THIN, INK);
        c.stroke(q + Vec2::new(-r * 0.7, r * 0.7), q + Vec2::new(r * 0.7, -r * 0.7), THIN, INK);
    }
    let Some(m) = g.range_m else { return };
    let sweep = (m / 3657.6).clamp(0.0, 1.0) * std::f32::consts::TAU;
    let n = ((sweep / std::f32::consts::TAU) * 48.0).ceil().max(1.0) as usize;
    let arc: Vec<Vec2> = (0..=n)
        .map(|k| sweep * k as f32 / n as f32)
        .map(|a| q + Vec2::new(a.sin(), -a.cos()) * (r + 6.0))
        .collect();
    c.polyline(&arc, PEN * 1.3, INK);
    small(c, q.x, q.y + r + 22.0, &format!("{:.1}", m / 1000.0), Align::Centre);
    if g.in_range && f.blink {
        text(c, q.x, q.y - r - 22.0, "SHOOT", Align::Centre);
    }
}

/// Launch zone right of the boresight without a locked target's zone: the weapon's nominal
/// maximum and minimum range and the target range caret.
fn launch_zone(c: &mut Canvas, f: &HudFrame) {
    let Some(rmax) = f.weapon_range_km.filter(|r| *r > 0.0) else { return };
    let (x, top, bottom) = (880.0, CY - 100.0, CY + 100.0);
    let full = rmax * 1.25;
    let y = |r: f32| bottom - (r / full).clamp(0.0, 1.0) * (bottom - top);
    c.stroke(Vec2::new(x, top), Vec2::new(x, bottom), THIN, INK);
    c.stroke(Vec2::new(x, y(rmax)), Vec2::new(x + 12.0, y(rmax)), PEN, INK);
    c.stroke(Vec2::new(x, y(rmax * 0.12)), Vec2::new(x + 12.0, y(rmax * 0.12)), PEN, INK);
    small(c, x + 16.0, top - 14.0, &format!("{:.0}", full), Align::Centre);
    if let Some(r) = f.target_range_km {
        let yy = y(r);
        c.polyline(
            &[
                Vec2::new(x - 2.0, yy),
                Vec2::new(x - 12.0, yy - 6.0),
                Vec2::new(x - 12.0, yy + 6.0),
                Vec2::new(x - 2.0, yy),
            ],
            PEN,
            INK,
        );
    }
}

/// Dynamic launch zone right of the boresight, clear of the altitude readout (F-16 style): a
/// line from the shortest range up to the longest with ticks at both ends, the no-escape part
/// a box on it (AA), and the target range caret left of it. The line fills the scale unless
/// the target is out of range: then it shrinks to leave the caret above it.
fn dynamic_launch_zone(c: &mut Canvas, z: &super::LaunchZoneCue) {
    let Some((lo, hi)) = z.zone_km else { return };
    let (x, top, bottom) = (820.0, CY - 100.0, CY + 100.0);
    let full = hi.max(z.range_km * 1.1).max(0.1);
    let y = |r: f32| bottom - (r / full).clamp(0.0, 1.0) * (bottom - top);
    let (a, b) = (y(hi), y(lo));
    c.polyline(
        &[Vec2::new(x + 10.0, a), Vec2::new(x, a), Vec2::new(x, b), Vec2::new(x + 10.0, b)],
        PEN,
        INK,
    );
    if let Some(ne) = z.no_escape_km {
        c.frame(Vec2::new(x, y(ne)), Vec2::new(x + 10.0, b), PEN, INK);
    }
    let yy = y(z.range_km);
    c.polyline(
        &[
            Vec2::new(x - 2.0, yy),
            Vec2::new(x - 12.0, yy - 6.0),
            Vec2::new(x - 12.0, yy + 6.0),
            Vec2::new(x - 2.0, yy),
        ],
        PEN,
        INK,
    );
}

/// "m:ss" of seconds (0 and up).
fn min_sec(secs: f32) -> String {
    let s = secs.max(0.0).round() as i32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Bomb readouts in the lower right: the slant range (nautical miles, F) to the impact point
/// (CCIP) or the target (CCRP), the time to release (CCRP, REL) and the bomb's time of fall
/// (TOF).
fn bomb_readouts(c: &mut Canvas, b: &super::BombCue) {
    let range = b.range_m.map_or("F---.-".to_string(), |m| format!("F{:05.1}", m / 1852.0));
    text(c, 1010.0, 392.0, &range, Align::Right);
    if b.ccrp {
        let rel = b.release_secs.map_or("--:--".to_string(), min_sec);
        text(c, 1010.0, 418.0, &format!("REL {rel}"), Align::Right);
    }
    text(
        c,
        1010.0,
        if b.ccrp { 444.0 } else { 418.0 },
        &format!("TOF {:.1}", b.fall_secs),
        Align::Right,
    );
}

/// Pixels per second to the release the CCRP solution cue sits above the flight path marker.
const SOLUTION_PX_PER_S: f32 = 10.0;

/// The part of the line through `p` along `d` inside the field.
fn field_line(p: Vec2, d: Vec2) -> Option<(Vec2, Vec2)> {
    let (mut t0, mut t1) = (-1e4f32, 1e4f32);
    for a in 0..2 {
        let (lo, hi, o, v) = (FIELD.0[a], FIELD.1[a], p[a], d[a]);
        if v.abs() < 1e-6 {
            if o < lo || o > hi {
                return None;
            }
        } else {
            let (a0, a1) = ((lo - o) / v, (hi - o) / v);
            t0 = t0.max(a0.min(a1));
            t1 = t1.min(a0.max(a1));
        }
    }
    (t0 < t1).then(|| (p + d * t0, p + d * t1))
}

/// AG, F-16 style: the target designator box; with a free-fall or retarded bomb in CCIP the
/// pipper on the bomb fall line from the flight path marker (a delay cue across the line while
/// the impact point is below the field), in CCRP the azimuth steering line through the target
/// with the solution cue sliding down it to the flight path marker (the bomb goes when the
/// cue reaches the marker with the button held); for the guided release (flight-path guide)
/// the steering line with the release cue.
fn air_to_ground(c: &mut Canvas, f: &HudFrame, fpm: Vec2, down: Vec2) {
    let cues = &f.cues;
    if let Some(p) = cues.target_dir.and_then(|d| project(d, f.scale)) {
        let q = p.clamp(FIELD.0, FIELD.1);
        let s = 16.0;
        c.frame(q - Vec2::splat(s), q + Vec2::splat(s), PEN, INK);
        c.ring(q, 1.5, PEN, INK);
        // A zone against the slewed pod point has its cue by the zone scale instead.
        match cues.launch_zone.filter(|z| z.at_lock) {
            Some(z) if z.shoot => {
                if f.blink {
                    small(c, q.x, q.y - s - 14.0, "SHOOT", Align::Centre);
                }
            }
            Some(z) if z.too_close => small(c, q.x, q.y - s - 14.0, "TOO CLOSE", Align::Centre),
            Some(_) => {}
            None if cues.lock && cues.in_range => {
                small(c, q.x, q.y - s - 14.0, "IN RNG", Align::Centre)
            }
            None => {}
        }
        if let Some(r) = f.target_range_km {
            small(c, q.x, q.y + s + 16.0, &format!("{r:.1}"), Align::Centre);
        }
    }
    if let Some(z) = &cues.launch_zone {
        dynamic_launch_zone(c, z);
        let cue =
            if z.shoot { f.blink.then_some("SHOOT") } else { z.too_close.then_some("TOO CLOSE") };
        if let Some(s) = cue.filter(|_| !z.at_lock || cues.target_dir.is_none()) {
            small(c, 820.0, CY + 124.0, s, Align::Centre);
        }
    }
    let bomb = cues.bomb;
    if let Some(dir) = cues.ccip_dir.filter(|_| bomb.is_none_or(|b| !b.ccrp)) {
        // The bomb fall line from the flight path marker to the pipper, a circle with a dot on
        // the impact point; while that is below the field the pipper stays on the line at the
        // field's edge and the time delay cue crosses the line there.
        let r = 10.0;
        let p = project(dir, f.scale).unwrap_or(Vec2::new(CX, FIELD.1.y));
        let along = (p - fpm).normalize_or(Vec2::Y);
        let q = if inside(p) {
            p
        } else {
            field_line(fpm, along).map_or(p.clamp(FIELD.0, FIELD.1), |(_, e)| e - along * r)
        };
        if (q - fpm).dot(along) > r + 9.0 {
            c.stroke(fpm + along * 9.0, q - along * r, PEN, INK);
        }
        if q != p {
            let across = along.perp();
            let tdc = q - along * (r + 24.0);
            c.stroke(tdc - across * 16.0, tdc + across * 16.0, PEN, INK);
        }
        c.ring(q, r, PEN, INK);
        c.ring(q, 1.5, PEN * 1.4, INK);
    }
    if let Some(b) = bomb.filter(|b| b.ccrp)
        && let Some(p) = cues.target_dir.and_then(|d| project(d, f.scale))
    {
        let q = p.clamp(FIELD.0, FIELD.1);
        if let Some((a, e)) = field_line(q, down) {
            c.stroke(a, e, PEN, INK);
        }
        if let Some(secs) = b.release_secs {
            // On the steering line, level with the flight path marker at release.
            let base = q + down * (fpm - q).dot(down);
            // It waits at the top of the line until the release comes within reach.
            let mut cue = base - down * secs.max(0.0) * SOLUTION_PX_PER_S;
            if let Some((a, e)) = field_line(q, down) {
                let top = if a.y < e.y { a } else { e };
                if (cue - top).dot(down) < 0.0 {
                    cue = top + down * 6.0;
                }
            }
            let h = down.perp();
            c.stroke(cue - h * 24.0, cue + h * 24.0, PEN * 1.3, INK);
        }
    }
    if let Some(p) = cues.diamond_dir.and_then(|d| project(d, f.scale)) {
        // Azimuth steering line through the release point, release cue sliding down to the
        // flight path marker.
        let x = p.x.clamp(FIELD.0.x, FIELD.1.x);
        c.stroke(Vec2::new(x, FIELD.0.y + 40.0), Vec2::new(x, FIELD.1.y - 40.0), THIN, INK);
        if let Some((bar, flash)) = cues.ranging_bar
            && (!flash || f.blink)
        {
            let y = (fpm.y - bar as f32 * 4.0).max(FIELD.0.y);
            c.stroke(Vec2::new(fpm.x - 22.0, y), Vec2::new(fpm.x + 22.0, y), PEN * 1.3, INK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::HudState;

    fn frame(mode: HudMode) -> HudFrame {
        HudFrame {
            scale: SCALE,
            state: HudState { mode, ..Default::default() },
            knots: 420,
            alt: 12_480,
            gees: 16,
            orientation: Quat::IDENTITY,
            path_dir: Vec3::new(0.0, -0.03, -1.0).normalize(),
            mach: 0.66,
            aoa: 3.0,
            radar_alt: 2000.0,
            steerpoint: Some((2, 12.4, 200.0)),
            steerpoint_dir: Some(Vec3::new(0.2, -0.3, -1.0)),
            waypoint_bearing: Some(0x400),
            weapon: Some((2, "MAVERICK".into())),
            gun_rounds: Some(650),
            weapon_range_km: Some(8.0),
            target_range_km: Some(5.0),
            ..Default::default()
        }
    }

    #[test]
    fn draws_every_mode() {
        for mode in [HudMode::Nav, HudMode::Air, HudMode::Gnd] {
            for declutter in 0..3 {
                let mut f = frame(mode);
                f.state.declutter = declutter;
                f.cues.target_dir = Some(Vec3::new(0.05, 0.02, -1.0));
                f.cues.ccip_dir = Some(Vec3::new(0.0, -0.4, -1.0));
                f.cues.diamond_dir = Some(Vec3::new(0.01, -0.1, -1.0));
                f.cues.ranging_bar = Some((10, false));
                f.weapon_air_to_air = mode == HudMode::Air;
                let mut c = Canvas::new(WIDTH, HEIGHT);
                draw(&mut c, &f);
                let ink = c.px.iter().filter(|&&p| p != 0).count();
                assert!(ink > 2000, "{mode:?} {declutter}");
                // The gun pipper with its range arc adds ink in every mode.
                f.gun_selected = true;
                let mut c = Canvas::new(WIDTH, HEIGHT);
                draw(&mut c, &f);
                let ink = c.px.iter().filter(|&&p| p != 0).count();
                f.cues.gun = Some(crate::hud::GunCue {
                    dir: Vec3::new(0.0, -0.01, -1.0),
                    screen: None,
                    range_m: Some(900.0),
                    in_range: true,
                });
                let mut c = Canvas::new(WIDTH, HEIGHT);
                draw(&mut c, &f);
                assert!(c.px.iter().filter(|&&p| p != 0).count() > ink, "{mode:?} {declutter} gun");
            }
        }
    }

    #[test]
    fn draws_the_bombsights() {
        let ink = |f: &HudFrame| {
            let mut c = Canvas::new(WIDTH, HEIGHT);
            draw(&mut c, f);
            c.px.iter().filter(|&&p| p != 0).count()
        };
        let mut f = frame(HudMode::Gnd);
        f.cues.target_dir = Some(Vec3::new(0.05, -0.2, -1.0));
        let plain = ink(&f);
        // CCIP: the pipper on its fall line.
        f.cues.ccip_dir = Some(Vec3::new(0.0, -0.3, -1.0));
        f.cues.bomb = Some(crate::hud::BombCue {
            ccrp: false,
            fall_secs: 5.5,
            range_m: Some(1200.0),
            release_secs: None,
        });
        let ccip = ink(&f);
        assert!(ccip > plain);
        // CCRP: no pipper, the steering line and the solution cue instead.
        f.cues.bomb = Some(crate::hud::BombCue {
            ccrp: true,
            fall_secs: 5.5,
            range_m: Some(4000.0),
            release_secs: None,
        });
        let asl = ink(&f);
        f.cues.bomb = Some(crate::hud::BombCue {
            ccrp: true,
            fall_secs: 5.5,
            range_m: Some(4000.0),
            release_secs: Some(8.0),
        });
        assert!(ink(&f) > asl, "solution cue");
    }

    #[test]
    fn draws_the_air_to_ground_launch_zone() {
        let ink = |f: &HudFrame| {
            let mut c = Canvas::new(WIDTH, HEIGHT);
            draw(&mut c, f);
            c.px.iter().filter(|&&p| p != 0).count()
        };
        let mut f = frame(HudMode::Gnd);
        f.blink = true;
        f.cues.target_dir = Some(Vec3::new(0.05, -0.2, -1.0));
        let plain = ink(&f);
        let mut z = crate::hud::LaunchZoneCue {
            range_km: 20.0,
            zone_km: None,
            no_escape_km: None,
            shoot: false,
            too_close: false,
            at_lock: true,
        };
        // No zone: nothing to draw.
        f.cues.launch_zone = Some(z);
        assert_eq!(ink(&f), plain);
        z.zone_km = Some((3.0, 25.0));
        f.cues.launch_zone = Some(z);
        let zone = ink(&f);
        assert!(zone > plain);
        z.shoot = true;
        f.cues.launch_zone = Some(z);
        assert!(ink(&f) > zone, "SHOOT");
        // Against a pod point slewed off the lock: SHOOT by the zone scale, not over the box.
        let lower_right = |f: &HudFrame| {
            let mut c = Canvas::new(WIDTH, HEIGHT);
            draw(&mut c, f);
            let rows = (CY + 110.0) as usize..(CY + 140.0) as usize;
            rows.flat_map(|y| (760..880).map(move |x| (x, y)))
                .filter(|&(x, y)| c.px[y * WIDTH + x] != 0)
                .count()
        };
        let at_box = lower_right(&f);
        z.at_lock = false;
        f.cues.launch_zone = Some(z);
        assert!(lower_right(&f) > at_box, "SHOOT by the scale");
        assert_eq!(ink(&f) - lower_right(&f), zone - at_box, "no SHOOT over the box");
        z.at_lock = true;
        z.shoot = false;
        z.too_close = true;
        z.range_km = 1.0;
        f.cues.launch_zone = Some(z);
        assert!(ink(&f) > zone, "TOO CLOSE");
    }

    #[test]
    fn draws_the_air_to_air_launch_zone() {
        let ink = |f: &HudFrame| {
            let mut c = Canvas::new(WIDTH, HEIGHT);
            draw(&mut c, f);
            c.px.iter().filter(|&&p| p != 0).count()
        };
        let mut f = frame(HudMode::Air);
        f.blink = true;
        f.weapon_air_to_air = true;
        f.cues.target_dir = Some(Vec3::new(0.05, 0.02, -1.0));
        let mut z = crate::hud::LaunchZoneCue {
            range_km: 30.0,
            zone_km: Some((2.0, 25.0)),
            no_escape_km: None,
            shoot: false,
            too_close: false,
            at_lock: true,
        };
        f.cues.launch_zone = Some(z);
        let zone = ink(&f);
        z.no_escape_km = Some(12.0);
        f.cues.launch_zone = Some(z);
        let no_escape = ink(&f);
        assert!(no_escape > zone, "no-escape box");
        z.shoot = true;
        z.range_km = 20.0;
        f.cues.launch_zone = Some(z);
        assert!(ink(&f) > no_escape, "SHOOT");
    }

    #[test]
    fn flight_path_marker_sits_on_the_velocity() {
        // 5 degrees below the nose: the marker's ring is 5 degrees under the boresight.
        let mut f = frame(HudMode::Nav);
        f.path_dir = Vec3::new(0.0, -(5f32.to_radians().sin()), -(5f32.to_radians().cos()));
        let p = project(f.orientation.inverse() * f.path_dir, f.scale).unwrap();
        assert!((p.x - CX).abs() < 0.01);
        assert!((p.y - (CY + SCALE * 5f32.to_radians().tan())).abs() < 0.01);
    }
}
