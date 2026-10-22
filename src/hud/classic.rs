//! The classic HUD, drawn into the cockpit picture (320x200 screen coordinates).
//! Two layers: the overlay (texts, flight path marker, bars, sights, readouts) and the
//! instruments (tapes, heading scale, frame, G, pitch ladder).

use crate::canvas::Canvas;
use crate::cockpit::CockpitArt;
use crate::font::{Fonts, upper};

use super::{CX, CY, HUD_COLOURS, HudFrame, HudMode};

/// HUD lines drawn over the 3D view: colour 0 would be see-through there, so black is
/// drawn with colour 5 (black in every flight palette).
fn ink3d(c: u8) -> u8 {
    if c == 0 { 5 } else { c }
}

/// Draws a number left-aligned.
fn number(c: &mut Canvas, fonts: &Fonts, v: i64, x: i32, y: i32, col: u8) {
    fonts.text(c, x, y, v.to_string().as_bytes(), col);
}

/// Draws a number right-aligned at x (starting at x - 4*len).
fn number_right(c: &mut Canvas, fonts: &Fonts, v: i64, x: i32, y: i32, col: u8) {
    let s = v.to_string();
    fonts.text(c, x - 4 * s.len() as i32, y, s.as_bytes(), col);
}

/// Box outline (x1,y1)-(x2,y2) inclusive.
fn frame_box(c: &mut Canvas, x1: i32, y1: i32, x2: i32, y2: i32, col: u8) {
    c.rect(x1, y1, x2 - x1 + 1, y2 - y1 + 1, col);
}

/// sin of a binary angle.
fn bsin(a: i32) -> f32 {
    (a as f32 / 65536.0 * std::f32::consts::TAU).sin()
}

fn bcos(a: i32) -> f32 {
    bsin(a + 0x4000)
}

/// Draws the whole HUD for the cockpit view.
pub fn draw(c: &mut Canvas, art: &CockpitArt, fonts: &Fonts, f: &HudFrame) {
    let (col, col2) = HUD_COLOURS;
    let atlas = &art.sprites;
    // HUD text and lines are clipped to the HUD area of the back page (y 0..90).
    c.set_clip(0, 0, 319, 90);
    overlay(c, atlas, fonts, f, col, col2);
    instruments(c, atlas, fonts, f, col, col2);
    c.reset_clip();
}

