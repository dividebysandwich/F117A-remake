//! Pushable cockpit controls.
//!
//! Panel buttons are caps carrying a face of the instrument atlas (a lamp or label, so lit/unlit
//! comes from the panel drawing code); the layout says which faces are caps. The modern
//! panel's MFD bezel buttons are caps too, with page-dependent labels (`mfd::osb`). Extra
//! buttons on two side panels carry text labels. Every control has a local frame with +Z pointing out of
//! its face. A VR fingertip touching a control highlights it (with a light haptic tick) and
//! pulling the trigger then fires it; a mouse click or a script `press=` fires it directly.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::displays::{Faces, side_key};
use super::geometry::{panel_point, panel_rect, panel_rotation, uv_quad};
use super::layout::{Effect, PanelLayout};
use super::{CockpitMaterials, CockpitRoot, Hands, spawn_box};
use crate::camera::{EyeCamera, View, ViewState};
use crate::canvas::{Canvas, CanvasTexture};
use crate::flight::{Aircraft, Player};
use crate::font::Fonts;
use crate::hud::{HudMode, HudState};
use crate::input::{Action, ActionEvent};
use crate::mfd::{CamDir, MfdCommand, MfdState, OsbFn, cmd_action, osb};

/// Presses the 3D-cockpit control bound to an action (scripts).
#[derive(Message, Debug, Clone, Copy)]
pub struct PressControl(pub Action);

