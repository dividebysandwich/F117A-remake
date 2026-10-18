//! Instrument panel (256PIT rows 91..199): indicator lights, LED digit readouts,
//! EMV/EGR/EAR gauge, attitude ball, HUD posts and the approach indexer.
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

/// Light rectangles `[x, y, w, h, colour set]` (table `DS:46D8`); rect = (x,y)-(x+w,y+h).
pub const LIGHTS: [(i32, i32, i32, i32, usize); 37] = [
    (119, 94, 18, 8, 0),  // 0 NAV
    (139, 94, 18, 8, 0),  // 1 AIR
    (159, 94, 18, 8, 0),  // 2 GND
    (184, 94, 21, 8, 2),  // 3 LOCK
    (38, 105, 24, 8, 3),  // 4 STALL
    (4, 117, 18, 8, 4),   // 5 MW
    (24, 117, 18, 8, 2),  // 6 ENG
    (44, 117, 18, 8, 2),  // 7 FC
    (4, 127, 18, 8, 2),   // 8 JAM
    (24, 127, 18, 8, 2),  // 9 FIRE
    (44, 127, 18, 8, 2),  // 10 FUEL (damage)
    (4, 137, 18, 8, 2),   // 11 BD
    (24, 137, 18, 8, 2),  // 12 AV
    (44, 137, 18, 8, 2),  // 13 (blank)
    (51, 149, 11, 6, 2),  // 14 chaff active
    (51, 159, 11, 6, 2),  // 15 flare active
    (51, 169, 11, 6, 2),  // 16 decoy active
    (4, 179, 28, 8, 0),   // 17 ECM
    (36, 179, 26, 8, 0),  // 18 IRJ
    (4, 191, 18, 8, 2),   // 19 RAD
    (24, 191, 18, 8, 2),  // 20 IR
    (44, 191, 18, 8, 2),  // 21 TRAK
    (80, 173, 18, 8, 0),  // 22 MAP
    (100, 173, 18, 8, 0), // 23 TAC
    (142, 193, 14, 5, 1), // 24 EMV warning
    (232, 184, 17, 8, 2), // 25 FLIR
    (190, 181, 18, 8, 0), // 26 WPN
    (190, 191, 38, 8, 0), // 27 camera direction
    (170, 181, 18, 8, 0), // 28 WAY
    (210, 181, 18, 8, 0), // 29 ORD
    (170, 191, 18, 8, 0), // 30 TC
    (257, 103, 25, 8, 3), // 31 BAY
    (257, 113, 25, 8, 2), // 32 AUTO
    (284, 113, 25, 8, 4), // 33 FLAPS
    (257, 123, 25, 8, 2), // 34 GEAR
    (284, 123, 25, 8, 2), // 35 BRAKE
    (302, 179, 13, 7, 2), // 36 low fuel
];

/// Colour sets (`DS:4672`): unlit -> lit pairs.
const COLOUR_SETS: [&[(u8, u8)]; 5] = [
    &[(45, 47), (44, 46)],
    &[(38, 41), (55, 42)],
    &[(54, 41), (55, 42)],
    &[(16, 41), (43, 42)],
    &[(16, 40), (43, 39), (54, 41), (55, 42)],
];

/// Lights a panel light (`1A18:D83E` with on = 1).
pub fn light(c: &mut Canvas, idx: usize) {
    let (x, y, w, h, set) = LIGHTS[idx];
    for &(from, to) in COLOUR_SETS[set] {
        c.recolor_rect(x, y, x + w, y + h, from, to);
    }
}

/// LED digits (`1A18:D92D`): atlas (67+5d, 31) 4x7, 5 px advance; non-digits show "0".
pub fn led(c: &mut Canvas, atlas: &Canvas, x: i32, y: i32, s: &str) {
    for (k, ch) in s.bytes().enumerate() {
        let d = if ch.is_ascii_digit() { (ch - b'0') as i32 } else { 0 };
        c.copy_rect(atlas, 67 + 5 * d, 31, 4, 7, x + 5 * k as i32, y);
    }
}

/// EMV gauge bar (`1A18:8316`): value > 0 grows from the left edge (x 86), < 0 from the right
/// edge (x 136), length |value|/2 (value 0 still fills the rightmost column).
fn gauge_bar(c: &mut Canvas, value: i32, colour: u8, y0: i32, y1: i32) {
    if value > 0 {
        c.fill_rect(86, 187 + y0, value / 2 + 1, y1 - y0 + 1, colour);
    } else {
        let x0 = 136 + value / 2;
        c.fill_rect(x0, 187 + y0, 136 - x0 + 1, y1 - y0 + 1, colour);
    }
}

