//! In-flight settings menu (Esc, Pause, gamepad Start or the MENU button of the left side panel):
//! a screen in front of the pilot, in the cockpit, so it works on the desktop and in VR. The
//! simulation pauses while it is open.
//!
//! Graphics switches (shaders, shadows, lighting), cockpit and HUD style and the VR cockpit size
//! apply at once and are saved to the preferences when the menu closes. Choose an entry with the
//! mouse, a VR fingertip and the trigger, the arrow keys / gamepad d-pad / VR thumbstick and
//! Enter / A / X.

use bevy::audio::{AudioSink, AudioSinkPlayback};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::camera::{EyeCamera, View, ViewState};
use crate::canvas::{Canvas, CanvasTexture};
use crate::cockpit3d::{BUTTON_PRIMARY, BUTTON_STICK_DOWN, BUTTON_STICK_LEFT, BUTTON_STICK_RIGHT, BUTTON_STICK_UP, CockpitRoot, Hands};
use crate::font::{Align, stroke_text_at};
use crate::input::{Action, ActionEvent, InputBlock};
use crate::settings::{AppState, Settings, VR_COCKPIT_SCALE_RANGE};

pub struct FlightMenuPlugin;

impl Plugin for FlightMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlightMenu>()
            .add_systems(OnExit(AppState::Flight), close_on_exit)
            .add_systems(
                Update,
                (toggle, spawn_screen, navigate, draw).chain().run_if(in_state(AppState::Flight)),
            );
    }
}

/// Menu state; `open` pauses the flight.
#[derive(Resource, Default)]
pub struct FlightMenu {
    pub open: bool,
    /// Highlighted entry.
    cursor: usize,
    /// Settings when the menu opened (saved to the preferences on close if changed).
    opened_with: Option<Settings>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Shaders,
    Shadows,
    Lighting,
    Cockpit,
    Hud,
    Avionics,
    CockpitSize,
    Resume,
    EndMission,
}

const ITEMS: [Item; 9] = [Item::Shaders, Item::Shadows, Item::Lighting, Item::Cockpit, Item::Hud, Item::Avionics, Item::CockpitSize, Item::Resume, Item::EndMission];

impl Item {
    fn label(self) -> &'static str {
        match self {
            Item::Shaders => "SHADERS",
            Item::Shadows => "SHADOWS",
            Item::Lighting => "POINT LIGHTS",
            Item::Cockpit => "COCKPIT",
            Item::Hud => "HUD",
            Item::Avionics => "AVIONICS",
            Item::CockpitSize => "COCKPIT SIZE",
            Item::Resume => "RESUME",
            Item::EndMission => "END MISSION",
        }
    }

    fn value(self, s: &Settings) -> String {
        let style = |classic: bool| if classic { "CLASSIC" } else { "MODERN" }.to_string();
        let on = |b: bool| if b { "ON" } else { "OFF" }.to_string();
        match self {
            Item::Shaders => style(!s.modern_shading),
            Item::Shadows => on(s.shadows),
            Item::Lighting => on(s.local_lights),
            Item::Cockpit => style(s.classic_cockpit),
            Item::Hud => style(s.classic_hud),
            Item::Avionics => style(s.classic_avionics),
            Item::CockpitSize => format!("< {:.0}% >", s.vr_cockpit_scale * 100.0),
            Item::Resume | Item::EndMission => String::new(),
        }
    }

    /// The cockpit size only matters in the headset, the cockpit style only for aircraft with
    /// two cockpits.
    fn enabled(self, s: &Settings) -> bool {
        match self {
            Item::CockpitSize => s.vr,
            Item::Cockpit => s.aircraft.spec().cockpits.len() > 1,
            _ => true,
        }
    }
}