impl Effect {
    /// The key action a script `press=` names for this control (None: blank bezel button).
    fn action(self, mfd: &MfdState) -> Option<Action> {
        Some(match self {
            Effect::Action(a) => a,
            Effect::HudMode(_) => Action::HudMode,
            Effect::LeftPage(_) => Action::Maps,
            Effect::CamCycle => Action::CamAhead,
            Effect::TimeToggle => Action::TimeAccel,
            Effect::Osb(side, slot) => match osb(side, slot, mfd)?.1 {
                OsbFn::Cmd(c) => cmd_action(c),
                OsbFn::Action(a) => a,
            },
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Cap on the panel carrying an atlas face (uv min, max), size in panel pixels.
    Cap,
    /// MFD bezel button.
    Osb,
    /// Labelled button on a side panel (side -1 left / 1 right, row, column).
    Side(f32, u8, u8, &'static str),
    /// Landing gear lever (left of the panel).
    GearLever,
    /// Ejection handle (front of the seat pan).
    EjectHandle,
}

const fn side(s: f32, row: u8, col: u8, label: &'static str, a: Action) -> (Kind, Effect) {
    (Kind::Side(s, row, col, label), Effect::Action(a))
}

/// Left panel: targeting and weapons; right panel: auxiliary functions.
const SIDE_BUTTONS: &[(Kind, Effect)] = &[
    side(-1.0, 0, 0, "NEAR", Action::NearestTarget),
    side(-1.0, 0, 1, "DESG", Action::DesignateTarget),
    side(-1.0, 1, 0, "NEXT", Action::SelectTarget),
    side(-1.0, 1, 1, "NONE", Action::ClearTarget),
    side(-1.0, 2, 0, "WPN", Action::SelectWeapon),
    side(-1.0, 2, 1, "FIRE", Action::FireWeapon),
    side(1.0, 0, 0, "NVG", Action::Nvg),
    side(1.0, 0, 1, "ILS", Action::Ils),
    (Kind::Side(1.0, 1, 0, "TIME"), Effect::TimeToggle),
    side(1.0, 1, 1, "DCLT", Action::HudDeclutter),
    side(1.0, 2, 0, "VOL", Action::Volume),
    side(1.0, 2, 1, "DTL", Action::Detail),
    (Kind::GearLever, Effect::Action(Action::Gear)),
    (Kind::EjectHandle, Effect::Action(Action::Eject)),
];

/// Ejection handle position (grabbed and pulled in VR, see `levers`).
pub(super) const EJECT_HANDLE: Vec3 = Vec3::new(0.0, -0.74, -0.20);

/// Cap height above the mount and press travel (metres).
const CAP_HEIGHT: f32 = 0.006;
const TRAVEL: f32 = 0.005;
/// Side panel button cap height.
const SIDE_HEIGHT: f32 = 0.008;
/// Fingertip radius.
const FINGER: f32 = 0.008;
/// A fingertip touches a control within this distance of its face (in front or pushed in), and
/// this far beyond its footprint.
const TOUCH_GAP: f32 = 0.012;
const TOUCH_MARGIN: f32 = 0.006;
/// Highlight frame: gap around the footprint and bar width.
const HIGHLIGHT_GAP: f32 = 0.0015;
const HIGHLIGHT_BAR: f32 = 0.0015;

#[derive(Component)]
pub(super) struct Control {
    kind: Kind,
    effect: Effect,
    half: Vec2,
    height: f32,
    travel: f32,
    /// Current push depth (metres), from the fingers or a click animation.
    depth: f32,
    /// A VR fingertip touches it.
    touched: bool,
    /// Seconds left of a click/script press animation.
    click: f32,
}

/// The moving part of a control (cap, lever arm).
#[derive(Component)]
pub(super) struct Moving;

/// Frame around a control shown while a fingertip touches it.
#[derive(Component)]
pub(super) struct Highlight;

/// A highlight frame (hidden) around a footprint of half size `half`, at `z` (under the
/// moving part, so it goes in with the cap).
fn spawn_highlight(commands: &mut Commands, mats: &CockpitMaterials, half: Vec2, z: f32) -> Entity {
    let (hx, hy) = (half.x + HIGHLIGHT_GAP + HIGHLIGHT_BAR / 2.0, half.y + HIGHLIGHT_GAP + HIGHLIGHT_BAR / 2.0);
    let bars = [
        (Vec2::new(0.0, hy), Vec2::new(2.0 * hx + HIGHLIGHT_BAR, HIGHLIGHT_BAR)),
        (Vec2::new(0.0, -hy), Vec2::new(2.0 * hx + HIGHLIGHT_BAR, HIGHLIGHT_BAR)),
        (Vec2::new(hx, 0.0), Vec2::new(HIGHLIGHT_BAR, 2.0 * hy - HIGHLIGHT_BAR)),
        (Vec2::new(-hx, 0.0), Vec2::new(HIGHLIGHT_BAR, 2.0 * hy - HIGHLIGHT_BAR)),
    ];
    let kids: Vec<Entity> = bars
        .into_iter()
        .map(|(c, s)| {
            commands
                .spawn((
                    Mesh3d(mats.unit_cube.clone()),
                    MeshMaterial3d(mats.highlight.clone()),
                    Transform::from_xyz(c.x, c.y, z).with_scale(s.extend(0.001)),
                    NoFrustumCulling,
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
    commands.spawn((Highlight, Transform::default(), Visibility::Hidden)).add_children(&kids).id()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_controls(
    commands: &mut Commands,
    root: Entity,
    layout: &PanelLayout,
    faces: &Faces,
    mats: &CockpitMaterials,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    fonts: &Fonts,
) {
    // Panel caps and bezel buttons: (rect, uv, kind, effect).
    let mut caps = Vec::new();
    for (p, &uv) in layout.items.iter().zip(&faces.items) {
        if let Some(effect) = p.cap {
            caps.push((p.rect, uv, Kind::Cap, effect));
        }
    }
    for m in &layout.mfds {
        for (slot, r) in layout.osbs(m) {
            caps.push((r, faces.osbs[&(side_key(m.side), slot)], Kind::Osb, Effect::Osb(m.side, slot)));
        }
    }
    for (r, (uv0, uv1), kind, effect) in caps {
        let (cx, cy) = r.centre();
        let base = layout.surface(cx, cy);
        let (_, size) = panel_rect(r.x, r.y, r.w, r.h, base);
        let tf = Transform::from_translation(panel_point(cx, cy, base)).with_rotation(panel_rotation());
        let face = commands
            .spawn((
                Mesh3d(meshes.add(uv_quad(size, uv0, uv1))),
                MeshMaterial3d(faces.material.clone()),
                Transform::from_xyz(0.0, 0.0, CAP_HEIGHT),
                NoFrustumCulling,
                NotShadowCaster,
            ))
            .id();
        let skirt = commands
            .spawn((
                Mesh3d(mats.unit_cube.clone()),
                MeshMaterial3d(mats.dark.clone()),
                Transform::from_xyz(0.0, 0.0, CAP_HEIGHT / 2.0 - 0.0005).with_scale(Vec3::new(size.x * 0.98, size.y * 0.96, CAP_HEIGHT)),
                NoFrustumCulling,
            ))
            .id();
        let highlight = spawn_highlight(commands, mats, size / 2.0, CAP_HEIGHT);
        let moving = commands.spawn((Moving, Transform::default(), Visibility::default())).add_children(&[face, skirt, highlight]).id();
        let c = commands
            .spawn((Control::new(kind, effect, size / 2.0, CAP_HEIGHT, TRAVEL), tf, Visibility::default()))
            .add_child(moving)
            .id();
        commands.entity(root).add_child(c);
    }

    for &(kind, effect) in SIDE_BUTTONS {
        match kind {
            Kind::Cap | Kind::Osb => {}
            Kind::Side(s, row, col, label) => {
                let size = Vec2::new(0.034, 0.022);
                let (centre, rot) = side_slot(s, row, col);
                let mut canvas = Canvas::new(28, 11);
                canvas.clear(44);
                let w = fonts.text(&mut Canvas::new(64, 16), 0, 0, label.as_bytes(), 47);
                fonts.text(&mut canvas, (28 - w) / 2, 3, label.as_bytes(), 47);
                let tex = CanvasTexture::new(canvas, images, false);
                let m = materials.add(StandardMaterial {
                    base_color_texture: Some(tex.image.clone()),
                    emissive_texture: Some(tex.image.clone()),
                    emissive: LinearRgba::rgb(1.0, 1.0, 1.0),
                    unlit: true,
                    fog_enabled: false,
                    ..default()
                });
                let face = commands
                    .spawn((
                        Mesh3d(meshes.add(uv_quad(size, Vec2::ZERO, Vec2::ONE))),
                        MeshMaterial3d(m.clone()),
                        super::displays::LitMaterial(m),
                        tex,
                        Transform::from_xyz(0.0, 0.0, SIDE_HEIGHT),
                        NoFrustumCulling,
                    ))
                    .id();
                // Cap body: its top stays just below the label face (no z-fighting).
                let skirt = commands
                    .spawn((
                        Mesh3d(mats.unit_cube.clone()),
                        MeshMaterial3d(mats.dark.clone()),
                        Transform::from_xyz(0.0, 0.0, SIDE_HEIGHT / 2.0 - 0.0005)
                            .with_scale(Vec3::new(size.x * 0.98, size.y * 0.96, SIDE_HEIGHT)),
                        NoFrustumCulling,
                    ))
                    .id();
                let highlight = spawn_highlight(commands, mats, size / 2.0, SIDE_HEIGHT);
                let moving = commands.spawn((Moving, Transform::default(), Visibility::default())).add_children(&[face, skirt, highlight]).id();
                let tf = Transform::from_translation(centre).with_rotation(rot);
                let c = commands
                    .spawn((Control::new(kind, effect, size / 2.0, SIDE_HEIGHT, TRAVEL), tf, Visibility::default()))
                    .add_child(moving)
                    .id();
                commands.entity(root).add_child(c);
                // Black bezel around the cap, standing slightly proud of the panel body.
                spawn_box(
                    commands,
                    root,
                    mats,
                    &mats.black,
                    tf * Transform::from_xyz(0.0, 0.0, 0.0005).with_scale(Vec3::new(size.x + 0.005, size.y + 0.005, 0.002)),
                );
            }
            Kind::GearLever => {
                // Mounted on the left side panel below the buttons, pushed to toggle; the arm
                // shows the gear position (down = gear down).
                let (centre, rot) = side_slot(-1.0, 4, 0);
                let centre = centre + rot * Vec3::new(0.025, 0.0, 0.0);
                let arm = commands
                    .spawn((
                        Mesh3d(mats.unit_cube.clone()),
                        MeshMaterial3d(mats.metal.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.03).with_scale(Vec3::new(0.008, 0.008, 0.06)),
                        NoFrustumCulling,
                    ))
                    .id();
                let wheel = commands
                    .spawn((
                        Mesh3d(meshes.add(Cylinder::new(0.016, 0.012))),
                        MeshMaterial3d(mats.panel_grey.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.062),
                        NoFrustumCulling,
                    ))
                    .id();
                let highlight = spawn_highlight(commands, mats, Vec2::splat(0.016), 0.069);
                let moving = commands.spawn((Moving, Transform::default(), Visibility::default())).add_children(&[arm, wheel, highlight]).id();
                let c = commands
                    .spawn((Control::new(kind, effect, Vec2::splat(0.02), 0.07, 0.01), Transform::from_translation(centre).with_rotation(rot), Visibility::default()))
                    .add_child(moving)
                    .id();
                commands.entity(root).add_child(c);
                // Slot plate.
                spawn_box(commands, root, mats, &mats.black, Transform::from_translation(centre).with_rotation(rot).with_scale(Vec3::new(0.02, 0.07, 0.004)));
            }
            Kind::EjectHandle => {
                // Yellow loop at the front of the seat pan; face points forward/up.
                let rot = Quat::from_rotation_x(-0.9);
                let centre = EJECT_HANDLE;
                let bar = commands
                    .spawn((
                        Mesh3d(mats.unit_cube.clone()),
                        MeshMaterial3d(mats.yellow.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.03).with_scale(Vec3::new(0.12, 0.014, 0.014)),
                        NoFrustumCulling,
                    ))
                    .id();
                let mut kids = vec![bar];
                for s in [-1.0f32, 1.0] {
                    kids.push(
                        commands
                            .spawn((
                                Mesh3d(mats.unit_cube.clone()),
                                MeshMaterial3d(mats.rubber.clone()),
                                Transform::from_xyz(0.055 * s, 0.0, 0.015).with_scale(Vec3::new(0.012, 0.012, 0.03)),
                                NoFrustumCulling,
                            ))
                            .id(),
                    );
                }
                let moving = commands.spawn((Moving, Transform::default(), Visibility::default())).add_children(&kids).id();
                let c = commands
                    .spawn((Control::new(kind, effect, Vec2::new(0.065, 0.02), 0.04, 0.02), Transform::from_translation(centre).with_rotation(rot), Visibility::default()))
                    .add_child(moving)
                    .id();
                commands.entity(root).add_child(c);
            }
        }
    }
    // Side panel bodies behind the buttons and the gear lever.
    for s in [-1.0f32, 1.0] {
        let (origin, rot) = side_slot(s, 0, 0);
        let c = origin + rot * Vec3::new(0.0225, -0.06, -0.021);
        spawn_box(commands, root, mats, &mats.panel_grey, Transform::from_translation(c).with_rotation(rot).with_scale(Vec3::new(0.11, 0.22, 0.04)));
    }
}

/// Side panel button slot: the panels sit left/right of the instrument panel, turned to face
/// the pilot.
fn side_slot(s: f32, row: u8, col: u8) -> (Vec3, Quat) {
    // Panel edge at the pixel column 0 / 320, row 120, pulled back a little.
    let edge = panel_point(if s < 0.0 { 0.0 } else { 320.0 }, 125.0, 0.0);
    let origin = edge + Vec3::new(0.0, -0.02, 0.12);
    let rot = Quat::from_rotation_y(-s * 0.75) * panel_rotation();
    let local = Vec3::new((col as f32 - 0.5) * 0.045, -(row as f32) * 0.035 + 0.04, 0.0);
    (origin + rot * local, rot)
}

impl Control {
    fn new(kind: Kind, effect: Effect, half: Vec2, height: f32, travel: f32) -> Self {
        Self { kind, effect, half, height, travel, depth: 0.0, touched: false, click: 0.0 }
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Fire<'w, 's> {
    actions: MessageWriter<'w, ActionEvent>,
    mfd_commands: MessageWriter<'w, MfdCommand>,
    hud: Res<'w, HudState>,
    mfd: Res<'w, MfdState>,
    accel: Res<'w, crate::combat::TimeAccel>,
    _p: std::marker::PhantomData<&'s ()>,
}

impl Fire<'_, '_> {
    fn fire(&mut self, effect: Effect) {
        match effect {
            Effect::Action(a) => {
                self.actions.write(ActionEvent(a));
            }
            Effect::HudMode(target) => {
                let cur = match self.hud.mode {
                    HudMode::Nav => 0,
                    HudMode::Air => 1,
                    HudMode::Gnd => 2,
                };
                for _ in 0..(target + 3 - cur) % 3 {
                    self.actions.write(ActionEvent(Action::HudMode));
                }
            }
            Effect::LeftPage(p) => {
                if self.mfd.left != p {
                    self.actions.write(ActionEvent(Action::Maps));
                }
            }
            Effect::Osb(side, slot) => match osb(side, slot, &self.mfd).map(|o| o.1) {
                Some(OsbFn::Cmd(c)) => {
                    self.mfd_commands.write(MfdCommand(c));
                }
                Some(OsbFn::Action(a)) => {
                    self.actions.write(ActionEvent(a));
                }
                None => {}
            },
            Effect::CamCycle => {
                let next = match self.mfd.cam_dir {
                    CamDir::Ahead => Action::CamRight,
                    CamDir::Right => Action::CamRear,
                    CamDir::Rear => Action::CamLeft,
                    CamDir::Left => Action::CamAhead,
                };
                self.actions.write(ActionEvent(next));
            }
            Effect::TimeToggle => {
                self.actions.write(ActionEvent(if self.accel.on { Action::TimeNormal } else { Action::TimeAccel }));
            }
        }
    }
}

/// Seconds a clicked control stays pushed in.
const CLICK_SECONDS: f32 = 0.18;

/// Fingertip touches with the trigger, mouse clicks and script presses.
#[allow(clippy::too_many_arguments)]
pub(super) fn interact(
    time: Res<Time<Real>>,
    settings: Res<crate::settings::Settings>,
    view: Res<ViewState>,
    mut hands: ResMut<Hands>,
    mut controls: Query<(Entity, &mut Control, &Transform)>,
    mut was_touching: Local<[Option<Entity>; 2]>,
    mut fire: Fire,
    mut presses: MessageReader<PressControl>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), With<EyeCamera>>,
    root: Query<&GlobalTransform, With<CockpitRoot>>,
) {
    let dt = time.delta_secs();
    for PressControl(a) in presses.read() {
        // Panel caps and bezel buttons before the side panel duplicates; of two bezel buttons
        // with the same key action (MAP / TAC), the one not selected.
        let mfd = fire.mfd.clone();
        let rank = |c: &Control| {
            let lit = matches!(c.effect, Effect::Osb(side, slot) if osb(side, slot, &mfd).is_some_and(|o| o.2));
            (!matches!(c.kind, Kind::Cap | Kind::Osb), lit)
        };
        if let Some((_, mut c, _)) = controls.iter_mut().filter(|(_, c, _)| c.effect.action(&mfd) == Some(*a)).min_by_key(|(_, c, _)| rank(c)) {
            c.click = CLICK_SECONDS;
            fire.fire(c.effect);
        } else {
            warn!("no 3D cockpit control for {a:?}");
        }
    }
    let cockpit_view = matches!(view.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear);

    // Mouse click: nearest control face under the cursor.
    if !settings.vr
        && cockpit_view
        && buttons.just_pressed(MouseButton::Left)
        && let (Ok(window), Ok((camera, cam_tf)), Ok(root_tf)) = (windows.single(), cams.single(), root.single())
        && let Some(cursor) = window.cursor_position()
        && let Ok(ray) = camera.viewport_to_world(cam_tf, cursor)
    {
        let inv = root_tf.affine().inverse();
        let o = inv.transform_point3(ray.origin);
        let d = inv.transform_vector3(*ray.direction).normalize();
        let mut best: Option<(f32, Mut<Control>)> = None;
        for (_, c, tf) in &mut controls {
            let n = tf.rotation * Vec3::Z;
            let plane = tf.translation + n * c.height;
            let denom = d.dot(n);
            if denom.abs() < 1e-5 {
                continue;
            }
            let t = (plane - o).dot(n) / denom;
            if t <= 0.0 || best.as_ref().is_some_and(|(bt, _)| *bt < t) {
                continue;
            }
            let local = tf.rotation.inverse() * (o + d * t - tf.translation);
            if local.x.abs() <= c.half.x && local.y.abs() <= c.half.y {
                best = Some((t, c));
            }
        }
        if let Some((_, mut c)) = best {
            c.click = CLICK_SECONDS;
            fire.fire(c.effect);
        }
    }

    // VR fingertips: each hand touches the control whose face is nearest its fingertip (not
    // while it holds the stick or throttle); the trigger fires it and holds it pushed in.
    let mut touching: [Option<(f32, Entity)>; 2] = [None; 2];
    for (e, mut c, tf) in &mut controls {
        c.click = (c.click - dt).max(0.0);
        for (h, hand) in hands.hands.iter().enumerate() {
            // The ejection handle has to be grabbed and pulled (no accidental pokes).
            if !hand.active || !cockpit_view || c.kind == Kind::EjectHandle {
                continue;
            }
            let local = tf.rotation.inverse() * (hand.finger - tf.translation);
            let inside = local.x.abs() <= c.half.x + TOUCH_MARGIN && local.y.abs() <= c.half.y + TOUCH_MARGIN && local.z > -0.04;
            if !inside {
                continue;
            }
            let face = c.height + FINGER;
            let gap = local.z - face;
            if hand.squeeze < 0.4 && gap < TOUCH_GAP && gap > -c.travel - TOUCH_GAP && touching[h].is_none_or(|(g, _)| gap.abs() < g) {
                touching[h] = Some((gap.abs(), e));
            }
        }
        c.depth = if c.click > 0.0 { c.travel } else { 0.0 };
    }
    for (h, hand) in hands.hands.iter_mut().enumerate() {
        let touched = touching[h].map(|(_, e)| e);
        if touched.is_some() && touched != was_touching[h] {
            hand.haptic = hand.haptic.max(0.1);
        }
        was_touching[h] = touched;
        if let Some(e) = touched
            && let Ok((_, mut c, _)) = controls.get_mut(e)
        {
            if hand.trigger_pulled() {
                hand.haptic = hand.haptic.max(0.6);
                c.click = CLICK_SECONDS;
                fire.fire(c.effect);
            }
            if hand.trigger > 0.7 {
                c.depth = c.travel;
            }
        }
    }
    for (e, mut c, _) in &mut controls {
        let touched = touching.iter().flatten().any(|&(_, t)| t == e);
        if c.touched != touched {
            c.touched = touched;
        }
    }
}

/// Moves the caps in, the gear lever to the gear position, and shows the touched controls'
/// highlights.
pub(super) fn animate(
    controls: Query<(&Control, &Children)>,
    mut moving: Query<(&mut Transform, &Children), With<Moving>>,
    mut highlights: Query<&mut Visibility, With<Highlight>>,
    aircraft: Query<&Aircraft, With<Player>>,
) {
    let gear = aircraft.single().is_ok_and(|a| a.gear_down);
    for (c, children) in &controls {
        for child in children.iter() {
            if let Ok((mut tf, parts)) = moving.get_mut(child) {
                for part in parts.iter() {
                    if let Ok(mut v) = highlights.get_mut(part) {
                        v.set_if_neq(if c.touched { Visibility::Inherited } else { Visibility::Hidden });
                    }
                }
                let mut t = Transform::from_xyz(0.0, 0.0, -c.depth);
                if c.kind == Kind::GearLever {
                    t.rotation = Quat::from_rotation_x(if gear { 0.5 } else { -0.5 });
                }
                if *tf != t {
                    *tf = t;
                }
            }
        }
    }
}