/// Attitude ball (`1A18:17A3`): sky 9, ground polygon 1 in the 32x28 window at (265,140),
/// then the bezel with the aircraft symbol, atlas (284,33) 33x33 at (265,138).
pub fn adi(c: &mut Canvas, atlas: &Canvas, pitch: f32, roll: f32) {
    let (s, co) = roll.sin_cos();
    // Horizon offset 16*tan(pitch) in ball units (x units = 2 px, y units = 1.5 px).
    let off = 16.0 * pitch.clamp(-1.5, 1.5).tan();
    for py in 0..28 {
        for px in 0..32 {
            let x = (px as f32 + 0.5 - 16.0) / 2.0;
            let y = (py as f32 + 0.5 - 14.0) / 1.5;
            // Ground side of the horizon (rotated with the bank, shifted by pitch).
            let ground = x * s + y * co > -off;
            c.set(265 + px, 140 + py, if ground { 1 } else { 9 });
        }
    }
    c.blit(atlas, 284, 33, 33, 33, 265, 138, Some(0), None);
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
    /// Draws the panel instruments onto the cockpit picture.
    pub fn draw(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
        let atlas = &art.pit;
        let blink = crate::hud::blink(&self.time);
        let a = self.aircraft.single().ok();
        let w = &self.warnings;
        let mut lit = [false; 37];
        lit[match self.hud.mode {
            HudMode::Nav => 0,
            HudMode::Air => 1,
            HudMode::Gnd => 2,
        }] = true;
        // LOCK blinks while the oval is up and is steady in range (the simulation blinks it).
        lit[3] = w.lock;
        if let Some(a) = a {
            lit[4] = !a.on_ground && a.velocity < a.stall_speed && blink;
            lit[31] = a.bay_open;
            lit[32] = a.autopilot();
            lit[33] = a.flaps;
            lit[34] = a.gear_down;
            lit[35] = a.brakes;
            lit[36] = a.fuel < LOW_FUEL && blink;
        }
        lit[5] = w.missile_warning;
        lit[6] = w.engine;
        lit[7] = w.fire_control;
        lit[8] = w.jammer;
        lit[9] = w.fire;
        lit[10] = w.fuel;
        lit[11] = w.bay_doors;
        lit[12] = w.avionics;
        for k in 0..3 {
            lit[14 + k] = self.cm.active[k] > 0.0;
        }
        lit[17] = self.cm.ecm;
        lit[18] = self.cm.ir_jammer;
        lit[19] = w.radar_missile;
        lit[20] = w.ir_missile;
        lit[21] = w.tracked;
        lit[22] = self.mfd.left == LeftMfd::Map;
        lit[23] = self.mfd.left == LeftMfd::Tac;
        let emv = (self.emv.emv.clamp(0.0, 1.0) * 99.0) as i32;
        let egr = (self.emv.egr.clamp(0.0, 1.0) * 99.0) as i32;
        let ear = (self.emv.ear.clamp(0.0, 1.0) * 99.0) as i32;
        lit[24] = w.emv_warning;
        lit[25] = self.mfd.flir;
        lit[26] = self.mfd.right == RightMfd::Wpn;
        lit[27] = self.mfd.right == RightMfd::Camera;
        lit[28] = self.mfd.right == RightMfd::Way;
        lit[29] = self.mfd.right == RightMfd::Ord;
        lit[30] = self.mfd.right == RightMfd::Camera;
        for (i, on) in lit.iter().enumerate() {
            if *on {
                light(c, i);
            }
        }
        // Camera direction text in light 27.
        if self.mfd.right == RightMfd::Camera {
            let s = match self.mfd.cam_dir {
                CamDir::Ahead => " Ahead ",
                CamDir::Right => " Right ",
                CamDir::Rear => " Rear  ",
                CamDir::Left => " Left  ",
            };
            fonts.text_opaque(c, 197, 193, &upper(s), 47, 46);
        }

        // LED readouts: CHAFF, FLARE, DECOY (2 digits), FUEL (fuel/10, max 999), THRUST %.
        led(c, atlas, 35, 149, &format!("{:02}", self.cm.chaff.min(99)));
        led(c, atlas, 35, 159, &format!("{:02}", self.cm.flares.min(99)));
        led(c, atlas, 35, 169, &format!("{:02}", self.cm.decoys.min(99)));
        if let Some(a) = a {
            led(c, atlas, 261, 179, &format!("{:03}", ((a.fuel / 10.0) as i32).clamp(0, 999)));
            led(c, atlas, 261, 191, &format!("{:03}", (a.thrust.round() as i32).clamp(0, 999)));
        }

        // EMV (blue from the left, red from the right), EGR and EAR bars.
        gauge_bar(c, 100 - emv, 1, 0, 10);
        gauge_bar(c, -emv, 42, 0, 10);
        if egr > 0 {
            let colour = if emv + egr > 100 { 17 } else if emv + egr > 50 { 41 } else { 42 };
            gauge_bar(c, egr, colour, 1, 4);
        }
        if ear > 0 {
            gauge_bar(c, ear, if emv + ear > 100 { 15 } else { 11 }, 6, 9);
        }

        if let Some(a) = a {
            let (_, p, r) = a.euler();
            adi(c, atlas, p, r);
        }
    }

    /// The approach indexer into its own 10x21 canvas.
    pub fn draw_approach(&self, c: &mut Canvas, art: &CockpitArt) {
        approach_indexer(c, &art.pit, self.approach.0, 0, 0);
    }
}

