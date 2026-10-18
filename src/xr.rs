//! OpenXR (`--vr`, cargo feature `xr`), via bevy_mod_openxr.
//!
//! - Rendering: `plugins` swaps Bevy's render plugin for the OpenXR one (without a runtime it
//!   falls back to the normal window). The XR tracking root becomes the rig's eye camera
//!   (`camera::spawn_rig`), so the headset sits at the pilot's eye point and follows the
//!   aircraft; `RigOffset` recentres it (on entering the flight, the left menu button or Home).
//! - Input: one action set (grip/aim poses, squeeze, trigger, A/B/X/Y, stick click, menu,
//!   haptics) with bindings for the common controllers, read into `cockpit3d::Hands` in the
//!   cockpit frame. The cockpit module turns that into button pokes, stick and throttle.

use bevy::app::PluginGroupBuilder;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy_mod_openxr::action_binding::{OxrSendActionBindings, OxrSuggestActionBinding};
use bevy_mod_openxr::action_set_attaching::OxrAttachActionSet;
use bevy_mod_openxr::action_set_syncing::{OxrActionSetSyncSet, OxrSyncActionSet};
use bevy_mod_openxr::resources::OxrInstance;
use bevy_mod_openxr::session::OxrSession;
use bevy_mod_openxr::spaces::OxrSpaceLocationFlags;
use bevy_mod_openxr::{add_xr_plugins, openxr_session_running};
use bevy_mod_xr::camera::XrCamera;
use bevy_mod_xr::session::{XrPreDestroySession, XrSessionCreated, XrTrackingRoot, session_available};
use bevy_mod_xr::spaces::XrSpaceSyncSet;
use openxr::{Haptic, Posef};

use crate::camera::{EyeCamera, MAIN_BACKGROUND_LAYER, RigOffset, View, ViewState, XrRoot};
use crate::cockpit3d::{BUTTON_MENU, BUTTON_PRIMARY, BUTTON_SECONDARY, BUTTON_THUMB, Hands, HandsUpdate};
use crate::settings::AppState;

/// Whether an OpenXR runtime is installed and active and offers Vulkan through
/// `XR_KHR_vulkan_enable2`, the only way bevy_mod_openxr renders (cheap: loads the loader and
/// asks the runtime for its extensions, without creating an instance, which would start
/// SteamVR on a PC).
pub fn runtime_available() -> bool {
    let Some(entry) = entry() else {
        info!("OpenXR: no loader");
        return false;
    };
    match entry.enumerate_extensions() {
        Ok(exts) => {
            info!("OpenXR: runtime found, XR_KHR_vulkan_enable2 {}", exts.khr_vulkan_enable2);
            exts.khr_vulkan_enable2
        }
        Err(e) => {
            info!("OpenXR: no runtime ({e})");
            false
        }
    }
}

/// The OpenXR loader. On Windows bevy_mod_openxr links the Khronos loader in. Elsewhere it is
/// `libopenxr_loader.so`, loaded as bevy_mod_openxr loads it: from beside the executable (the
/// Linux packages ship it there, for systems without the openxr package such as SteamOS; the
/// executable's RUNPATH is `$ORIGIN`, see build.rs), else from the system.
fn entry() -> Option<openxr::Entry> {
    #[cfg(windows)]
    {
        Some(openxr::Entry::linked())
    }
    #[cfg(not(windows))]
    {
        // SAFETY: loads the OpenXR loader library.
        unsafe { openxr::Entry::load() }.ok()
    }
}

/// The default plugins with OpenXR rendering.
pub fn plugins(plugins: PluginGroupBuilder) -> PluginGroupBuilder {
    // Pipelined rendering adds a frame of latency to head and hand poses.
    add_xr_plugins(plugins.disable::<PipelinedRenderingPlugin>())
}

pub struct XrPlugin;

