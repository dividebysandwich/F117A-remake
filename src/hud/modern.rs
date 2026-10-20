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
    c.frame(Vec2::new(x0 - 6.0, y - TEXT / 2.0 - 6.0), Vec2::new(x0 + w + 6.0, y + TEXT / 2.0 + 6.0), THIN, INK);
}

fn diamond(c: &mut Canvas, p: Vec2, r: f32) {
    c.polyline(&[p + Vec2::new(0.0, -r), p + Vec2::new(r, 0.0), p + Vec2::new(0.0, r), p + Vec2::new(-r, 0.0), p + Vec2::new(0.0, -r)], PEN, INK);
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
    (fw.x.atan2(-fw.z).rem_euclid(std::f32::consts::TAU), fw.y.clamp(-1.0, 1.0).asin(), (-r.y).atan2(u.y))
}

/// Draws the whole HUD.
pub fn draw(c: &mut Canvas, f: &HudFrame) {
    c.reset_clip();
    let st = &f.state;
    let inv = f.orientation.inverse();
    let (h, down) = horizon_axes(f);

    // Flight path marker (caged to the field with an X when it would leave it).
    let fpm_raw = project(inv * f.path_dir, f.scale);
    let fpm = fpm_raw.map_or(Vec2::new(CX, FIELD.1.y), |p| p.clamp(FIELD.0, FIELD.1));
    let caged = fpm_raw.is_none_or(|p| !inside(p));

    if st.declutter < 2 || st.mode == HudMode::Nav {
        pitch_ladder(c, f, fpm, h, down);
    }
    flight_path_marker(c, fpm, h, down, caged);
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
    if f.gear_down {
        aoa_bracket(c, fpm, f.aoa);
    }

    // Upper left: load factor, Mach, maximum load factor.
    let g = f.gees as f32 / 16.0;
    text(c, 300.0, 100.0, &format!("{g:.1}"), Align::Left);
    let mach = if f.mach < 1.0 { format!(".{:02}", (f.mach * 100.0).round().min(99.0) as i32) } else { format!("{:.2}", f.mach) };
    text(c, 300.0, 126.0, &mach, Align::Left);
    text(c, 300.0, 152.0, &format!("{:.1}", f.state.max_gees as f32 / 16.0), Align::Left);

    // Lower left: mode, stores, gun.
    let mode = match st.mode {
        HudMode::Nav => "NAV",
        HudMode::Air => "AA",
        HudMode::Gnd => "AG",
    };
    text(c, 270.0, 392.0, mode, Align::Left);
    if let Some((count, name)) = &f.weapon {
        text(c, 270.0, 418.0, &format!("{count} {name}"), Align::Left);
    }
    if let Some(rounds) = f.gun_rounds {
        text(c, 270.0, 444.0, &format!("GUN {rounds}"), Align::Left);
    }

    // Lower right: steerpoint number, distance and time to go.
    if let Some((n, km, secs)) = f.steerpoint {
        text(c, 1010.0, 392.0, &format!("STPT {n}"), Align::Right);
        text(c, 1010.0, 418.0, &format!("{km:.1} KM"), Align::Right);
        let s = secs.clamp(0.0, 359_999.0) as i32;
        let t = if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{:02}:{:02}", s / 60, s % 60) };
        text(c, 1010.0, 444.0, &t, Align::Right);
    }
    if st.mode == HudMode::Nav
        && let Some(p) = f.steerpoint_dir.and_then(|d| project(inv * d, f.scale)).filter(|&p| inside(p))
    {
        diamond(c, p, 9.0);
    }

    match st.mode {
        HudMode::Nav => {}
        HudMode::Air => air_to_air(c, f),
        HudMode::Gnd => air_to_ground(c, f, fpm),
    }

    // Messages and status words.
    if let Some(m) = &f.message {
        text(c, CX, 105.0, m, Align::Centre);
    }
    let status: Vec<&str> = [(f.accel, "ACCEL"), (f.training, "TRAINING")].iter().filter(|s| s.0).map(|s| s.1).collect();
    if !status.is_empty() {
        text(c, CX, 131.0, &status.join("  "), Align::Centre);
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
        .or_else(|| { let n = f.orientation * Vec3::NEG_Z; Vec3::new(n.x, 0.0, n.z).try_normalize() })
        .unwrap_or(Vec3::NEG_Z);
    let gamma = f.path_dir.y.clamp(-1.0, 1.0).asin().to_degrees();
    c.set_clip(FIELD.0.x as i32 - 20, FIELD.0.y as i32 - 10, FIELD.1.x as i32 + 20, FIELD.1.y as i32 + 10);
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
            let end = if k > 0 { b } else { b + tip_dir * (deg.abs() * 0.5).to_radians().tan() * len };
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
        c.polyline(&[Vec2::new(x + 4.0, CY), Vec2::new(x + 12.0, CY - 6.0), Vec2::new(x + 12.0, CY + 6.0), Vec2::new(x + 4.0, CY)], THIN, INK);
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
        c.polyline(&[Vec2::new(x - 4.0, CY), Vec2::new(x - 12.0, CY - 6.0), Vec2::new(x - 12.0, CY + 6.0), Vec2::new(x - 4.0, CY)], THIN, INK);
    }
    let a = (alt / 10.0).round() as i32 * 10;
    let s = if a >= 1000 { format!("{},{:03}", a / 1000, a % 1000) } else { a.to_string() };
    boxed(c, x - 22.0, CY, &s, 6, Align::Right);
    if f.radar_alt < 5000.0 {
        text(c, x - 22.0, CY + 40.0, &format!("R {:05}", (f.radar_alt / 10.0).round() as i32 * 10), Align::Right);
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
        c.polyline(&[Vec2::new(x - 6.0, y + 12.0), Vec2::new(x, y + 4.0), Vec2::new(x + 6.0, y + 12.0)], PEN, INK);
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

/// Approach angle-of-attack bracket left of the flight path marker (gear down): centred on
/// the marker at the approach angle of attack, marker high in the bracket = fast.
fn aoa_bracket(c: &mut Canvas, fpm: Vec2, aoa: f32) {
    const ON_SPEED: f32 = 10.0;
    let x = fpm.x - 48.0;
    let y = fpm.y - (aoa - ON_SPEED) * 8.0;
    let half = 18.0;
    c.polyline(&[Vec2::new(x + 6.0, y - half), Vec2::new(x, y - half), Vec2::new(x, y + half), Vec2::new(x + 6.0, y + half)], PEN, INK);
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
        if cues.lock && cues.in_range && f.blink {
            text(c, CX, CY - ase - 22.0, "SHOOT", Align::Centre);
        }
        launch_zone(c, f);
    } else if f.gun_rounds.is_some() {
        let p = Vec2::new(CX + (f.gun_lead.0 >> 4) as f32 * 4.0, CY + (f.gun_lead.1 >> 4) as f32 * 4.0);
        c.ring(p, 30.0, PEN, INK);
        c.ring(p, 1.5, PEN * 1.4, INK);
    }
}

/// Dynamic launch zone right of the boresight: the weapon's maximum and minimum range and the
/// target range caret.
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
        c.polyline(&[Vec2::new(x - 2.0, yy), Vec2::new(x - 12.0, yy - 6.0), Vec2::new(x - 12.0, yy + 6.0), Vec2::new(x - 2.0, yy)], PEN, INK);
    }
}

