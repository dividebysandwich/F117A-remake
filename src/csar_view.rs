//! Status panel for the downed pilot: the rescue helicopter, the Hinds, flares and time on
//! the ground (the cockpit displays are gone with the aircraft), with buttons for the signal
//! flare and time acceleration.
//!
//! On the desktop a 2D overlay clicked with the mouse (the keys 1, Shift-Z and Shift-X work as
//! well); in VR a screen floating at chest height in front of the pilot, pointed at with either
//! controller (a laser shows while it is on the screen) and pressed with the trigger.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use stealth_sim::csar::CsarState;
use stealth_sim::math::approx;
use stealth_sim::types::fflags;

use crate::camera::{HeadAnchor, RigOffset, View, ViewState};
use crate::canvas::{Canvas, CanvasTexture};
use crate::cockpit3d::Hands;
use crate::combat::{Combat, TimeAccel};
use crate::font::{Align, stroke_text_at, stroke_width};
use crate::input::{Action, ActionEvent, InputBlock};
use crate::settings::{AppState, Settings};
use crate::world::WU;

pub struct CsarViewPlugin;

impl Plugin for CsarViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                update_panel,
                click_buttons,
                style_buttons,
                (spawn_vr_screen, vr_pointer, draw_vr_screen)
                    .chain()
                    .run_if(|s: Res<Settings>| s.vr),
            )
                .run_if(in_state(AppState::Flight)),
        )
        .add_systems(OnExit(AppState::Flight), despawn_panel);
    }
}

#[derive(Component)]
struct CsarPanel;

#[derive(Component)]
struct CsarText;

/// The panel's buttons.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum CsarButton {
    Flare,
    Faster,
    Normal,
}

const BUTTONS: [CsarButton; 3] = [CsarButton::Flare, CsarButton::Faster, CsarButton::Normal];

impl CsarButton {
    fn label(self) -> &'static str {
        match self {
            CsarButton::Flare => "SIGNAL FLARE",
            CsarButton::Faster => "FASTER",
            CsarButton::Normal => "NORMAL TIME",
        }
    }

    fn key(self) -> &'static str {
        match self {
            CsarButton::Flare => "1",
            CsarButton::Faster => "SHIFT-Z",
            CsarButton::Normal => "SHIFT-X",
        }
    }

    fn action(self) -> Action {
        match self {
            CsarButton::Flare => Action::Flare,
            CsarButton::Faster => Action::TimeAccel,
            CsarButton::Normal => Action::TimeNormal,
        }
    }

    /// The time button of the current time rate is lit.
    fn lit(self, accel: &TimeAccel) -> bool {
        match self {
            CsarButton::Flare => false,
            CsarButton::Faster => accel.on,
            CsarButton::Normal => !accel.on,
        }
    }
}

/// Map units -> km.
fn km(map: i32) -> f32 {
    map as f32 * 32.0 * WU as f32 / 1000.0
}

/// Compass point of a bearing from (x, y) to (tx, ty) in map units (y grows south).
fn compass(x: i32, y: i32, tx: i32, ty: i32) -> &'static str {
    let deg = ((tx - x) as f32).atan2((y - ty) as f32).to_degrees().rem_euclid(360.0);
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][((deg + 22.5) / 45.0) as usize % 8]
}

fn status(combat: &Combat) -> Option<Vec<String>> {
    let sim = &combat.sim;
    // A campaign has no rescue: the sortie ends when the parachute is down.
    if sim.campaign().is_some() {
        return None;
    }
    let (x, y, alt) = sim.chute()?;
    let Some(c) = sim.csar() else {
        return Some(vec![if alt > 0 {
            "PARACHUTE DEPLOYED".into()
        } else {
            "ON THE GROUND".into()
        }]);
    };
    let mut lines = vec!["DOWN IN ENEMY TERRITORY".to_string()];
    let r = sim.flights()[c.rescue];
    let d = approx(r.x - x, r.y - y);
    lines.push(match c.state {
        CsarState::Pickup(_) => "RESCUE   UH-60 OVERHEAD - PICKUP".into(),
        _ if c.located => {
            format!("RESCUE   UH-60 {:.1} KM {} - INBOUND", km(d), compass(x, y, r.x, r.y))
        }
        _ => format!(
            "RESCUE   UH-60 {:.1} KM {} - FIRE A FLARE WHEN CLOSE",
            km(d),
            compass(x, y, r.x, r.y)
        ),
    });
    let hinds: Vec<i32> = c
        .hunters
        .iter()
        .map(|&h| sim.flights()[h])
        .filter(|u| u.flags & fflags::DESTROYED == 0 && u.flags & fflags::EXISTS != 0)
        .map(|u| approx(u.x - x, u.y - y))
        .collect();
    if let Some(&n) = hinds.iter().min() {
        lines.push(format!("ENEMY    {} HIND, NEAREST {:.1} KM", hinds.len(), km(n)));
    }
    let secs = c.frames / 15;
    lines.push(format!("FLARES   {}      ON THE GROUND {}:{:02}", c.flares, secs / 60, secs % 60));
    Some(lines)
}