/// HUD posts and the approach indexer (`1A18:D9A3`), replaced by the modelled combiner
/// (kept for the snapshot comparison): atlas (264,33) and (271,33) 6x58 to
/// (43,33) and (272,33); indexer atlas (251,67) 10x21 to (33,70) with the lit part recoloured
/// 30 -> 41 (top chevron), 47 (circle) or 14 (bottom chevron).
#[cfg_attr(not(test), allow(dead_code))]
pub fn posts(c: &mut Canvas, atlas: &Canvas, approach: u8) {
    c.copy_rect(atlas, 264, 33, 6, 58, 43, 33);
    c.copy_rect(atlas, 271, 33, 6, 58, 272, 33);
    approach_indexer(c, atlas, approach, 33, 70);
}

/// The approach indexer (10x21) with its top-left at (x, y).
pub fn approach_indexer(c: &mut Canvas, atlas: &Canvas, approach: u8, x: i32, y: i32) {
    c.copy_rect(atlas, 251, 67, 10, 21, x, y);
    const PARTS: [(i32, i32, u8); 3] = [(2, 4, 41), (7, 7, 47), (15, 4, 14)];
    if let Some(&(dy, h, colour)) = (approach as usize).checked_sub(1).and_then(|i| PARTS.get(i)) {
        c.recolor_rect(x + 1, y + dy, x + 8, y + dy + h, 30, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a file relative to the asset root's directory (`assets/...`, `re-work/...`).
    fn load(name: &str) -> Option<Vec<u8>> {
        std::fs::read(crate::asset_path::root().join(name)).ok()
    }

    /// Panel of the emulator snapshot on the runway (re-work/hud/snaps/base.vram, optional):
    /// NAV, MAP, ORD, GEAR lit; chaff 18, flares 12, decoys 3, fuel 999, thrust 0; EMV 0.
    #[test]
    fn base_panel() {
        let (Some(pit), Some(vram)) = (load("assets/images/256pit.idx"), load("re-work/hud/snaps/base.vram")) else {
            return;
        };
        let atlas = Canvas::from_pixels(320, 200, pit);
        let mut c = atlas.clone();
        for i in [0, 22, 29, 34] {
            light(&mut c, i);
        }
        led(&mut c, &atlas, 35, 149, "18");
        led(&mut c, &atlas, 35, 159, "12");
        led(&mut c, &atlas, 35, 169, "03");
        led(&mut c, &atlas, 261, 179, "999");
        led(&mut c, &atlas, 261, 191, "000");
        gauge_bar(&mut c, 100, 1, 0, 10);
        gauge_bar(&mut c, 0, 42, 0, 10);
        adi(&mut c, &atlas, 0.0, 0.0);
        let mut diffs = Vec::new();
        for y in 91..200 {
            for x in 0..320 {
                let in_mfd = (110..=169).contains(&y) && ((74..=150).contains(&x) || (169..=245).contains(&x));
                // See-through corners of the panel strip show the 3D view.
                let window = y < 107 && atlas.px[y * 320 + x] == 0;
                if !in_mfd && !window && c.px[y * 320 + x] != vram[y * 320 + x] {
                    diffs.push((x, y, c.px[y * 320 + x], vram[y * 320 + x]));
                }
            }
        }
        eprintln!("{} panel pixels differ: {:?}", diffs.len(), &diffs[..diffs.len().min(40)]);
        assert!(diffs.is_empty());
    }
}
