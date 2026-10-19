//! Head-up display, drawn into the cockpit picture (320x200 original screen coordinates).
//! Layout and behaviour: docs/re/cockpit.md section 2, VGAME `1A18:7898` (overlay part) and
//! `2AC0:000C` (instruments: tapes, heading scale, frame, G, pitch ladder).
//!
//! The drawing is a pure function of [`HudFrame`] so it can be compared with VRAM dumps of
//! the original; [`HudParams`] gathers the frame from the ECS.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::canvas::Canvas;
use crate::cockpit::CockpitArt;
use crate::flight::{Aircraft, Player, to_bam};
use crate::font::{Fonts, upper};
use crate::input::{Action, ActionEvent};
use crate::world::{WorldPos, bevy_to_wu};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .init_resource::<HudMessages>()
            .init_resource::<Stores>()
            .init_resource::<Waypoints>()
            .init_resource::<GunLead>()
            .init_resource::<HudCues>()
            .add_systems(OnEnter(crate::settings::AppState::Flight), reset_hud)
            .add_systems(
                Update,
                (hud_keys, update_messages, advance_waypoints)
                    .after(crate::flight::FlightSystems)
                    .run_if(in_state(crate::settings::AppState::Flight)),
            );
    }
}

/// HUD centre (boresight) in screen pixels.
pub const CX: i32 = 159;
pub const CY: i32 = 52;

/// F2: NAV / AIR / GND (`DS:E2FE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HudMode {
    #[default]
    Nav,
    Air,
    Gnd,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct HudState {
    pub mode: HudMode,
    /// V: 0 = tapes, 1 = boxed digits, 2 = boxed digits + heading box (`DS:294A`).
    pub declutter: u8,
    /// F4: night HUD colours (`DS:43B0`).
    pub night: bool,
}

impl HudState {
    /// Main HUD colour (`DS:2CA0`) and second colour (`DS:2CA1`, pitch ladder).
    pub fn colours(&self) -> (u8, u8) {
        if self.night { (10, 47) } else { (15, 7) }
    }
}

/// HUD message line (y = 20, centred) and status words. Other systems post messages with
/// [`HudMessages::show`].
#[derive(Resource, Debug, Clone, Default)]
pub struct HudMessages {
    pub text: String,
    /// Seconds the message stays up.
    pub timer: f32,
    /// "ACCEL" (time acceleration active).
    pub accel: bool,
    /// "TRAINING".
    pub training: bool,
}

/// Damage flash request (DS:98EE): VGAME 78CC fills rows 0..89 with colour 0x0D for one frame
/// in the cockpit views when the aircraft is hit.
#[derive(Resource, Default)]
pub struct DamageFlash(pub u8);

impl HudMessages {
    pub fn show(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.timer = 3.0;
    }
}

/// One weapon bay.
#[derive(Debug, Clone)]
pub struct Bay {
    /// Original weapon name (`DS:366C` table), e.g. "IN Cluster".
    pub name: String,
    pub count: u32,
    /// ORDNANCE.PIC side view index (`DS:00A2` table).
    pub picture: usize,
    /// Air-to-air missile (weapon class 7): AIR mode shows the missile envelope.
    pub air_to_air: bool,
}

/// Weapon stores (mirrored from the combat simulation every frame; defaults to a typical load).
#[derive(Resource, Debug, Clone)]
pub struct Stores {
    pub bays: Vec<Bay>,
    pub selected: usize,
    pub gun_rounds: Option<u32>,
}

impl Default for Stores {
    fn default() -> Self {
        let bay = |name: &str, count, picture, air_to_air| Bay { name: name.into(), count, picture, air_to_air };
        Self {
            bays: vec![
                bay("IN Cluster", 2, 9, false),
                bay("Durandal", 2, 8, false),
                bay("Maverick", 2, 16, false),
                bay("Sidewinder", 4, 0, true),
            ],
            selected: 0,
            gun_rounds: Some(650),
        }
    }
}

/// A navigation waypoint in original world units.
#[derive(Debug, Clone, Copy)]
pub struct Waypoint {
    pub east: f64,
    pub north: f64,
}