/// Screen size (metres) and its distance in front of and height relative to the eye.
const SIZE: Vec2 = Vec2::new(0.42, 0.28);
const POSITION: Vec3 = Vec3::new(0.0, -0.02, -0.52);
/// Canvas pixels.
const W: usize = 768;
const H: usize = 512;
/// Rows of the entries.
const TOP: f32 = 104.0;
const ROW: f32 = 43.0;
/// Palette colours: background, bright and dim green (the panel's lamp greens), dark row.
const BG: u8 = 5;
const BRIGHT: u8 = 47;
const DIM: u8 = 45;
const ROW_BG: u8 = 44;
/// A VR fingertip touches the screen within this distance (metres).
const TOUCH: f32 = 0.025;

#[derive(Component)]
struct MenuScreen;

fn close_on_exit(mut menu: ResMut<FlightMenu>, mut time: ResMut<Time<Virtual>>, mut block: ResMut<InputBlock>) {
    menu.open = false;
    menu.opened_with = None;
    time.unpause();
    block.0 = false;
}

/// Opens and closes the menu (Action::Menu), pausing the flight and its sounds.
#[allow(clippy::too_many_arguments)]
fn toggle(
    mut actions: MessageReader<ActionEvent>,
    mut menu: ResMut<FlightMenu>,
    settings: Res<Settings>,
    mut time: ResMut<Time<Virtual>>,
    mut block: ResMut<InputBlock>,
    mut view: ResMut<ViewState>,
    sinks: Query<&AudioSink>,
) {
    let pressed = actions.read().filter(|a| a.0 == Action::Menu).count() % 2 == 1;
    if pressed {
        let open = !menu.open;
        set_open(&mut menu, open, &settings, &mut time, &mut block, &sinks);
        if menu.open && view.view != View::Cockpit {
            view.view = View::Cockpit;
        }
    }
}

fn set_open(menu: &mut FlightMenu, open: bool, settings: &Settings, time: &mut Time<Virtual>, block: &mut InputBlock, sinks: &Query<&AudioSink>) {
    if menu.open == open {
        return;
    }
    menu.open = open;
    block.0 = open;
    if open {
        time.pause();
        menu.opened_with = Some(settings.clone());
        for s in sinks {
            s.pause();
        }
    } else {
        time.unpause();
        for s in sinks {
            s.play();
        }
        if menu.opened_with.take().is_some_and(|before| switches(&before) != switches(settings)) {
            crate::prefs::Prefs::save_switches(settings);
        }
    }
}

fn switches(s: &Settings) -> (bool, bool, bool, bool, bool, bool, u32) {
    (s.modern_shading, s.shadows, s.local_lights, s.classic_cockpit, s.classic_hud, s.classic_avionics, (s.vr_cockpit_scale * 100.0).round() as u32)
}

/// The menu screen: in VR in front of the pilot in the cockpit (spawned again when the cockpit
/// is rebuilt, e.g. after switching its style), on the desktop a 2D overlay.
fn spawn_screen(
    mut commands: Commands,
    roots: Query<Entity, With<CockpitRoot>>,
    screens: Query<(), With<MenuScreen>>,
    settings: Res<Settings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(root) = roots.single() else { return };
    if !screens.is_empty() {
        return;
    }
    let tex = CanvasTexture::new(Canvas::new(W, H), &mut images, false);
    if let Some(mut img) = images.get_mut(&tex.image) {
        img.sampler = bevy::image::ImageSampler::linear();
    }
    if !settings.vr {
        // On the desktop a flat overlay in the middle of the window: in front of the eye it
        // would be cut by the camera's near plane.
        let image = ImageNode::new(tex.image.clone());
        let overlay = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                GlobalZIndex(10),
                DespawnOnExit(AppState::Flight),
            ))
            .id();
        let screen = commands
            .spawn((
                MenuScreen,
                tex,
                image,
                Node { height: vh(70), aspect_ratio: Some(W as f32 / H as f32), ..default() },
                RelativeCursorPosition::default(),
                Visibility::Hidden,
            ))
            .id();
        commands.entity(overlay).add_child(screen);
        return;
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
            MenuScreen,
            tex,
            Mesh3d(meshes.add(Rectangle::new(SIZE.x, SIZE.y))),
            MeshMaterial3d(material),
            Transform::from_translation(POSITION),
            Visibility::Hidden,
            NotShadowCaster,
        ))
        .id();
    commands.entity(root).add_child(screen);
}

