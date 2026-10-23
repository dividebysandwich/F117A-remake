//! In-flight settings menu (Esc, Pause, gamepad Start or the MENU button of the left side panel):
//! a screen in front of the pilot, in the cockpit, so it works on the desktop and in VR. The
//! simulation pauses while it is open.
//!
//! Graphics switches (shaders, shadows, lighting), cockpit and HUD style, the VR cockpit size and
//! the cockpit tooltips apply at once and are saved to the preferences when the menu closes. Choose an entry with the
//! mouse, a VR fingertip and the trigger, the arrow keys / gamepad d-pad / VR thumbstick and
//! Enter / A / X. In VR a fingertip on the screen highlights an entry and the trigger presses it
//! (a trigger pulled away from the screen and the cockpit controls presses the highlighted entry).
//!
//! KEY GUIDE shows the key bindings and the mouse or VR controllers (`key_guide.rs`) on the
//! screen, scrolled with the arrow keys, the mouse wheel, the gamepad d-pad or a VR thumbstick;
//! Esc, Enter, a click or the trigger go back to the entries.
//!
//! END MISSION keeps the flight paused and shows EXITING... until the debrief takes over.
//!
//! In a dynamic campaign the world goes on while the menu is open (nothing pauses), and END
//! SORTIE ends the sortie: the campaign's debrief and mission board come up in the flight.
//!
//! In VR the end-of-mission overlay (a 2D overlay on the desktop) is a screen floating in front of
//! the head, also in the outside views.

use bevy::audio::{AudioSink, AudioSinkPlayback};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::camera::{EyeCamera, View, ViewState};
use crate::canvas::{Canvas, CanvasTexture};
use crate::cockpit3d::{
    BUTTON_PRIMARY, BUTTON_STICK_DOWN, BUTTON_STICK_LEFT, BUTTON_STICK_RIGHT, BUTTON_STICK_UP,
    CockpitRoot, Hands,
};
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
                (toggle, spawn_screen, scroll_guide, navigate, draw, vr_end_screen)
                    .chain()
                    .run_if(in_state(AppState::Flight)),
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
    /// END MISSION was chosen: the menu shows EXITING... and the flight ends without the end
    /// overlay.
    pub exiting: bool,
    /// The left mouse button was up since the menu opened: only then does a click choose an
    /// entry (the click on the cockpit's MENU button that opened it must not, and the desktop
    /// overlay's cursor position is stale until it is drawn again).
    mouse_armed: bool,
    /// The key guide is shown instead of the entries, scrolled down this far (canvas pixels).
    guide: bool,
    guide_scroll: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Shaders,
    Shadows,
    Lighting,
    TimeOfDay,
    Cockpit,
    Hud,
    Avionics,
    CockpitSize,
    Tooltips,
    KeyGuide,
    Resume,
    EndMission,
}

const ITEMS: [Item; 12] = [
    Item::Shaders,
    Item::Shadows,
    Item::Lighting,
    Item::TimeOfDay,
    Item::Cockpit,
    Item::Hud,
    Item::Avionics,
    Item::CockpitSize,
    Item::Tooltips,
    Item::KeyGuide,
    Item::Resume,
    Item::EndMission,
];