/// HUD overlay: texts, flight path marker, bars, sights, digital readouts, waypoint cue,
/// messages.
fn overlay(c: &mut Canvas, atlas: &Canvas, fonts: &Fonts, f: &HudFrame, col: u8, col2: u8) {
    let st = &f.state;
    // Weapon line "n NAME" at (53,85); gun line "GUN nnn" at (239,85).
    if f.gun_selected {
        fonts.text(c, 53, 85, b"GUN", col);
    } else if let Some((count, name)) = &f.weapon {
        fonts.text(c, 53, 85, &upper(&format!("{count} {name}")), col);
    }
    if let Some(rounds) = f.gun_rounds {
        fonts.text(c, 239, 85, &upper(&format!("GUN {rounds}")), col);
    }
    // Flight path marker (NAV/GND): atlas (55,77) 11x7 at (154, y-4), y = 52 + fpm>>6.
    if st.mode != HudMode::Air {
        let y = CY + (f.fpm >> 6);
        if y > 10 && y < 93 {
            c.blit(atlas, 55, 77, 11, 7, 154, y - 4, Some(0), Some(col));
        }
    }
    // Camera sight '+'.
    if f.camera_sight {
        c.line(156, 85, 164, 85, col);
        c.line(160, 82, 160, 88, col);
    }
    if st.declutter == 0 {
        // Stall bar x=72 (colour 42 on night missions, else black) and VVI bar x=247.
        let bar = ink3d(if f.night_mission { 42 } else { 0 });
        let len = ((f.stall_knots - f.knots) * 2 / 5 + 29).clamp(0, 57);
        if len != 0 {
            c.line(72, 81 - len, 72, 81, bar);
        }
        let vy = (CY - (f.climb >> 4)).clamp(20, 81);
        c.line(247, CY, 247, vy, bar);
        // Landing sink-limit caret (gear down, sinking, blinking).
        if f.gear_down && f.blink && f.climb < 0 {
            let y = CY + ((0x80 / LANDING_DIFFICULTY) >> 4);
            c.line(242, y - 2, 244, y, col);
            c.line(242, y + 2, 244, y, col);
        }
    }
    if st.mode == HudMode::Air || f.gun_selected {
        // Gun sight: atlas (38,76) 13x11, colour 15 (with the gun selected, also outside AIR
        // and at the pipper).
        let k = super::classic_zoom(f.scale);
        let x = CX + ((f.gun_lead.0 as f32 * k) as i32 >> 4);
        let y = CY + ((f.gun_lead.1 as f32 * k) as i32 >> 4);
        if x > 10 && x < 309 && y > 8 && y < 86 {
            c.blit(atlas, 38, 76, 13, 11, x - 6, y - 5, Some(0), Some(15));
        }
    }
    if st.mode == HudMode::Air {
        // Missile envelope: 16-segment ellipse 40x35.
        if f.weapon_air_to_air {
            let mut prev: Option<(i32, i32)> = None;
            for k in 0..=16 {
                let a = k << 12;
                let p = (CX + (bsin(a) * 40.0) as i32, CY - (bcos(a) * 35.0) as i32);
                if let Some(q) = prev {
                    c.line(q.0, q.1, p.0, p.1, col2);
                }
                prev = Some(p);
            }
        }
    }
    // Digital speed and altitude (altitude rounded to 5 ft above 100 ft).
    let alt = if f.alt >= 100 { f.alt / 5 * 5 } else { f.alt };
    number(c, fonts, f.knots as i64, if st.declutter == 0 { 80 } else { 58 }, 50, col);
    number_right(c, fonts, alt as i64, if st.declutter == 0 { 240 } else { 264 }, 50, col);
    if st.declutter != 0 {
        if st.declutter == 2 {
            number(c, fonts, (f.heading as u32 / 182) as i64, 154, 8, col);
            frame_box(c, 151, 5, 167, 15, col);
        }
        frame_box(c, 55, 47, 79, 57, col);
        frame_box(c, 241, 47, 265, 57, col);
    }
    if f.accel {
        fonts.text(c, 110, 85, b"ACCEL", col);
    }
    if f.training {
        fonts.text(c, 182, 85, b"TRAINING", col);
    }
    // Waypoint cue (colour 11).
    if let Some(brg) = f.waypoint_bearing {
        let d = brg.wrapping_sub(f.heading) as i16 as i32;
        let x = (CX + (d >> 6) / 3).clamp(89, 229);
        if st.declutter == 2 {
            c.line(x, 17, x, 17, 11);
            c.line(x - 1, 18, x + 1, 18, 11);
        } else {
            c.line(x - 2, 11, x, 13, 11);
            c.line(x, 13, x + 2, 11, 11);
            c.line(x - 2, 11, x + 2, 11, 11);
        }
    }
    weapon_cues(c, f, col);
    if let Some((loc, gs)) = f.ils {
        ils_cross(c, f, loc, gs, col);
    }
    if let Some(z) = f.cues.launch_zone.filter(|_| st.mode == HudMode::Gnd) {
        launch_zone(c, fonts, &z, f.blink, col);
    }
    // Recon target locked: how close the camera has to get, under the message line.
    if let Some(r) = &f.cues.recon {
        let s = r.text("CAMERA");
        fonts.text(c, 160 - 2 * (s.len() as i32 & !1), 28, s.as_bytes(), col);
    }
    // HUD message at y=20, centred.
    if let Some(m) = &f.message {
        let s = upper(m);
        fonts.text(c, 160 - 2 * (s.len() as i32 & !1), 20, &s, col);
    }
}