/// INS waypoints (from the mission: 0 midpoint, 1 primary, 2 secondary, 3 landing base). The
/// HUD cue and the autopilot point at `points[current]`; empty = no waypoint.
#[derive(Resource, Debug, Clone, Default)]
pub struct Waypoints {
    pub points: Vec<Waypoint>,
    pub current: usize,
    /// Waypoint the mission logic selected last (DS:2E7C); Shift-F8 returns to it.
    pub mission_current: usize,
}

impl Waypoints {
    pub fn target(&self) -> Option<Waypoint> {
        self.points.get(self.current).copied()
    }
}

/// Weapon cues over the 3D view (weapons.md section 5.4), in 320x200 screen pixels; filled by
/// the combat simulation.
#[derive(Resource, Debug, Clone, Default)]
pub struct HudCues {
    /// Tracking box around the locked target.
    pub target_box: Option<TargetBox>,
    /// CCIP bomb impact point (free-fall bombs), screen position.
    pub ccip: Option<(i32, i32)>,
    /// Ranging bar half-length and whether it flashes.
    pub ranging_bar: Option<(i32, bool)>,
    /// Flight-path guide diamond (128 map units past the target at the own altitude).
    pub diamond: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Copy)]
pub struct TargetBox {
    pub x: i32,
    pub y: i32,
    /// Half width (half height = size - size/4).
    pub size: i32,
    /// 0 = HUD colour, 6 = wrong weapon, 0x29 = in range.
    pub colour: u8,
    /// LOCK oval shown; `red` = in range (the oval contracts).
    pub oval: bool,
    pub red: bool,
}

/// AIR-mode gun sight lead (`DS:E546`/`DS:E550`, pixels << 4). STUB: 0 = boresight.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct GunLead {
    pub x: i32,
    pub y: i32,
}

fn reset_hud(mut hud: ResMut<HudState>, mut msg: ResMut<HudMessages>) {
    *hud = HudState::default();
    *msg = HudMessages::default();
}

fn hud_keys(mut actions: MessageReader<ActionEvent>, mut hud: ResMut<HudState>, mut wps: ResMut<Waypoints>, stores: Res<Stores>) {
    let n = wps.points.len();
    for ActionEvent(a) in actions.read() {
        match a {
            Action::HudMode => {
                hud.mode = match hud.mode {
                    HudMode::Nav => HudMode::Air,
                    HudMode::Air => HudMode::Gnd,
                    HudMode::Gnd => HudMode::Nav,
                }
            }
            Action::NearestTarget => {
                let air = stores.bays.get(stores.selected).is_some_and(|b| b.air_to_air);
                hud.mode = if air { HudMode::Air } else { HudMode::Gnd };
            }
            Action::HudDeclutter => hud.declutter = (hud.declutter + 1) % 3,
            Action::DayNightHud => hud.night = !hud.night,
            // INS: PgUp / PgDn step through the waypoints, F8 changes to the next one, Shift-F8
            // returns to the mission's current waypoint (F7 shows the WAY page, see mfd.rs).
            // Weapon selection (Space) is done by the combat simulation.
            Action::LastWaypoint if n > 0 => wps.current = (wps.current + n - 1) % n,
            Action::NextWaypoint | Action::ChangeWaypoint if n > 0 => wps.current = (wps.current + 1) % n,
            Action::ResetWaypoint if n > 0 => wps.current = wps.mission_current.min(n - 1),
            _ => {}
        }
    }
}

fn update_messages(time: Res<Time>, mut msg: ResMut<HudMessages>) {
    if msg.timer > 0.0 {
        msg.timer = (msg.timer - time.delta_secs()).max(0.0);
    }
}

/// Map units (wu >> 5, y growing southward) of a world position.
pub fn map_units(pos: &WorldPos) -> (i32, i32) {
    let (e, n, _) = bevy_to_wu(pos.0);
    ((e / 32.0) as i32, 0x8000 - (n / 32.0) as i32)
}

/// Bearing (binary angle) from a position to a waypoint.
fn waypoint_bearing(pos: &WorldPos, wp: Waypoint) -> u16 {
    let (x, y) = map_units(pos);
    let dx = (wp.east / 32.0) as i32 - x;
    let dy_north = y - (0x8000 - (wp.north / 32.0) as i32);
    to_bam((dx as f32).atan2(dy_north as f32)) as i32 as u16
}