/// The status while the downed pilot's view is shown.
fn shown_status(combat: Option<&Combat>, view: &ViewState) -> Option<Vec<String>> {
    combat.filter(|c| view.view == View::Pilot && c.end.is_none()).and_then(status)
}

const GREEN: Color = Color::srgb(0.3, 1.0, 0.45);

fn update_panel(
    mut commands: Commands,
    combat: Option<Res<Combat>>,
    view: Res<ViewState>,
    panel: Query<Entity, With<CsarPanel>>,
    mut texts: Query<&mut Text, With<CsarText>>,
) {
    let text = shown_status(combat.as_deref(), &view).map(|l| l.join("\n"));
    match (text, panel.single()) {
        (Some(t), Ok(_)) => {
            for mut s in &mut texts {
                if s.0 != t {
                    s.0 = t.clone();
                }
            }
        }
        (Some(t), Err(_)) => {
            let font = |px: f32| TextFont { font_size: bevy::text::FontSize::Px(px), ..default() };
            commands
                .spawn((
                    CsarPanel,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(16),
                        bottom: px(16),
                        padding: UiRect::all(px(12)),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                ))
                .with_children(|p| {
                    p.spawn((CsarText, Text::new(t), font(18.0), TextColor(GREEN)));
                    p.spawn(Node { column_gap: px(8), ..default() }).with_children(|row| {
                        for b in BUTTONS {
                            row.spawn((
                                b,
                                Button,
                                Node {
                                    padding: UiRect::axes(px(12), px(6)),
                                    border: UiRect::all(px(2)),
                                    ..default()
                                },
                                BorderColor::all(GREEN),
                                BackgroundColor(Color::NONE),
                            ))
                            .with_child((
                                Text::new(format!("{}  [{}]", b.label(), b.key())),
                                font(16.0),
                                TextColor(GREEN),
                            ));
                        }
                    });
                });
        }
        (None, Ok(e)) => commands.entity(e).despawn(),
        (None, Err(_)) => {}
    }
}

/// Mouse clicks on the 2D buttons.
fn click_buttons(
    buttons: Query<(&Interaction, &CsarButton), Changed<Interaction>>,
    block: Res<InputBlock>,
    mut actions: MessageWriter<ActionEvent>,
) {
    for (i, b) in &buttons {
        if *i == Interaction::Pressed && !block.0 {
            actions.write(ActionEvent(b.action()));
        }
    }
}

/// Lit (the current time rate) and hovered buttons.
fn style_buttons(
    accel: Res<TimeAccel>,
    mut buttons: Query<(&Interaction, &CsarButton, &mut BackgroundColor, &Children)>,
    mut colors: Query<&mut TextColor>,
) {
    for (i, b, mut bg, children) in &mut buttons {
        let lit = b.lit(&accel);
        let want = match (lit, i) {
            (true, _) => GREEN,
            (false, Interaction::None) => Color::NONE,
            (false, _) => Color::srgba(0.3, 1.0, 0.45, 0.25),
        };
        bg.set_if_neq(BackgroundColor(want));
        for c in children.iter() {
            if let Ok(mut t) = colors.get_mut(c) {
                t.set_if_neq(TextColor(if lit { Color::BLACK } else { GREEN }));
            }
        }
    }
}