/// AG dynamic launch zone: a scale left of the altitude tape up to the range the seeker locks
/// on from, the zone bracketed beside it and the target range caret; SHOOT (blinking) in the
/// zone with the lock or TOO CLOSE inside it, under the message and recon lines.
fn launch_zone(c: &mut Canvas, fonts: &Fonts, z: &super::LaunchZoneCue, blink: bool, col: u8) {
    let (x, top, bottom) = (228, 58, 82);
    let full = z.scale_km.max(0.1);
    let y = |r: f32| bottom - ((r / full).clamp(0.0, 1.0) * (bottom - top) as f32).round() as i32;
    c.line(x, top, x, bottom, col);
    c.line(x, top, x + 1, top, col);
    c.line(x, bottom, x + 1, bottom, col);
    if let Some((lo, hi)) = z.zone_km {
        let (a, b) = (y(hi), y(lo));
        c.line(x, a, x + 3, a, col);
        c.line(x + 3, a, x + 3, b, col);
        c.line(x, b, x + 3, b, col);
    }
    let yy = y(z.range_km);
    c.line(x - 3, yy - 2, x - 1, yy, col);
    c.line(x - 3, yy + 2, x - 1, yy, col);
    let text: &[u8] = if z.shoot && blink {
        b"SHOOT"
    } else if z.too_close {
        b"TOO CLOSE"
    } else {
        return;
    };
    fonts.text(c, 160 - 2 * (text.len() as i32 & !1), 36, text, col);
}

/// Ellipse outline (16 segments) around (x, y).
fn ellipse(c: &mut Canvas, x: i32, y: i32, rx: i32, ry: i32, col: u8) {
    let mut prev: Option<(i32, i32)> = None;
    for k in 0..=16 {
        let a = k << 12;
        let p =
            (x + (bsin(a) * rx as f32).round() as i32, y - (bcos(a) * ry as f32).round() as i32);
        if let Some(q) = prev {
            c.line(q.0, q.1, p.0, p.1, col);
        }
        prev = Some(p);
    }
}

/// Tracking box: half width `size`, half height `size - size/4`, with the LOCK
/// oval around it (contracted when in range).
fn target_box(c: &mut Canvas, x: i32, y: i32, size: i32, oval: bool, red: bool, col: u8) {
    let (hw, hh) = (size, size - size / 4);
    frame_box(c, x - hw, y - hh, x + hw, y + hh, col);
    if oval {
        let grow = if red { 3 } else { 7 };
        ellipse(c, x, y, hw + grow, hh + grow - 1, col);
    }
}

/// 4-pixel diamond.
fn diamond(c: &mut Canvas, x: i32, y: i32, col: u8) {
    c.line(x, y - 4, x + 4, y, col);
    c.line(x + 4, y, x, y + 4, col);
    c.line(x, y + 4, x - 4, y, col);
    c.line(x - 4, y, x, y - 4, col);
}

/// ILS steering bars (as the modern HUD's): the localizer bar upright, the glideslope bar level,
/// referenced to the flight path marker, 12 pixels out at full scale.
fn ils_cross(c: &mut Canvas, f: &HudFrame, loc: f32, gs: f32, col: u8) {
    let (fx, fy) = (CX, CY + (f.fpm >> 6));
    let x = fx - ((loc / 2.5).clamp(-1.0, 1.0) * 12.0).round() as i32;
    let y = fy + ((gs / 0.7).clamp(-1.0, 1.0) * 12.0).round() as i32;
    c.line(x, fy - 11, x, fy + 11, col);
    c.line(fx - 14, y, fx + 14, y, col);
}

/// Targeting cues: tracking box and lock oval, CCIP bullseye with
/// its fall line, ranging bar and flight-path guide diamond.
fn weapon_cues(c: &mut Canvas, f: &HudFrame, col: u8) {
    let cues = &f.cues;
    // Cues projected at the classic focal length, moved out as the glass shrinks the HUD so
    // they stay on what they mark.
    let k = super::classic_zoom(f.scale);
    let at = |(x, y): (i32, i32)| {
        (CX + ((x - CX) as f32 * k).round() as i32, CY + ((y - CY) as f32 * k).round() as i32)
    };
    if let Some(b) = cues.target_box {
        let colour = if b.colour == 0 { col } else { b.colour };
        let (x, y) = at((b.x, b.y));
        target_box(c, x, y, b.size, b.oval, b.red, colour);
    }
    if let Some((x, y)) = cues.ccip.map(at) {
        let fpm_y = CY + (f.fpm >> 6);
        c.line(0xA0, fpm_y, x, y, col);
        target_box(c, x, y, 5, true, true, 0x29);
    }
    if let Some((bar, flash)) = cues.ranging_bar
        && (!flash || f.blink)
    {
        c.line(0x9F - bar, 0x21, 0x9F - bar, 0x1E, 0x29);
        c.line(0xA0 + bar, 0x21, 0xA0 + bar, 0x1E, 0x29);
        c.line(0x9F - bar, 0x1E, 0xA0 + bar, 0x1E, 0x29);
    }
    if let Some((x, y)) = cues.diamond.map(at) {
        diamond(c, x, y, col);
    }
}