impl Plugin for XrPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (create_actions, find_root).run_if(session_available))
            .add_systems(Startup, no_runtime.run_if(not(session_available)))
            .add_systems(OxrSendActionBindings, suggest_bindings)
            .add_systems(XrSessionCreated, (attach_set, spawn_hand_spaces))
            .add_systems(XrPreDestroySession, despawn_hand_spaces)
            .add_systems(
                PreUpdate,
                (
                    sync_actions.before(OxrActionSetSyncSet),
                    read_hands
                        .in_set(HandsUpdate)
                        .after(OxrActionSetSyncSet)
                        .after(XrSpaceSyncSet)
                        .after(crate::input::InputSystemsSet),
                )
                    .run_if(openxr_session_running)
                    .run_if(resource_exists::<XrActions>),
            )
            .add_systems(Update, (setup_cameras, recenter).run_if(session_available))
            .add_systems(PostUpdate, haptics.run_if(openxr_session_running).run_if(resource_exists::<XrActions>))
            .add_systems(OnEnter(AppState::Flight), |mut r: ResMut<Recenter>| r.0 = true)
            .add_systems(OnExit(AppState::Flight), detach_root.before(crate::combat::FlightTeardown))
            .init_resource::<Recenter>();
    }
}

#[derive(Resource)]
struct XrActions {
    set: openxr::ActionSet,
    grip: openxr::Action<Posef>,
    aim: openxr::Action<Posef>,
    squeeze: openxr::Action<f32>,
    trigger: openxr::Action<f32>,
    primary: openxr::Action<bool>,
    secondary: openxr::Action<bool>,
    thumb: openxr::Action<bool>,
    menu: openxr::Action<bool>,
    haptic: openxr::Action<Haptic>,
    /// /user/hand/left, /user/hand/right
    hands: [openxr::Path; 2],
}

/// An action space of one hand.
#[derive(Component)]
struct HandSpace {
    hand: usize,
    aim: bool,
}

/// Recentre the headset on the next tracked frame.
#[derive(Resource, Default)]
struct Recenter(bool);

fn create_actions(instance: Res<OxrInstance>, mut commands: Commands) {
    let result = (|| -> openxr::Result<XrActions> {
        let hands = [instance.string_to_path("/user/hand/left")?, instance.string_to_path("/user/hand/right")?];
        let set = instance.create_action_set("cockpit", "Cockpit", 0)?;
        Ok(XrActions {
            grip: set.create_action("grip_pose", "Grip pose", &hands)?,
            aim: set.create_action("aim_pose", "Aim pose", &hands)?,
            squeeze: set.create_action("squeeze", "Grab", &hands)?,
            trigger: set.create_action("trigger", "Trigger", &hands)?,
            primary: set.create_action("primary", "Primary button", &hands)?,
            secondary: set.create_action("secondary", "Secondary button", &hands)?,
            thumb: set.create_action("thumb_click", "Thumbstick click", &hands)?,
            menu: set.create_action("menu", "Menu / recentre", &hands)?,
            haptic: set.create_action("haptic", "Haptics", &hands)?,
            set,
            hands,
        })
    })();
    match result {
        Ok(a) => commands.insert_resource(a),
        Err(e) => error!("OpenXR actions: {e}"),
    }
}

/// No OpenXR runtime: carry on as a desktop game (keeping the 3D cockpit).
fn no_runtime(mut settings: ResMut<crate::settings::Settings>) {
    warn!("no OpenXR runtime available, running on the desktop");
    settings.vr = false;
}

fn find_root(root: Query<Entity, With<XrTrackingRoot>>, mut xr_root: ResMut<XrRoot>) {
    xr_root.0 = root.single().ok();
}