/// "Waypoint n reached" when within 0x200 map units (VGAME 7E56..7E97); the autopilot steers to
/// the current waypoint.
fn advance_waypoints(
    mut q: Query<(&WorldPos, &mut Aircraft), With<Player>>,
    mut wps: ResMut<Waypoints>,
    mut msg: ResMut<HudMessages>,
) {
    let Ok((pos, mut a)) = q.single_mut() else { return };
    let Some(wp) = wps.target() else { return };
    let (x, y) = map_units(pos);
    let (dx, dy) = ((wp.east / 32.0) as i32 - x, (0x8000 - (wp.north / 32.0) as i32) - y);
    if ((dx * dx + dy * dy) as f64).sqrt() < 512.0 && wps.current + 1 < wps.points.len() {
        wps.current += 1;
        msg.show(format!("Waypoint {} reached", wps.current));
    }
    if let Some(wp) = wps.target() {
        a.waypoint_bearing = waypoint_bearing(pos, wp) as f32;
    }
}

/// Everything the HUD shows, in the original's units.
#[derive(Debug, Clone, Default)]
pub struct HudFrame {
    pub state: HudState,
    /// `DS:B168`
    pub knots: i32,
    /// Altitude in feet (`DS:2DF6`).
    pub alt: i32,
    /// Heading, pitch, roll in 16-bit binary angles (`DS:2DEE/2DF0/2DF2`); roll > 0 = right.
    pub heading: u16,
    pub pitch: i16,
    pub roll: i16,
    /// 1/16 g (`DS:98DC`)
    pub gees: i32,
    /// Stall speed, knots (`DS:E448`).
    pub stall_knots: i32,
    /// Climb rate, wu/s (`DS:E55A`).
    pub climb: i32,
    /// Flight path marker offset << 6 (`DS:98C6` = cos(roll) * (lift - 0x200)).
    pub fpm: i32,
    pub gear_down: bool,
    /// Night mission (`DS:3658`): stall/VVI bar colour.
    pub night_mission: bool,
    /// Frame parity used for blinking (`DS:3D60 & 1`).
    pub blink: bool,
    /// Bearing to the current waypoint (binary angle).
    pub waypoint_bearing: Option<u16>,
    /// Selected weapon: (count, name).
    pub weapon: Option<(u32, String)>,
    pub weapon_air_to_air: bool,
    pub gun_rounds: Option<u32>,
    pub message: Option<String>,
    pub accel: bool,
    pub training: bool,
    /// Tracking camera selected on the right MFD: camera sight cross.
    pub camera_sight: bool,
    pub gun_lead: (i32, i32),
    pub cues: HudCues,
}

#[derive(SystemParam)]
pub struct HudParams<'w, 's> {
    pub aircraft: Query<'w, 's, (&'static Aircraft, &'static WorldPos), With<Player>>,
    pub state: Res<'w, HudState>,
    pub messages: Res<'w, HudMessages>,
    pub stores: Res<'w, Stores>,
    pub waypoints: Res<'w, Waypoints>,
    pub lead: Res<'w, GunLead>,
    pub cues: Res<'w, HudCues>,
    pub tod: Res<'w, crate::palette::TimeOfDay>,
    pub time: Res<'w, Time>,
}

/// Original frame parity (`DS:3D60`), ticking at about the original frame rate.
pub fn blink(time: &Time) -> bool {
    (time.elapsed_secs() * 8.0) as u32 & 1 == 1
}

impl HudParams<'_, '_> {
    pub fn frame(&self, camera_sight: bool) -> Option<HudFrame> {
        let (a, pos) = self.aircraft.single().ok()?;
        let (h, p, r) = a.euler();
        let lift = (a.stall_speed * 3072.0 / (a.velocity.abs() + 1.0)).min(8192.0);
        // Avionics damage also takes out the INS waypoint cue.
        let bearing = self.waypoints.target().filter(|_| !a.damage.autopilot_failed).map(|wp| waypoint_bearing(pos, wp));
        let bay = self.stores.bays.get(self.stores.selected);
        Some(HudFrame {
            state: self.state.clone(),
            knots: a.knots() as i32,
            alt: a.altitude.clamp(0.0, 65535.0) as i32,
            heading: to_bam(h) as i32 as u16,
            pitch: to_bam(p) as i32 as i16,
            roll: to_bam(r) as i32 as i16,
            gees: a.gees as i32,
            stall_knots: (a.stall_speed / 27.0) as i32,
            climb: a.climb_rate as i32,
            fpm: (r.cos() * (lift - 512.0)) as i32,
            gear_down: a.gear_down,
            night_mission: self.tod.night_mission,
            blink: blink(&self.time),
            waypoint_bearing: bearing,
            weapon: bay.map(|b| (b.count, b.name.clone())),
            weapon_air_to_air: bay.is_some_and(|b| b.air_to_air),
            gun_rounds: self.stores.gun_rounds,
            message: (self.messages.timer > 0.0).then(|| self.messages.text.clone()),
            accel: self.messages.accel,
            training: self.messages.training,
            camera_sight,
            gun_lead: (self.lead.x, self.lead.y),
            cues: self.cues.clone(),
        })
    }
}

