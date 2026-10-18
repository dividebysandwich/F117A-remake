//! Pushable cockpit controls.
//!
//! Panel buttons are caps over the original panel lights (their face shows that part of the
//! live panel picture, so lit/unlit comes from `panel::PanelParams::draw`). Extra buttons on
//! two side panels carry text labels. Every control has a local frame with +Z pointing out of
//! its face; a VR fingertip pushing the cap in by most of its travel fires it once (with a
//! haptic tick), a mouse click or a script `press=` fires it directly.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::geometry::{panel_point, panel_rect, panel_rotation, uv_quad};
use super::{CockpitMaterials, CockpitRoot, Hands, spawn_box};
use crate::camera::{EyeCamera, View, ViewState};
use crate::canvas::{Canvas, CanvasTexture};
use crate::flight::{Aircraft, Player};
use crate::font::Fonts;
use crate::hud::{HudMode, HudState};
use crate::input::{Action, ActionEvent};
use crate::mfd::{CamDir, LeftMfd, MfdState};

/// Presses the 3D-cockpit control bound to an action (scripts).
#[derive(Message, Debug, Clone, Copy)]
pub struct PressControl(pub Action);

#[derive(Clone, Copy, PartialEq)]
enum Effect {
    Action(Action),
    /// Select a HUD mode (0 NAV, 1 AIR, 2 GND) by cycling.
    HudMode(u8),
    /// Select a left MFD page.
    LeftPage(LeftMfd),
    /// Next tracking camera direction.
    CamCycle,
}