fn suggest_bindings(actions: Option<Res<XrActions>>, mut out: MessageWriter<OxrSuggestActionBinding>) {
    let Some(a) = actions else { return };
    let both = |s: &str| vec![format!("/user/hand/left/{s}").into(), format!("/user/hand/right/{s}").into()];
    let per_hand = |l: &str, r: &str| vec![format!("/user/hand/left/{l}").into(), format!("/user/hand/right/{r}").into()];
    let left = |s: &str| vec![format!("/user/hand/left/{s}").into()];
    // (profile, squeeze, trigger, primary, secondary, thumb, menu)
    type B = Vec<std::borrow::Cow<'static, str>>;
    let profiles: Vec<(&str, B, B, B, B, B, B)> = vec![
        (
            "/interaction_profiles/oculus/touch_controller",
            both("input/squeeze/value"),
            both("input/trigger/value"),
            per_hand("input/x/click", "input/a/click"),
            per_hand("input/y/click", "input/b/click"),
            both("input/thumbstick/click"),
            left("input/menu/click"),
        ),
        (
            "/interaction_profiles/valve/index_controller",
            both("input/squeeze/value"),
            both("input/trigger/value"),
            both("input/a/click"),
            both("input/b/click"),
            both("input/thumbstick/click"),
            vec![],
        ),
        (
            "/interaction_profiles/htc/vive_controller",
            both("input/squeeze/click"),
            both("input/trigger/value"),
            both("input/trackpad/click"),
            vec![],
            vec![],
            both("input/menu/click"),
        ),
        (
            "/interaction_profiles/microsoft/motion_controller",
            both("input/squeeze/click"),
            both("input/trigger/value"),
            both("input/trackpad/click"),
            vec![],
            both("input/thumbstick/click"),
            both("input/menu/click"),
        ),
        ("/interaction_profiles/khr/simple_controller", vec![], both("input/select/click"), vec![], vec![], vec![], both("input/menu/click")),
    ];
    for (profile, squeeze, trigger, primary, secondary, thumb, menu) in profiles {
        let mut add = |action: openxr::sys::Action, bindings: B| {
            if !bindings.is_empty() {
                out.write(OxrSuggestActionBinding { action, interaction_profile: profile.into(), bindings });
            }
        };
        add(a.grip.as_raw(), both("input/grip/pose"));
        add(a.aim.as_raw(), both("input/aim/pose"));
        add(a.haptic.as_raw(), both("output/haptic"));
        add(a.squeeze.as_raw(), squeeze);
        add(a.trigger.as_raw(), trigger);
        add(a.primary.as_raw(), primary);
        add(a.secondary.as_raw(), secondary);
        add(a.thumb.as_raw(), thumb);
        add(a.menu.as_raw(), menu);
    }
}

fn attach_set(actions: Option<Res<XrActions>>, mut attach: MessageWriter<OxrAttachActionSet>) {
    if let Some(a) = actions {
        attach.write(OxrAttachActionSet(a.set.clone()));
    }
}

fn sync_actions(actions: Res<XrActions>, mut sync: MessageWriter<OxrSyncActionSet>) {
    sync.write(OxrSyncActionSet(actions.set.clone()));
}

fn spawn_hand_spaces(actions: Option<Res<XrActions>>, session: Res<OxrSession>, mut commands: Commands) {
    let Some(a) = actions else { return };
    for hand in 0..2 {
        for aim in [false, true] {
            let action = if aim { &a.aim } else { &a.grip };
            match session.create_action_space(action, a.hands[hand], Isometry3d::IDENTITY) {
                Ok(space) => {
                    commands.spawn((space, HandSpace { hand, aim }));
                }
                Err(e) => error!("OpenXR hand space: {e}"),
            }
        }
    }
}