impl Item {
    fn label(self) -> &'static str {
        match self {
            Item::Shaders => "SHADERS",
            Item::Shadows => "SHADOWS",
            Item::Lighting => "POINT LIGHTS",
            Item::TimeOfDay => "TIME OF DAY",
            Item::Cockpit => "COCKPIT",
            Item::Hud => "HUD",
            Item::Avionics => "AVIONICS",
            Item::CockpitSize => "COCKPIT SIZE",
            Item::Tooltips => "TOOLTIPS",
            Item::KeyGuide => "KEY GUIDE",
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
            Item::TimeOfDay => style(!s.continuous_time),
            Item::Cockpit => style(s.classic_cockpit),
            Item::Hud => style(s.classic_hud),
            Item::Avionics => style(s.classic_avionics),
            Item::CockpitSize => format!("< {:.0}% >", s.vr_cockpit_scale * 100.0),
            Item::Tooltips => on(s.tooltips),
            Item::KeyGuide => ">".to_string(),
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
const ROW: f32 = 33.0;
/// The key guide's area.
const GUIDE: [f32; 4] = [24.0, 70.0, W as f32 - 24.0, H as f32 - 40.0];
/// Palette colours: background, bright and dim green (the panel's lamp greens), dark row.
const BG: u8 = 5;
const BRIGHT: u8 = 47;
const DIM: u8 = 45;
const ROW_BG: u8 = 44;
/// A VR fingertip touches the screen from this far in front of it to this far behind it
/// (metres; a finger easily passes through the plane).
const TOUCH_FRONT: f32 = 0.05;
const TOUCH_BEHIND: f32 = 0.10;

#[derive(Component)]
struct MenuScreen;

#[derive(Component)]
struct EndScreen;

/// The VR end screen: size (metres) and distance in front of the head.
const END_SIZE: Vec2 = Vec2::new(0.6, 0.4);
const END_DISTANCE: f32 = 0.8;

fn close_on_exit(
    mut menu: ResMut<FlightMenu>,
    mut time: ResMut<Time<Virtual>>,
    mut block: ResMut<InputBlock>,
) {
    menu.open = false;
    menu.opened_with = None;
    menu.exiting = false;
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
    campaign: Option<Res<crate::campaign::ActiveCampaign>>,
) {
    let pressed = actions.read().filter(|a| a.0 == Action::Menu).count() % 2 == 1;
    // Esc in the key guide goes back to the entries.
    if pressed && menu.open && menu.guide {
        menu.guide = false;
    } else if pressed && !menu.exiting {
        let open = !menu.open;
        let pause = campaign.is_none();
        set_open(&mut menu, open, pause, &settings, &mut time, &mut block, &sinks);
        if menu.open && view.view != View::Cockpit {
            view.view = View::Cockpit;
        }
    }
}

fn set_open(
    menu: &mut FlightMenu,
    open: bool,
    pause: bool,
    settings: &Settings,
    time: &mut Time<Virtual>,
    block: &mut InputBlock,
    sinks: &Query<&AudioSink>,
) {
    if menu.open == open {
        return;
    }
    menu.open = open;
    menu.guide = false;
    block.0 = open;
    if open {
        menu.mouse_armed = false;
        menu.opened_with = Some(settings.clone());
        if pause {
            time.pause();
            for s in sinks {
                s.pause();
            }
        }
    } else {
        time.unpause();
        if pause {
            for s in sinks {
                s.play();
            }
        }
        if menu.opened_with.take().is_some_and(|before| switches(&before) != switches(settings)) {
            crate::prefs::Prefs::save_switches(settings);
        }
    }
}

fn switches(s: &Settings) -> (bool, bool, bool, bool, bool, bool, bool, u32, bool) {
    (
        s.modern_shading,
        s.shadows,
        s.local_lights,
        s.continuous_time,
        s.classic_cockpit,
        s.classic_hud,
        s.classic_avionics,
        (s.vr_cockpit_scale * 100.0).round() as u32,
        s.tooltips,
    )
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
    ((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v))
        .then(|| Vec2::new(u * W as f32, v * H as f32))
}

/// Entry under a canvas pixel, and whether the pixel is on the right half (size: larger).
fn item_at(p: Vec2) -> Option<(usize, bool)> {
    let i = ((p.y - TOP) / ROW).floor();
    (p.y >= TOP && (i as usize) < ITEMS.len()).then(|| (i as usize, p.x > W as f32 / 2.0))
}

/// The key guide scrolls: arrow keys (held), PgUp / PgDn, the mouse wheel, the gamepad d-pad,
/// a VR thumbstick.
fn scroll_guide(
    mut menu: ResMut<FlightMenu>,
    keys: Res<ButtonInput<KeyCode>>,
    wheel: Res<AccumulatedMouseScroll>,
    gamepads: Query<&Gamepad>,
    hands: Res<Hands>,
    time: Res<Time<Real>>,
) {
    if !menu.open || !menu.guide {
        return;
    }
    let held =
        |k: KeyCode, b: GamepadButton| keys.pressed(k) || gamepads.iter().any(|g| g.pressed(b));
    let mut speed = held(KeyCode::ArrowDown, GamepadButton::DPadDown) as i8 as f32
        - held(KeyCode::ArrowUp, GamepadButton::DPadUp) as i8 as f32;
    for hand in hands.hands.iter().filter(|h| h.active && h.thumb.y.abs() > 0.15) {
        speed -= hand.thumb.y;
    }
    let page = GUIDE[3] - GUIDE[1] - 40.0;
    let mut d = speed * crate::key_guide::SCROLL_SPEED * time.delta_secs()
        - crate::key_guide::wheel_notches(&wheel) * crate::key_guide::WHEEL_STEP;
    if keys.just_pressed(KeyCode::PageDown) {
        d += page;
    }
    if keys.just_pressed(KeyCode::PageUp) {
        d -= page;
    }
    if d != 0.0 {
        // Kept within the list when it is drawn.
        menu.guide_scroll = (menu.guide_scroll + d).max(0.0);
    }
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
    campaign: Option<Res<crate::campaign::ActiveCampaign>>,
) {
    if !menu.open || menu.exiting {
        return;
    }
    let pause = campaign.is_none();
    if !mouse.pressed(MouseButton::Left) {
        menu.mouse_armed = true;
    }
    let click = menu.mouse_armed && mouse.just_pressed(MouseButton::Left);
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
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space])
        || pad(GamepadButton::South)
    {
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
    // The key guide: anything that presses an entry goes back to the entries.
    if menu.guide {
        let trigger = hands.hands.iter().any(|h| h.active && h.trigger_pulled());
        if select || close || click || trigger {
            menu.guide = false;
        }
        return;
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
        if click {
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
                if let Some(hit) =
                    (t > 0.0).then(|| to_canvas(o + d * t)).flatten().and_then(item_at)
                {
                    menu.cursor = hit.0;
                    if click {
                        clicked = Some(hit);
                    }
                }
            }
        }
        // Fingertips are in the cockpit frame, the screen's parent.
        let local_from_cockpit = screen_tf.compute_affine().inverse();
        for hand in hands.hands.iter_mut().filter(|h| h.active) {
            let l = local_from_cockpit.transform_point3(hand.finger);
            let hit = (l.z < TOUCH_FRONT && l.z > -TOUCH_BEHIND)
                .then(|| to_canvas(l))
                .flatten()
                .and_then(item_at);
            if let Some(hit) = hit {
                if menu.cursor != hit.0 {
                    hand.haptic = hand.haptic.max(0.1);
                }
                menu.cursor = hit.0;
            }
            // Away from the screen, not while the finger is on a cockpit control (the MENU
            // button that opens and closes the menu).
            if hand.trigger_pulled() && (hit.is_some() || !hand.touching) {
                hand.haptic = hand.haptic.max(0.6);
                match hit {
                    Some(hit) => clicked = Some(hit),
                    None => select = true,
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
            set_open(&mut menu, false, pause, &settings, &mut time, &mut block, &sinks);
        }
        return;
    }
    // Left / right only change values; they don't press Resume, the key guide or End mission.
    if adjust != 0
        && clicked.is_none()
        && !select
        && matches!(item, Item::Resume | Item::KeyGuide | Item::EndMission)
    {
        return;
    }
    let s = &mut *settings;
    match item {
        Item::Shaders => s.modern_shading = !s.modern_shading,
        Item::Shadows => s.shadows = !s.shadows,
        Item::Lighting => s.local_lights = !s.local_lights,
        Item::TimeOfDay => s.continuous_time = !s.continuous_time,
        Item::Cockpit => s.classic_cockpit = !s.classic_cockpit,
        Item::Hud => s.classic_hud = !s.classic_hud,
        Item::Avionics => s.classic_avionics = !s.classic_avionics,
        Item::Tooltips => s.tooltips = !s.tooltips,
        Item::CockpitSize => {
            let dir = if adjust != 0 {
                adjust.signum()
            } else if right == Some(false) {
                -1
            } else {
                1
            };
            s.vr_cockpit_scale = (s.vr_cockpit_scale + 0.05 * dir as f32)
                .clamp(*VR_COCKPIT_SCALE_RANGE.start(), *VR_COCKPIT_SCALE_RANGE.end());
        }
        Item::KeyGuide => {
            menu.guide = true;
            menu.guide_scroll = 0.0;
        }
        Item::Resume => close = true,
        // A campaign sortie ends; the flight goes on with the campaign's board.
        Item::EndMission if !pause => {
            actions.write(ActionEvent(Action::Quit));
            close = true;
        }
        Item::EndMission => {
            // Stays open (paused and silent) showing EXITING... until the flight ends.
            menu.exiting = true;
            if menu.opened_with.take().is_some_and(|before| switches(&before) != switches(s)) {
                crate::prefs::Prefs::save_switches(s);
            }
            actions.write(ActionEvent(Action::Quit));
        }
    }
    if close {
        set_open(&mut menu, false, pause, &settings, &mut time, &mut block, &sinks);
    }
}

fn draw(
    mut menu: ResMut<FlightMenu>,
    settings: Res<Settings>,
    mut q: Query<(&mut CanvasTexture, &mut Visibility), With<MenuScreen>>,
    campaign: Option<Res<crate::campaign::ActiveCampaign>>,
    config: Res<crate::hotas::InputConfig>,
) {
    let Ok((mut tex, mut vis)) = q.single_mut() else { return };
    vis.set_if_neq(if menu.open { Visibility::Inherited } else { Visibility::Hidden });
    if !menu.open {
        return;
    }
    let c = &mut tex.canvas;
    c.clear(BG);
    c.frame(Vec2::new(6.0, 6.0), Vec2::new(W as f32 - 7.0, H as f32 - 7.0), 3.0, DIM);
    if menu.exiting {
        stroke_text_at(
            c,
            W as f32 / 2.0,
            H as f32 / 2.0,
            "EXITING...",
            40.0,
            4.0,
            BRIGHT,
            Align::Centre,
        );
        tex.dirty = true;
        return;
    }
    if menu.guide {
        stroke_text_at(c, W as f32 / 2.0, 42.0, "KEY GUIDE", 22.0, 2.8, BRIGHT, Align::Centre);
        let lines = crate::key_guide::lines(&config, settings.vr);
        let style = crate::key_guide::Style {
            size: 0.72,
            heading: BRIGHT,
            bar: ROW_BG,
            keys: BRIGHT,
            text: 46,
            dim: DIM,
        };
        let mut scroll = menu.guide_scroll;
        crate::key_guide::draw(c, &lines, GUIDE, &mut scroll, &style);
        menu.guide_scroll = scroll;
        let hint = if settings.vr {
            "STICK: SCROLL    TRIGGER: BACK"
        } else {
            "WHEEL, ARROWS: SCROLL    ESC, CLICK: BACK"
        };
        stroke_text_at(c, W as f32 / 2.0, H as f32 - 22.0, hint, 12.0, 1.8, DIM, Align::Centre);
        tex.dirty = true;
        return;
    }
    stroke_text_at(
        c,
        W as f32 / 2.0,
        52.0,
        if campaign.is_some() { "SETTINGS  -  CAMPAIGN RUNS ON" } else { "SETTINGS  -  PAUSED" },
        26.0,
        3.2,
        BRIGHT,
        Align::Centre,
    );
    for (i, item) in ITEMS.iter().enumerate() {
        let y = TOP + ROW * i as f32;
        let mid = y + ROW / 2.0;
        let enabled = item.enabled(&settings);
        let col = if enabled { BRIGHT } else { DIM };
        if i == menu.cursor {
            c.fill_rect(24, y as i32 + 3, W as i32 - 48, ROW as i32 - 6, ROW_BG);
        }
        let label = match item {
            Item::EndMission if campaign.is_some() => "END SORTIE",
            _ => item.label(),
        };
        stroke_text_at(c, 48.0, mid, label, 22.0, 2.8, col, Align::Left);
        let v = item.value(&settings);
        if !v.is_empty() {
            stroke_text_at(c, W as f32 - 48.0, mid, &v, 22.0, 2.8, col, Align::Right);
        }
    }
    tex.dirty = true;
}

/// VR: the end-of-mission overlay on a screen in front of the head (a child of the XR tracking
/// root, so it stays in view in every camera view). Not after END MISSION, which goes straight
/// to the debrief.
#[allow(clippy::too_many_arguments)]
fn vr_end_screen(
    mut commands: Commands,
    settings: Res<Settings>,
    menu: Res<FlightMenu>,
    combat: Option<Res<crate::combat::Combat>>,
    roots: Query<Entity, With<EyeCamera>>,
    cams: Query<(&ChildOf, &Transform), With<Camera>>,
    screens: Query<(), With<EndScreen>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !settings.vr || menu.exiting || !screens.is_empty() {
        return;
    }
    let Some(end) = combat.as_ref().and_then(|c| c.end.as_ref()) else { return };
    let Ok(root) = roots.single() else { return };
    // The head pose in the tracking space: the headset cameras' (the root's children).
    let eyes: Vec<&Transform> =
        cams.iter().filter(|(p, _)| p.parent() == root).map(|(_, t)| t).collect();
    let head = eyes.iter().map(|t| t.translation).sum::<Vec3>() / eyes.len().max(1) as f32;
    let fwd = eyes.first().map_or(Vec3::NEG_Z, |t| t.forward().as_vec3());
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    let centre = head + flat * END_DISTANCE - Vec3::Y * 0.05;

    let mut canvas = Canvas::new(W, H);
    canvas.clear(BG);
    canvas.frame(Vec2::new(6.0, 6.0), Vec2::new(W as f32 - 7.0, H as f32 - 7.0), 3.0, DIM);
    // Each line as large as fits the width.
    let fit = |s: &str, h: f32| h.min((W as f32 - 64.0) / s.chars().count().max(1) as f32);
    let title = end.title.to_uppercase();
    stroke_text_at(
        &mut canvas,
        W as f32 / 2.0,
        64.0,
        &title,
        fit(&title, 32.0),
        3.6,
        BRIGHT,
        Align::Centre,
    );
    for (i, l) in end.lines.iter().enumerate() {
        let y = 140.0 + 42.0 * i as f32;
        stroke_text_at(&mut canvas, W as f32 / 2.0, y, l, fit(l, 20.0), 2.6, BRIGHT, Align::Centre);
    }
    let hint = "PULL THE TRIGGER";
    stroke_text_at(
        &mut canvas,
        W as f32 / 2.0,
        H as f32 - 50.0,
        hint,
        20.0,
        2.6,
        DIM,
        Align::Centre,
    );
    let tex = CanvasTexture::new(canvas, &mut images, false);
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
            EndScreen,
            tex,
            Mesh3d(meshes.add(Rectangle::new(END_SIZE.x, END_SIZE.y))),
            MeshMaterial3d(material),
            Transform::from_translation(centre).looking_to(flat, Vec3::Y),
            NotShadowCaster,
            DespawnOnExit(AppState::Flight),
        ))
        .id();
    commands.entity(root).add_child(screen);
}