/// Canvas pixel of a point in the screen's local frame (None off the screen).
fn to_canvas(local: Vec3) -> Option<Vec2> {
    let u = local.x / SIZE.x + 0.5;
    let v = 0.5 - local.y / SIZE.y;
    ((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v)).then(|| Vec2::new(u * W as f32, v * H as f32))
}

/// Entry under a canvas pixel, and whether the pixel is on the right half (size: larger).
fn item_at(p: Vec2) -> Option<(usize, bool)> {
    let i = ((p.y - TOP) / ROW).floor();
    (p.y >= TOP && (i as usize) < ITEMS.len()).then(|| (i as usize, p.x > W as f32 / 2.0))
}

/// Keyboard, gamepad, VR thumbsticks and buttons, VR fingertips and the mouse.
#[allow(clippy::too_many_arguments)]
fn navigate(
    mut menu: ResMut<FlightMenu>,
    mut settings: ResMut<Settings>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    mut hands: ResMut<Hands>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), With<EyeCamera>>,
    screen: Query<(&GlobalTransform, &Transform), With<MenuScreen>>,
    overlay: Query<&RelativeCursorPosition, With<MenuScreen>>,
    mut time: ResMut<Time<Virtual>>,
    mut block: ResMut<InputBlock>,
    mut actions: MessageWriter<ActionEvent>,
    sinks: Query<&AudioSink>,
) {
    if !menu.open {
        return;
    }
    let n = ITEMS.len();
    let mut step = 0i32;
    let mut adjust = 0i32;
    let mut select = false;
    let pad = |b: GamepadButton| gamepads.iter().any(|g| g.just_pressed(b));
    if keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp) {
        step -= 1;
    }
    if keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown) {
        step += 1;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft) {
        adjust -= 1;
    }
    if keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight) {
        adjust += 1;
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) || pad(GamepadButton::South) {
        select = true;
    }
    let mut close = pad(GamepadButton::East);
    for hand in hands.hands.iter().filter(|h| h.active) {
        if hand.just_pressed(BUTTON_STICK_UP) {
            step -= 1;
        }
        if hand.just_pressed(BUTTON_STICK_DOWN) {
            step += 1;
        }
        if hand.just_pressed(BUTTON_STICK_LEFT) {
            adjust -= 1;
        }
        if hand.just_pressed(BUTTON_STICK_RIGHT) {
            adjust += 1;
        }
        if hand.just_pressed(BUTTON_PRIMARY) {
            select = true;
        }
    }
    if step != 0 {
        menu.cursor = (menu.cursor as i32 + step).rem_euclid(n as i32) as usize;
    }

    // Pointing: the mouse hovers and clicks, a fingertip touches and the trigger presses.
    let mut clicked: Option<(usize, bool)> = None;
    // Desktop overlay: the cursor's place on it (-0.5 .. 0.5 from its centre).
    if let Ok(rel) = overlay.single()
        && rel.cursor_over()
        && let Some(n) = rel.normalized
        && let Some(hit) = item_at(Vec2::new((n.x + 0.5) * W as f32, (n.y + 0.5) * H as f32))
    {
        menu.cursor = hit.0;
        if mouse.just_pressed(MouseButton::Left) {
            clicked = Some(hit);
        }
    }
    if let Ok((screen_gtf, screen_tf)) = screen.single() {
        let inv = screen_gtf.affine().inverse();
        if !settings.vr
            && let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), cams.single())
            && let Some(cursor) = window.cursor_position()
            && let Ok(ray) = camera.viewport_to_world(cam_tf, cursor)
        {
            let o = inv.transform_point3(ray.origin);
            let d = inv.transform_vector3(*ray.direction);
            if d.z.abs() > 1e-6 {
                let t = -o.z / d.z;
                if let Some(hit) = (t > 0.0).then(|| to_canvas(o + d * t)).flatten().and_then(item_at) {
                    menu.cursor = hit.0;
                    if mouse.just_pressed(MouseButton::Left) {
                        clicked = Some(hit);
                    }
                }
            }
        }
        // Fingertips are in the cockpit frame, the screen's parent.
        let local_from_cockpit = screen_tf.compute_affine().inverse();
        for hand in hands.hands.iter_mut().filter(|h| h.active) {
            let l = local_from_cockpit.transform_point3(hand.finger);
            if l.z.abs() > TOUCH {
                continue;
            }
            if let Some(hit) = to_canvas(l).and_then(item_at) {
                if menu.cursor != hit.0 {
                    hand.haptic = hand.haptic.max(0.1);
                }
                menu.cursor = hit.0;
                if hand.trigger_pulled() {
                    hand.haptic = hand.haptic.max(0.6);
                    clicked = Some(hit);
                }
            }
        }
    }

    let (index, right) = match clicked {
        Some(hit) => (hit.0, Some(hit.1)),
        None => (menu.cursor, None),
    };
    let item = ITEMS[index];
    if !item.enabled(&settings) || !(select || clicked.is_some() || adjust != 0) {
        if close {
            set_open(&mut menu, false, &settings, &mut time, &mut block, &sinks);
        }
        return;
    }
    // Left / right only change values; they don't press Resume or End mission.
    if adjust != 0 && clicked.is_none() && !select && matches!(item, Item::Resume | Item::EndMission) {
        return;
    }
    let s = &mut *settings;
    match item {
        Item::Shaders => s.modern_shading = !s.modern_shading,
        Item::Shadows => s.shadows = !s.shadows,
        Item::Lighting => s.local_lights = !s.local_lights,
        Item::Cockpit => s.classic_cockpit = !s.classic_cockpit,
        Item::Hud => s.classic_hud = !s.classic_hud,
        Item::Avionics => s.classic_avionics = !s.classic_avionics,
        Item::CockpitSize => {
            let dir = if adjust != 0 { adjust.signum() } else if right == Some(false) { -1 } else { 1 };
            s.vr_cockpit_scale = (s.vr_cockpit_scale + 0.05 * dir as f32).clamp(*VR_COCKPIT_SCALE_RANGE.start(), *VR_COCKPIT_SCALE_RANGE.end());
        }
        Item::Resume => close = true,
        Item::EndMission => {
            close = true;
            actions.write(ActionEvent(Action::Quit));
        }
    }
    if close {
        set_open(&mut menu, false, &settings, &mut time, &mut block, &sinks);
    }
}