/// Difficulty divisor of the landing sink limit (STUB: rookie).
const LANDING_DIFFICULTY: i32 = 1;

/// Tick strip of a tape (MG slot `0151`): a dot every 2 rows, a 2-px tick every 10 rows and a
/// 3-px tick every 20 rows (at the labels), anchored at `tick0` (a label tick row). Rows
/// below `floor` (values below zero) are left empty.
fn tape_ticks(c: &mut Canvas, x: i32, tick0: i32, floor: Option<i32>, col: u8, right: bool) {
    for y in 22..=82 {
        if floor.is_some_and(|f| y > f) {
            continue;
        }
        let d = (y - tick0).rem_euclid(20);
        if d % 2 != 0 {
            continue;
        }
        let len = match d {
            0 => 3,
            10 => 2,
            _ => 1,
        };
        for k in 0..len {
            // Speed tape ticks grow to the left of x=71, altitude ticks to the right of 248.
            c.set(if right { x + k } else { x - k }, y, col);
        }
    }
}

/// HUD instruments: tapes, heading scale, nose cross, G, pitch ladder.
fn instruments(c: &mut Canvas, atlas: &Canvas, fonts: &Fonts, f: &HudFrame, col: u8, col2: u8) {
    let decl = f.state.declutter;
    if decl == 0 {
        speed_tape(c, fonts, f.knots.max(0), col);
    }
    if decl <= 1 {
        if decl == 0 {
            c.line(73, CY, 76, CY, col);
            c.line(243, CY, 246, CY, col);
        }
        c.line(159, 16, 159, 18, col);
        if decl == 0 {
            alt_tape(c, fonts, f.alt.clamp(0, 65535), col);
        }
        heading_scale(c, atlas, fonts, f.heading, col);
    }
    // (The HUD frame is the 3D cockpit's combiner: `border`.)
    // Nose cross: atlas (69,73) 25x15 at (147,44).
    c.blit(atlas, 69, 73, 25, 15, 147, 44, Some(0), None);
    // G readout "1.0G" at (60,16): integer part and eighths (gees & 14) / 2.
    let g = f.gees;
    fonts.text(c, 60, 16, format!("{}.{}G", g / 16, (g.abs() & 0xE) >> 1).as_bytes(), col);
    // Pitch ladder: NAV mode, or steep attitudes (|pitch| - |roll|/2 > 22.5 deg).
    let steep = (f.pitch as i32).abs() - (f.roll as i32).abs() / 2 > 0x1000;
    if f.state.mode == HudMode::Nav || steep {
        pitch_ladder(c, fonts, f.pitch, f.roll, col2);
    }
}

fn speed_tape(c: &mut Canvas, fonts: &Fonts, knots: i32, col: u8) {
    let (q, r) = (knots / 50, knots % 50);
    let mut y = r * 2 / 5 + 90;
    let mut di = q - 1;
    let mut tick0 = None;
    let mut floor = None;
    c.set_clip(0, 22, 319, 82);
    for _ in 0..5 {
        y -= 20;
        if di < 0 {
            // Values below zero: no label, and the tick strip stops at the zero mark.
            floor = Some(y - 18);
        } else if di < 20 {
            fonts.text(c, 49, y, format!(" {:03}", di * 50).as_bytes(), col);
        }
        tick0.get_or_insert(y + 2);
        di += 1;
    }
    c.set_clip(0, 0, 319, 90);
    tape_ticks(c, 71, tick0.unwrap_or(CY), floor, col, false);
}