/// HUD lines drawn over the 3D view: colour 0 would be see-through there, so black is
/// drawn with colour 5 (black in every flight palette).
fn ink3d(c: u8) -> u8 {
    if c == 0 { 5 } else { c }
}

/// Draws a number left-aligned (`1A18:8B3B`).
fn number(c: &mut Canvas, fonts: &Fonts, v: i64, x: i32, y: i32, col: u8) {
    fonts.text(c, x, y, v.to_string().as_bytes(), col);
}

/// Draws a number right-aligned at x (`1A18:8B5C`: x - 4*len).
fn number_right(c: &mut Canvas, fonts: &Fonts, v: i64, x: i32, y: i32, col: u8) {
    let s = v.to_string();
    fonts.text(c, x - 4 * s.len() as i32, y, s.as_bytes(), col);
}

/// Box outline (x1,y1)-(x2,y2) inclusive (`1A18:88A8`).
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
    let (col, col2) = f.state.colours();
    let atlas = if f.state.night { &art.atlas_night } else { &art.sprites };
    // HUD text and lines are clipped to the HUD area of the back page (y 0..90).
    c.set_clip(0, 0, 319, 90);
    overlay(c, atlas, fonts, f, col, col2);
    instruments(c, atlas, fonts, f, col, col2);
    c.reset_clip();
}