fn draw(menu: Res<FlightMenu>, settings: Res<Settings>, mut q: Query<(&mut CanvasTexture, &mut Visibility), With<MenuScreen>>) {
    let Ok((mut tex, mut vis)) = q.single_mut() else { return };
    vis.set_if_neq(if menu.open { Visibility::Inherited } else { Visibility::Hidden });
    if !menu.open {
        return;
    }
    let c = &mut tex.canvas;
    c.clear(BG);
    c.frame(Vec2::new(6.0, 6.0), Vec2::new(W as f32 - 7.0, H as f32 - 7.0), 3.0, DIM);
    stroke_text_at(c, W as f32 / 2.0, 52.0, "SETTINGS  -  PAUSED", 26.0, 3.2, BRIGHT, Align::Centre);
    for (i, item) in ITEMS.iter().enumerate() {
        let y = TOP + ROW * i as f32;
        let mid = y + ROW / 2.0;
        let enabled = item.enabled(&settings);
        let col = if enabled { BRIGHT } else { DIM };
        if i == menu.cursor {
            c.fill_rect(24, y as i32 + 3, W as i32 - 48, ROW as i32 - 6, ROW_BG);
        }
        stroke_text_at(c, 48.0, mid, item.label(), 22.0, 2.8, col, Align::Left);
        let v = item.value(&settings);
        if !v.is_empty() {
            stroke_text_at(c, W as f32 - 48.0, mid, &v, 22.0, 2.8, col, Align::Right);
        }
    }
    tex.dirty = true;
}