fn despawn_panel(mut commands: Commands, q: Query<Entity, With<CsarPanel>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// --- VR

/// The screen in the headset (child of the head anchor, whose frame is the recentred headset
/// space under the parachute), with the button the pointer is on.
#[derive(Component, Default)]
struct VrScreen {
    hovered: Option<CsarButton>,
}

#[derive(Component)]
struct VrLaser;

/// Screen size (metres) and place relative to the recentred head: at chest height, a forearm
/// ahead, facing the eye.
const SIZE: Vec2 = Vec2::new(0.5, 0.275);
const POSITION: Vec3 = Vec3::new(0.0, -0.32, -0.5);
/// Canvas pixels.
const W: usize = 800;
const H: usize = 440;
/// The button row (canvas pixels).
const BUTTON_TOP: f32 = 316.0;
const BUTTON_H: f32 = 100.0;
const BUTTON_GAP: f32 = 24.0;
/// Palette colours (as the in-flight menu): background, bright and dim green, dark highlight.
const BG: u8 = 5;
const BRIGHT: u8 = 47;
const DIM: u8 = 45;
const HOVER_BG: u8 = 44;

/// Left and right edge of button `i` (canvas pixels).
fn button_span(i: usize) -> (f32, f32) {
    let w = (W as f32 - BUTTON_GAP * (BUTTONS.len() + 1) as f32) / BUTTONS.len() as f32;
    let x0 = BUTTON_GAP + i as f32 * (w + BUTTON_GAP);
    (x0, x0 + w)
}

fn button_at(p: Vec2) -> Option<CsarButton> {
    if !(BUTTON_TOP..BUTTON_TOP + BUTTON_H).contains(&p.y) {
        return None;
    }
    (0..BUTTONS.len())
        .find(|&i| (button_span(i).0..button_span(i).1).contains(&p.x))
        .map(|i| BUTTONS[i])
}

fn spawn_vr_screen(
    mut commands: Commands,
    heads: Query<Entity, With<HeadAnchor>>,
    screens: Query<(), With<VrScreen>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(head) = heads.single() else { return };
    if !screens.is_empty() {
        return;
    }
    let tex = CanvasTexture::new(Canvas::new(W, H), &mut images, false);
    if let Some(mut img) = images.get_mut(&tex.image) {
        img.sampler = bevy::image::ImageSampler::linear();
    }
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(tex.image.clone()),
        unlit: true,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    });
    let screen = commands
        .spawn((
            VrScreen::default(),
            tex,
            Mesh3d(meshes.add(Rectangle::new(SIZE.x, SIZE.y))),
            MeshMaterial3d(material),
            Transform::from_translation(POSITION).looking_to(POSITION, Vec3::Y),
            Visibility::Hidden,
            NotShadowCaster,
        ))
        .id();
    let laser = commands
        .spawn((
            VrLaser,
            Mesh3d(meshes.add(Cuboid::new(0.003, 0.003, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: GREEN,
                unlit: true,
                fog_enabled: false,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
        ))
        .id();
    commands.entity(head).add_children(&[screen, laser]);
}

/// The controllers pointing at the screen: the laser, the hovered button, trigger presses.
#[allow(clippy::too_many_arguments)]
fn vr_pointer(
    mut hands: ResMut<Hands>,
    offset: Res<RigOffset>,
    combat: Option<Res<Combat>>,
    view: Res<ViewState>,
    block: Res<InputBlock>,
    mut screens: Query<(&Transform, &mut VrScreen)>,
    mut laser: Query<(&mut Transform, &mut Visibility), (With<VrLaser>, Without<VrScreen>)>,
    mut actions: MessageWriter<ActionEvent>,
) {
    let Ok((screen_tf, mut screen)) = screens.single_mut() else { return };
    let Ok((mut ltf, mut lvis)) = laser.single_mut() else { return };
    let shown = shown_status(combat.as_deref(), &view).is_some() && !block.0;
    let inv = screen_tf.compute_affine().inverse();
    // The right hand points first, the left one if the right is not on the screen.
    let mut hit = None;
    for i in [1, 0] {
        let Some(aim) = hands.hands[i].aim_raw.filter(|_| shown) else { continue };
        // The aim pose is in the tracking space; the head anchor's frame is the recentred one.
        let aim = offset.0 * aim;
        let (o, d) = (aim.translation, aim.forward().as_vec3());
        let (lo, ld) = (inv.transform_point3(o), inv.transform_vector3(d));
        if ld.z.abs() < 1e-6 {
            continue;
        }
        let t = -lo.z / ld.z;
        let p = lo + ld * t;
        if t > 0.0 && p.x.abs() <= SIZE.x / 2.0 && p.y.abs() <= SIZE.y / 2.0 {
            hit = Some((
                i,
                o,
                d,
                t,
                Vec2::new((p.x / SIZE.x + 0.5) * W as f32, (0.5 - p.y / SIZE.y) * H as f32),
            ));
            break;
        }
    }
    let Some((i, o, d, t, px)) = hit else {
        screen.hovered = None;
        lvis.set_if_neq(Visibility::Hidden);
        return;
    };
    *ltf = Transform::from_translation(o + d * t / 2.0)
        .looking_to(d, Vec3::Y)
        .with_scale(Vec3::new(1.0, 1.0, t));
    lvis.set_if_neq(Visibility::Inherited);
    let hovered = button_at(px);
    let hand = &mut hands.hands[i];
    if hovered.is_some() && hovered != screen.hovered {
        hand.haptic = hand.haptic.max(0.1);
    }
    screen.hovered = hovered;
    if let Some(b) = hovered
        && hand.trigger_pulled()
    {
        hand.haptic = hand.haptic.max(0.6);
        actions.write(ActionEvent(b.action()));
    }
}

/// Text height up to `h`, smaller if the text would be wider than `width`.
fn fit(s: &str, h: f32, width: f32) -> f32 {
    h.min(h * width / stroke_width(s, h).max(1.0))
}

fn draw_vr_screen(
    combat: Option<Res<Combat>>,
    view: Res<ViewState>,
    accel: Res<TimeAccel>,
    mut q: Query<(&mut CanvasTexture, &mut Visibility, &VrScreen)>,
) {
    let Ok((mut tex, mut vis, screen)) = q.single_mut() else { return };
    let lines = shown_status(combat.as_deref(), &view);
    vis.set_if_neq(if lines.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    let Some(lines) = lines else { return };
    let c = &mut tex.canvas;
    c.clear(BG);
    c.frame(Vec2::new(6.0, 6.0), Vec2::new(W as f32 - 7.0, H as f32 - 7.0), 3.0, DIM);
    stroke_text_at(
        c,
        W as f32 / 2.0,
        46.0,
        &lines[0],
        fit(&lines[0], 26.0, W as f32 - 72.0),
        2.8,
        BRIGHT,
        Align::Centre,
    );
    // The rescue's hint goes on a line of its own (larger text than one long line).
    let rows = lines[1..].iter().flat_map(|l| match l.split_once(" - ") {
        Some((a, b)) => vec![a.to_string(), format!("         {b}")],
        None => vec![l.clone()],
    });
    for (k, row) in rows.enumerate() {
        stroke_text_at(
            c,
            36.0,
            110.0 + 44.0 * k as f32,
            &row,
            fit(&row, 20.0, W as f32 - 72.0),
            2.8,
            BRIGHT,
            Align::Left,
        );
    }
    for (i, b) in BUTTONS.into_iter().enumerate() {
        let (x0, x1) = button_span(i);
        let lit = b.lit(&accel);
        if lit || screen.hovered == Some(b) {
            c.fill_rect(
                x0 as i32,
                BUTTON_TOP as i32,
                (x1 - x0) as i32,
                BUTTON_H as i32,
                if lit { BRIGHT } else { HOVER_BG },
            );
        }
        c.frame(Vec2::new(x0, BUTTON_TOP), Vec2::new(x1, BUTTON_TOP + BUTTON_H), 3.0, BRIGHT);
        let h = fit(b.label(), 20.0, x1 - x0 - 28.0);
        stroke_text_at(
            c,
            (x0 + x1) / 2.0,
            BUTTON_TOP + BUTTON_H / 2.0,
            b.label(),
            h,
            2.8,
            if lit { BG } else { BRIGHT },
            Align::Centre,
        );
    }
    tex.dirty = true;
}