fn alt_tape(c: &mut Canvas, fonts: &Fonts, alt: i32, col: u8) {
    let (q, r) = (alt / 1000, alt % 1000);
    c.set_clip(0, 22, 319, 82);
    let mut labels: Vec<(i32, Vec<u8>)> = Vec::new();
    let mut floor = None;
    let tick0;
    if q == 0 {
        let (h, rem) = (r / 100, r % 100);
        let mut y = rem * 2 / 10 + 90;
        let mut di = h - 1;
        let mut count = 5;
        let mut thousands = false;
        let mut k = 1;
        while count > 0 {
            y -= 20;
            if !thousands && di < 0 {
                floor = Some(y - 18);
                di = 0;
                count -= 1;
                continue;
            }
            if thousands {
                labels.push((y, format!("{k:>2}K").into_bytes()));
                k += 1;
            } else {
                labels.push((y, format!("{di}00").into_bytes()));
                di += 1;
                if di == 10 {
                    thousands = true;
                }
            }
            count -= 1;
        }
        tick0 = labels.first().map_or(CY, |l| l.0 + 2);
    } else {
        let mut y = r * 2 / 100 + 90;
        // The slot for the lowest label shows "900" when it is the 0 ft mark (classic quirk).
        for k in 0..5 {
            y -= 20;
            let v = q - 1 + k;
            let s = if v == 0 { b"900".to_vec() } else { format!("{v:>2}K").into_bytes() };
            labels.push((y, s));
        }
        tick0 = labels[0].0 + 2;
    }
    for (y, s) in &labels {
        fonts.text(c, 255, *y, s, col);
    }
    c.set_clip(0, 0, 319, 90);
    tape_ticks(c, 248, tick0, floor, col, true);
}

/// Heading scale: 3-digit labels every 45 px at y=6 (clip x 90..230), tick sprite
/// 141x3 from the atlas at (90,12).
fn heading_scale(c: &mut Canvas, atlas: &Canvas, fonts: &Fonts, heading: u16, col: u8) {
    const LABELS: [&[u8]; 8] = [b"000", b"045", b"090", b"135", b"180", b"225", b"270", b"315"];
    let h = heading.wrapping_sub(0x2000);
    let off = (((h & 0x1F80) >> 7) as i32 * 45) >> 6;
    let sector = (h >> 13) as usize;
    let x0 = 109 - off;
    c.set_clip(90, 0, 230, 90);
    for k in 0..4 {
        fonts.text(c, x0 + 45 * k as i32, 6, LABELS[(sector + k) & 7], col);
    }
    c.set_clip(0, 0, 319, 90);
    let mut phase = off % 10;
    if heading & 0x2000 != 0 {
        phase += 5;
    }
    if phase >= 10 {
        phase -= 10;
    }
    c.blit(atlas, 0, 27 - 3 * phase, 141, 3, 90, 12, Some(0), None);
}