/// `1A18:7898`: texts, flight path marker, bars, sights, digital readouts, waypoint cue,
/// messages.
fn overlay(c: &mut Canvas, atlas: &Canvas, fonts: &Fonts, f: &HudFrame, col: u8, col2: u8) {
    let st = &f.state;
    // Weapon line "n NAME" at (53,85); gun line "GUN nnn" at (239,85).
    if let Some((count, name)) = &f.weapon {
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
    if st.mode == HudMode::Air {
        // Gun sight: atlas (38,76) 13x11, colour 15.
        let x = CX + (f.gun_lead.0 >> 4);
        let y = CY + (f.gun_lead.1 >> 4);
        if x > 10 && x < 309 && y > 8 && y < 86 {
            c.blit(atlas, 38, 76, 13, 11, x - 6, y - 5, Some(0), Some(15));
        }
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
    // HUD message at y=20, centred.
    if let Some(m) = &f.message {
        let s = upper(m);
        fonts.text(c, 160 - 2 * (s.len() as i32 & !1), 20, &s, col);
    }
}

/// Ellipse outline (16 segments) around (x, y).
fn ellipse(c: &mut Canvas, x: i32, y: i32, rx: i32, ry: i32, col: u8) {
    let mut prev: Option<(i32, i32)> = None;
    for k in 0..=16 {
        let a = k << 12;
        let p = (x + (bsin(a) * rx as f32).round() as i32, y - (bcos(a) * ry as f32).round() as i32);
        if let Some(q) = prev {
            c.line(q.0, q.1, p.0, p.1, col);
        }
        prev = Some(p);
    }
}

/// Tracking box (`1A18:B3E6`): half width `size`, half height `size - size/4`, with the LOCK
/// oval around it (contracted when in range).
fn target_box(c: &mut Canvas, x: i32, y: i32, size: i32, oval: bool, red: bool, col: u8) {
    let (hw, hh) = (size, size - size / 4);
    frame_box(c, x - hw, y - hh, x + hw, y + hh, col);
    if oval {
        let grow = if red { 3 } else { 7 };
        ellipse(c, x, y, hw + grow, hh + grow - 1, col);
    }
}

/// 4-pixel diamond (`1A18:B339`).
fn diamond(c: &mut Canvas, x: i32, y: i32, col: u8) {
    c.line(x, y - 4, x + 4, y, col);
    c.line(x + 4, y, x, y + 4, col);
    c.line(x, y + 4, x - 4, y, col);
    c.line(x - 4, y, x, y - 4, col);
}

/// Targeting cues (weapons.md 5.4.3 / 5.4.5): tracking box and lock oval, CCIP bullseye with
/// its fall line, ranging bar and flight-path guide diamond.
fn weapon_cues(c: &mut Canvas, f: &HudFrame, col: u8) {
    let cues = &f.cues;
    if let Some(b) = cues.target_box {
        let colour = if b.colour == 0 { col } else { b.colour };
        target_box(c, b.x, b.y, b.size, b.oval, b.red, colour);
    }
    if let Some((x, y)) = cues.ccip {
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
    if let Some((x, y)) = cues.diamond {
        diamond(c, x, y, col);
    }
}

/// Difficulty divisor of the landing sink limit (`[DS:98F4]+0x40`; STUB: rookie).
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

/// `2AC0:000C`: tapes, heading scale, nose cross, G, pitch ladder.
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
        // The slot for the lowest label shows "900" when it is the 0 ft mark (VGAME quirk).
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

/// Pitch ladder (`2AC0:04AC..09CD`): a rung every 10 degrees = 52 units, rotated by roll and
/// squashed vertically by 0.75; labels at both ends.
fn pitch_ladder(c: &mut Canvas, fonts: &Fonts, pitch: i16, roll: i16, col: u8) {
    // Label x/y offsets by roll (DS:264C/26CC/274C), 64 entries.
    const LX: [i8; 64] = [
        2, 2, 2, 2, 2, 1, 1, 1, 0, 0, 0, 0, -1, -1, -2, -2, -3, -4, -4, -5, -5, -6, -6, -7, -7, -8, -8, -8, -9, -9, -9,
        -9, -9, -9, -9, -9, -9, -8, -8, -8, -7, -7, -6, -6, -5, -5, -4, -4, -3, -2, -2, -1, -1, 0, 0, 0, 0, 1, 1, 1, 2,
        2, 2, 2,
    ];
    const LY: [i8; 64] = [
        -1, -2, -2, -3, -3, -4, -4, -5, -5, -6, -6, -6, -7, -7, -7, -7, -7, -7, -7, -7, -7, -6, -6, -6, -5, -5, -4, -4,
        -3, -3, -2, -2, -1, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 2, 2, 1, 1, 0, 0, 0, 0,
    ];
    const LXN: [i8; 64] = [
        0, 0, 0, 0, 0, 0, 0, 0, -1, -1, -2, -2, -3, -3, -4, -4, -5, -6, -6, -7, -7, -8, -8, -9, -9, -10, -10, -10, -11,
        -11, -11, -11, -11, -11, -11, -11, -11, -10, -10, -10, -9, -9, -8, -8, -7, -7, -6, -6, -5, -4, -4, -3, -3, -2,
        -2, -1, -1, 0, 0, 0, 0, 0, 0, 0,
    ];
    const NAMES: [&[u8]; 23] = [
        b"-70", b"-80", b"-90", b"-80", b"-70", b"-60", b"-50", b"-40", b"-30", b"-20", b"-10", b"00", b"10", b"20",
        b"30", b"40", b"50", b"60", b"70", b"80", b"90", b"80", b"70",
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
    // Rotation by roll with the original's Q15 maths (sin of -roll):
    // x' = cos*x - s*y (truncated), y' = 0.75*(cos*y + s*x) as (3*a*b >> 16) >> 1.
    let (cf, sf) = (bcos(-(roll as i32)), bsin(-(roll as i32)));
    let (cq, sq) = ((cf * 32767.0).round() as i64, (sf * 32767.0).round() as i64);
    let mul2 = |a: i64, b: i32| (a * b as i64 * 2) >> 16;
    let mul3 = |a: i64, b: i32| ((a * b as i64 * 3) >> 16) >> 1;
    let rot = |x: i32, y: i32| -> (i32, i32) { ((mul2(cq, x) - mul2(sq, y)) as i32, (mul3(cq, y) + mul3(sq, x)) as i32) };
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
        // Lines are clipped to y 16..86 (VGAME 2A19:0326, measured), labels to (78,20)-(241,89).
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