fn despawn_hand_spaces(mut commands: Commands, q: Query<Entity, With<HandSpace>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Controller state and poses into `Hands` (cockpit frame = rig offset * tracking-root frame).
fn read_hands(
    actions: Res<XrActions>,
    session: Res<OxrSession>,
    offset: Res<RigOffset>,
    view: Res<ViewState>,
    spaces: Query<(&HandSpace, &Transform, &OxrSpaceLocationFlags)>,
    mut hands: ResMut<Hands>,
) {
    let cockpit_view = view.view == View::Cockpit;
    for (i, hand) in hands.hands.iter_mut().enumerate() {
        let path = actions.hands[i];
        let f = |a: &openxr::Action<f32>| a.state(&session, path).map(|s| s.current_state).unwrap_or(0.0);
        let b = |a: &openxr::Action<bool>| a.state(&session, path).is_ok_and(|s| s.current_state);
        hand.prev_buttons = hand.buttons;
        hand.prev_trigger = hand.trigger;
        hand.squeeze = f(&actions.squeeze);
        hand.trigger = f(&actions.trigger);
        hand.buttons = (b(&actions.primary) as u8 * BUTTON_PRIMARY)
            | (b(&actions.secondary) as u8 * BUTTON_SECONDARY)
            | (b(&actions.thumb) as u8 * BUTTON_THUMB)
            | (b(&actions.menu) as u8 * BUTTON_MENU);
        let mut grip = None;
        let mut aim = None;
        for (s, tf, flags) in &spaces {
            if s.hand == i && flags.pos_valid() && flags.rot_valid() {
                let t = offset.0 * *tf;
                if s.aim { aim = Some(t) } else { grip = Some(t) }
            }
        }
        hand.active = cockpit_view && grip.is_some();
        if let Some(g) = grip {
            hand.grip = g;
            // Fingertip: a little ahead of the aim pose (the controller's pointing end).
            hand.finger = aim.map_or(g.transform_point(Vec3::new(0.0, -0.02, -0.09)), |a| a.transform_point(Vec3::new(0.0, 0.0, -0.02)));
        }
    }
}

fn haptics(actions: Res<XrActions>, session: Res<OxrSession>, mut hands: ResMut<Hands>) {
    for (i, hand) in hands.hands.iter_mut().enumerate() {
        if hand.haptic > 0.0 {
            let v = openxr::HapticVibration::new()
                .amplitude(hand.haptic.min(1.0))
                .duration(openxr::Duration::from_nanos(25_000_000))
                .frequency(openxr::FREQUENCY_UNSPECIFIED);
            if let Err(e) = actions.haptic.apply_feedback(&session, actions.hands[i], &v) {
                debug!("haptics: {e}");
            }
            hand.haptic = 0.0;
        }
    }
}

/// Headset cameras: the main render layers (world + background sphere), no tonemapping.
fn setup_cameras(mut commands: Commands, cams: Query<Entity, Added<XrCamera>>) {
    for e in &cams {
        commands.entity(e).insert((RenderLayers::from_layers(&[0, MAIN_BACKGROUND_LAYER, crate::camera::MAIN_LOD_LAYER]), Tonemapping::None));
    }
}

/// Puts the current head pose at the design eye point, facing forward.
fn recenter(
    mut pending: ResMut<Recenter>,
    keys: Res<ButtonInput<KeyCode>>,
    hands: Res<Hands>,
    cams: Query<&Transform, With<XrCamera>>,
    mut offset: ResMut<RigOffset>,
) {
    if keys.just_pressed(KeyCode::Home) || hands.hands[0].just_pressed(BUTTON_MENU) {
        pending.0 = true;
    }
    if !pending.0 {
        return;
    }
    let n = cams.iter().len();
    if n == 0 {
        return;
    }
    let pos = cams.iter().map(|t| t.translation).sum::<Vec3>() / n as f32;
    if pos == Vec3::ZERO {
        return; // not tracked yet
    }
    let fwd = cams.iter().next().map_or(Vec3::NEG_Z, |t| t.forward().as_vec3());
    let yaw = (-fwd.x).atan2(-fwd.z);
    let rot = Quat::from_rotation_y(-yaw);
    offset.0 = Transform::from_translation(-(rot * pos)).with_rotation(rot);
    pending.0 = false;
}

/// Takes the tracking root out of the aircraft before the flight entities are despawned.
fn detach_root(
    mut commands: Commands,
    root: Res<XrRoot>,
    children: Query<&Children>,
    background: Query<(), (With<Mesh3d>, With<RenderLayers>)>,
) {
    let Some(r) = root.0 else { return };
    if let Ok(kids) = children.get(r) {
        for k in kids.iter().filter(|k| background.contains(*k)) {
            commands.entity(k).despawn();
        }
    }
    commands.entity(r).remove::<(ChildOf, EyeCamera)>().insert(Transform::IDENTITY);
}