/// Pitch ladder: a rung every 10 degrees = 52 units, rotated by roll and
/// squashed vertically by 0.75; labels at both ends.
fn pitch_ladder(c: &mut Canvas, fonts: &Fonts, pitch: i16, roll: i16, col: u8) {
    // Label x/y offsets by roll, 64 entries.
    const LX: [i8; 64] = [
        2, 2, 2, 2, 2, 1, 1, 1, 0, 0, 0, 0, -1, -1, -2, -2, -3, -4, -4, -5, -5, -6, -6, -7, -7, -8,
        -8, -8, -9, -9, -9, -9, -9, -9, -9, -9, -9, -8, -8, -8, -7, -7, -6, -6, -5, -5, -4, -4, -3,
        -2, -2, -1, -1, 0, 0, 0, 0, 1, 1, 1, 2, 2, 2, 2,
    ];
    const LY: [i8; 64] = [
        -1, -2, -2, -3, -3, -4, -4, -5, -5, -6, -6, -6, -7, -7, -7, -7, -7, -7, -7, -7, -7, -6, -6,
        -6, -5, -5, -4, -4, -3, -3, -2, -2, -1, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 3, 4, 4, 4, 4, 4, 4,
        4, 4, 4, 3, 3, 3, 2, 2, 1, 1, 0, 0, 0, 0,
    ];
    const LXN: [i8; 64] = [
        0, 0, 0, 0, 0, 0, 0, 0, -1, -1, -2, -2, -3, -3, -4, -4, -5, -6, -6, -7, -7, -8, -8, -9, -9,
        -10, -10, -10, -11, -11, -11, -11, -11, -11, -11, -11, -11, -10, -10, -10, -9, -9, -8, -8,
        -7, -7, -6, -6, -5, -4, -4, -3, -3, -2, -2, -1, -1, 0, 0, 0, 0, 0, 0, 0,
    ];
    const NAMES: [&[u8]; 23] = [
        b"-70", b"-80", b"-90", b"-80", b"-70", b"-60", b"-50", b"-40", b"-30", b"-20", b"-10",
        b"00", b"10", b"20", b"30", b"40", b"50", b"60", b"70", b"80", b"90", b"80", b"70",
    ];
    // Pitch in quarter degrees -> tens and pixel offset within the decade.
    let p = pitch as i32;
    let quarter = ((p.unsigned_abs() >> 6) * 360) >> 8;
    let mut tens = (quarter / 40) as i32;
    let mut frac = ((quarter % 40) * 52 / 40) as i32;
    if p < 0 {
        frac = 52 - frac;
        tens = -tens - 1;
    }
    let mut y = frac + 104;
    let mut dl = tens - 2;
    // Rotation by roll in Q15 fixed point (sin of -roll):
    // x' = cos*x - s*y (truncated), y' = 0.75*(cos*y + s*x) as (3*a*b >> 16) >> 1.
    let (cf, sf) = (bcos(-(roll as i32)), bsin(-(roll as i32)));
    let (cq, sq) = ((cf * 32767.0).round() as i64, (sf * 32767.0).round() as i64);
    let mul2 = |a: i64, b: i32| (a * b as i64 * 2) >> 16;
    let mul3 = |a: i64, b: i32| ((a * b as i64 * 3) >> 16) >> 1;
    let rot = |x: i32, y: i32| -> (i32, i32) {
        ((mul2(cq, x) - mul2(sq, y)) as i32, (mul3(cq, y) + mul3(sq, x)) as i32)
    };
    let line_pt = |x: i32, y: i32| {
        let (rx, ry) = rot(x, y);
        (CX + rx, CY + ry)
    };
    let ridx = ((roll as u16) >> 10) as usize;
    let lidx = (((roll as u16).wrapping_add(0x8000)) >> 10) as usize;
    for _ in 0..5 {
        // Polylines in ladder units: (points, label anchor indices).
        let (segs, anchors): (Vec<Vec<(i32, i32)>>, (usize, usize, usize, usize)) = match dl {
            9 => (vec![vec![(-50, y), (50, y)]], (0, 0, 0, 1)),
            0 | -9 => (vec![vec![(-50, y), (-12, y)], vec![(13, y), (50, y)]], (0, 0, 1, 1)),
            d if d > 9 => (vec![vec![(-50, y - 5), (-50, y), (50, y), (50, y - 5)]], (0, 1, 0, 2)),
            d if d < -9 => (
                vec![vec![(-50, y + 5), (-50, y), (-12, y)], vec![(13, y), (50, y), (50, y + 5)]],
                (0, 1, 1, 1),
            ),
            d if d > 0 => (vec![vec![(-50, y + 5), (-50, y), (50, y), (50, y + 5)]], (0, 1, 0, 2)),
            _ => (
                vec![vec![(-50, y - 5), (-50, y), (-12, y)], vec![(13, y), (50, y), (50, y - 5)]],
                (0, 1, 1, 1),
            ),
        };
        // Lines are clipped to y 16..86, labels to (78,20)-(241,89).
        c.set_clip(0, 16, 319, 86);
        for seg in &segs {
            for w in seg.windows(2) {
                let (a, b) = (line_pt(w[0].0, w[0].1), line_pt(w[1].0, w[1].1));
                c.line(a.0, a.1, b.0, b.1, col);
            }
        }
        c.set_clip(78, 20, 241, 89);
        // Labels: left at the first anchor, right at the second.
        let li = (dl + 11).clamp(0, 22) as usize;
        let neg = li < 11;
        let (lseg, lpt, rseg, rpt) = anchors;
        for (pt, idx, add) in [(segs[lseg][lpt], lidx, -1), (segs[rseg][rpt], ridx, 2)] {
            let (rx, ry) = rot(pt.0, pt.1);
            let ox = if neg { LXN[idx] } else { LX[idx] } as i32 + add;
            let x = CX + rx + ox;
            let y = CY + ry + LY[idx] as i32;
            fonts.text(c, x, y, NAMES[li], col);
        }
        y -= 52;
        dl += 1;
    }
    c.set_clip(0, 0, 319, 90);
}