/// AG: CCIP pipper with the bomb fall line, target designator box, and for the guided
/// release (flight-path guide) the azimuth steering line with the release cue.
fn air_to_ground(c: &mut Canvas, f: &HudFrame, fpm: Vec2) {
    let cues = &f.cues;
    if let Some(p) = cues.target_dir.and_then(|d| project(d, f.scale)) {
        let q = p.clamp(FIELD.0, FIELD.1);
        let s = 16.0;
        c.frame(q - Vec2::splat(s), q + Vec2::splat(s), PEN, INK);
        c.ring(q, 1.5, PEN, INK);
        if cues.lock && cues.in_range {
            small(c, q.x, q.y - s - 14.0, "IN RNG", Align::Centre);
        }
        if let Some(r) = f.target_range_km {
            small(c, q.x, q.y + s + 16.0, &format!("{r:.1}"), Align::Centre);
        }
    }
    if let Some(dir) = cues.ccip_dir {
        let p = project(dir, f.scale).unwrap_or(Vec2::new(CX, FIELD.1.y));
        let q = p.clamp(FIELD.0, FIELD.1);
        c.stroke(fpm, q, THIN, INK);
        if q == p || f.blink {
            c.ring(q, 14.0, PEN, INK);
            c.ring(q, 1.5, PEN * 1.4, INK);
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
                assert!(c.px.iter().filter(|&&p| p != 0).count() > 2000, "{mode:?} {declutter}");
            }
        }
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