impl Effect {
    fn action(self) -> Action {
        match self {
            Effect::Action(a) => a,
            Effect::HudMode(_) => Action::HudMode,
            Effect::LeftPage(_) => Action::Maps,
            Effect::CamCycle => Action::CamAhead,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Cap on the panel picture, (x, y, w, h) in panel pixels.
    PanelCap(i32, i32, i32, i32),
    /// Labelled button on a side panel (side -1 left / 1 right, row, column).
    Side(f32, u8, u8, &'static str),
    /// Landing gear lever (left of the panel).
    GearLever,
    /// Ejection handle (front of the seat pan).
    EjectHandle,
}

struct Def {
    kind: Kind,
    effect: Effect,
}

const fn cap(i: usize, effect: Effect) -> Def {
    let (x, y, w, h, _) = crate::panel::LIGHTS[i];
    Def { kind: Kind::PanelCap(x, y, w + 1, h + 1), effect }
}

const fn side(s: f32, row: u8, col: u8, label: &'static str, a: Action) -> Def {
    Def { kind: Kind::Side(s, row, col, label), effect: Effect::Action(a) }
}

const DEFS: &[Def] = &[
    cap(0, Effect::HudMode(0)),
    cap(1, Effect::HudMode(1)),
    cap(2, Effect::HudMode(2)),
    cap(3, Effect::Action(Action::DesignateTarget)),
    Def { kind: Kind::PanelCap(4, 147, 25, 9), effect: Effect::Action(Action::Chaff) },
    Def { kind: Kind::PanelCap(4, 157, 25, 9), effect: Effect::Action(Action::Flare) },
    Def { kind: Kind::PanelCap(4, 167, 25, 9), effect: Effect::Action(Action::Decoy) },
    cap(17, Effect::Action(Action::Ecm)),
    cap(18, Effect::Action(Action::IrJammer)),
    cap(22, Effect::LeftPage(LeftMfd::Map)),
    cap(23, Effect::LeftPage(LeftMfd::Tac)),
    cap(25, Effect::Action(Action::Flir)),
    cap(26, Effect::Action(Action::Ordnance)),
    cap(27, Effect::CamCycle),
    cap(28, Effect::Action(Action::SelectWaypoint)),
    cap(29, Effect::Action(Action::MissionOrders)),
    cap(30, Effect::Action(Action::CamAhead)),
    cap(31, Effect::Action(Action::BayDoors)),
    cap(32, Effect::Action(Action::Autopilot)),
    cap(33, Effect::Action(Action::Flaps)),
    cap(34, Effect::Action(Action::Gear)),
    cap(35, Effect::Action(Action::Brakes)),
    side(-1.0, 0, 0, "DCLT", Action::HudDeclutter),
    side(-1.0, 0, 1, "NITE", Action::DayNightHud),
    side(-1.0, 1, 0, "ZM+", Action::Zoom),
    side(-1.0, 1, 1, "ZM-", Action::Unzoom),
    side(-1.0, 2, 0, "ILS", Action::Ils),
    side(-1.0, 2, 1, "TIME", Action::TimeAccel),
    side(1.0, 0, 0, "WP-", Action::LastWaypoint),
    side(1.0, 0, 1, "WP+", Action::NextWaypoint),
    side(1.0, 1, 0, "WPN", Action::SelectWeapon),
    side(1.0, 1, 1, "TGT", Action::SelectTarget),
    side(1.0, 2, 0, "RST", Action::ResetWaypoint),
    side(1.0, 2, 1, "NORM", Action::TimeNormal),
    Def { kind: Kind::GearLever, effect: Effect::Action(Action::Gear) },
    Def { kind: Kind::EjectHandle, effect: Effect::Action(Action::Eject) },
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

#[derive(Component)]
pub(super) struct Control {
    def: usize,
    half: Vec2,
    height: f32,
    travel: f32,
    /// Current push depth (metres), from the fingers or a click animation.
    depth: f32,
    latched: bool,
    /// Per hand: the finger was in front of the face inside the footprint.
    armed: [bool; 2],
    /// Seconds left of a click/script press animation.
    click: f32,
}

/// The moving part of a control (cap, lever arm).
#[derive(Component)]
pub(super) struct Moving;

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_controls(
    commands: &mut Commands,
    root: Entity,
    mats: &CockpitMaterials,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    fonts: &Fonts,
) {
    // Cap faces get the panel material in `animate` (spawned in the same command batch).
    for (i, def) in DEFS.iter().enumerate() {
        match def.kind {
            Kind::PanelCap(x, y, w, h) => {
                let (centre, size) = panel_rect(x, y, w, h, 0.0);
                let tf = Transform::from_translation(centre).with_rotation(panel_rotation());
                let uv0 = Vec2::new(x as f32 / 320.0, y as f32 / 200.0);
                let uv1 = Vec2::new((x + w) as f32 / 320.0, (y + h) as f32 / 200.0);
                let face = commands
                    .spawn((
                        PanelCapFace,
                        Mesh3d(meshes.add(uv_quad(size, uv0, uv1))),
                        Transform::from_xyz(0.0, 0.0, CAP_HEIGHT),
                        NoFrustumCulling,
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
                let moving = commands.spawn((Moving, Transform::default(), Visibility::default())).add_children(&[face, skirt]).id();
                let c = commands
                    .spawn((Control::new(i, size / 2.0, CAP_HEIGHT, TRAVEL), tf, Visibility::default()))
                    .add_child(moving)
                    .id();
                commands.entity(root).add_child(c);
            }
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
                let moving = commands.spawn((Moving, Transform::default(), Visibility::default())).add_children(&[face, skirt]).id();
                let tf = Transform::from_translation(centre).with_rotation(rot);
                let c = commands
                    .spawn((Control::new(i, size / 2.0, SIDE_HEIGHT, TRAVEL), tf, Visibility::default()))
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
                let moving = commands.spawn((Moving, Transform::default(), Visibility::default())).add_children(&[arm, wheel]).id();
                let c = commands
                    .spawn((Control::new(i, Vec2::splat(0.02), 0.07, 0.01), Transform::from_translation(centre).with_rotation(rot), Visibility::default()))
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
                    .spawn((Control::new(i, Vec2::new(0.065, 0.02), 0.04, 0.02), Transform::from_translation(centre).with_rotation(rot), Visibility::default()))
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

/// Marks the cap faces that show the panel picture (they get the panel material).
#[derive(Component)]
pub(super) struct PanelCapFace;

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
    fn new(def: usize, half: Vec2, height: f32, travel: f32) -> Self {
        Self { def, half, height, travel, depth: 0.0, latched: false, armed: [false; 2], click: 0.0 }
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Fire<'w, 's> {
    actions: MessageWriter<'w, ActionEvent>,
    hud: Res<'w, HudState>,
    mfd: Res<'w, MfdState>,
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
            Effect::CamCycle => {
                let next = match self.mfd.cam_dir {
                    CamDir::Ahead => Action::CamRight,
                    CamDir::Right => Action::CamRear,
                    CamDir::Rear => Action::CamLeft,
                    CamDir::Left => Action::CamAhead,
                };
                self.actions.write(ActionEvent(next));
            }
        }
    }
}

/// Seconds a clicked control stays pushed in.
const CLICK_SECONDS: f32 = 0.18;

/// Fingertip pokes, mouse clicks and script presses.
#[allow(clippy::too_many_arguments)]
pub(super) fn interact(
    time: Res<Time<Real>>,
    settings: Res<crate::settings::Settings>,
    view: Res<ViewState>,
    mut hands: ResMut<Hands>,
    mut controls: Query<(&mut Control, &Transform)>,
    mut fire: Fire,
    mut presses: MessageReader<PressControl>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), With<EyeCamera>>,
    root: Query<&GlobalTransform, With<CockpitRoot>>,
) {
    let dt = time.delta_secs();
    for PressControl(a) in presses.read() {
        if let Some((mut c, _)) = controls.iter_mut().find(|(c, _)| DEFS[c.def].effect.action() == *a) {
            c.click = CLICK_SECONDS;
            fire.fire(DEFS[c.def].effect);
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
        for (c, tf) in &mut controls {
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
            fire.fire(DEFS[c.def].effect);
        }
    }

    // VR fingertips.
    for (mut c, tf) in &mut controls {
        c.click = (c.click - dt).max(0.0);
        let mut depth: f32 = 0.0;
        for (h, hand) in hands.hands.iter_mut().enumerate() {
            // The ejection handle has to be grabbed and pulled (no accidental pokes).
            if !hand.active || !cockpit_view || DEFS[c.def].kind == Kind::EjectHandle {
                c.armed[h] = false;
                continue;
            }
            let local = tf.rotation.inverse() * (hand.finger - tf.translation);
            let inside = local.x.abs() <= c.half.x + 0.006 && local.y.abs() <= c.half.y + 0.006 && local.z > -0.04;
            if !inside {
                c.armed[h] = false;
                continue;
            }
            let face = c.height + FINGER;
            if local.z > face {
                c.armed[h] = true;
            }
            if c.armed[h] {
                let push = (face - local.z).clamp(0.0, c.travel);
                if !c.latched && push >= c.travel * 0.7 {
                    hand.haptic = hand.haptic.max(0.6);
                }
                depth = depth.max(push);
            }
        }
        if !c.latched && depth >= c.travel * 0.7 {
            c.latched = true;
            fire.fire(DEFS[c.def].effect);
        } else if c.latched && depth < c.travel * 0.3 {
            c.latched = false;
        }
        c.depth = if c.click > 0.0 { c.travel } else { depth };
    }
}

/// Moves the caps in, and the gear lever to the gear position.
pub(super) fn animate(
    controls: Query<(&Control, &Children)>,
    mut moving: Query<&mut Transform, With<Moving>>,
    aircraft: Query<&Aircraft, With<Player>>,
    panel_mat: Query<&super::displays::LitMaterial, With<super::displays::PanelTexture>>,
    faces: Query<Entity, (With<PanelCapFace>, Without<MeshMaterial3d<StandardMaterial>>)>,
    mut commands: Commands,
) {
    if let Ok(m) = panel_mat.single() {
        for e in &faces {
            commands.entity(e).insert((MeshMaterial3d(m.0.clone()), NotShadowCaster));
        }
    }
    let gear = aircraft.single().is_ok_and(|a| a.gear_down);
    for (c, children) in &controls {
        for child in children.iter() {
            if let Ok(mut tf) = moving.get_mut(child) {
                let mut t = Transform::from_xyz(0.0, 0.0, -c.depth);
                if DEFS[c.def].kind == Kind::GearLever {
                    t.rotation = Quat::from_rotation_x(if gear { 0.5 } else { -0.5 });
                }
                if *tf != t {
                    *tf = t;
                }
            }
        }
    }
}
