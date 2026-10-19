//! Main menu: theater selector and mission generation (replaces the original START.EXE menus).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui};

use f117_mission::{Mission, MissionKind, Sky};

use crate::mission::{ActiveMission, MissionData, clean_text};
use crate::settings::{AppState, MissionSetup};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        // The menu camera owns the egui context (the flight has several cameras, e.g. the
        // tracking camera, which must not get one).
        app.add_plugins(EguiPlugin::default())
            .insert_resource(EguiGlobalSettings { auto_create_primary_context: false, ..default() })
            .init_resource::<MenuState>()
            .insert_resource(VrAvailable(vr_available()))
            .add_systems(OnEnter(AppState::Menu), spawn_menu_camera)
            .add_systems(OnExit(AppState::Menu), despawn_menu_camera)
            .add_systems(EguiPrimaryContextPass, menu_ui.run_if(in_state(AppState::Menu)));
    }
}

/// Theaters in the original's order (file prefix, display name, era text).
pub const THEATERS: [(&str, &str); 9] = [
    ("lb", "Libya (1986)"),
    ("pg", "Persian Gulf (1984)"),
    ("nc", "North Cape"),
    ("ce", "Central Europe"),
    ("vn", "Vietnam"),
    ("me", "Middle East"),
    ("ku", "Desert Storm"),
    ("cu", "Cuba"),
    ("ko", "Korea"),
];

#[derive(Resource)]
pub struct MenuState {
    pub theater: usize,
    /// 0 Cold War, 1 Limited War, 2 Conventional War.
    pub roe: u8,
    pub kind: MissionKind,
    pub opponents: u8,
    pub lockheed: bool,
    /// None: rolled from the seed.
    pub weather: Option<Sky>,
    pub seed: u16,
    pub mission: Option<Result<Mission, String>>,
    dirty: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            theater: 3,
            roe: 1,
            kind: MissionKind::Strike,
            opponents: 1,
            lockheed: false,
            weather: None,
            seed: rand::random(),
            mission: None,
            dirty: true,
        }
    }
}

#[derive(Component)]
struct MenuCamera;

/// An OpenXR runtime is there, so "Fly in VR" can relaunch the game with `--vr`.
#[derive(Resource)]
struct VrAvailable(bool);

fn vr_available() -> bool {
    #[cfg(feature = "xr")]
    {
        let ok = crate::xr::runtime_available();
        info!("OpenXR runtime available: {ok}");
        ok
    }
    #[cfg(not(feature = "xr"))]
    {
        false
    }
}

/// Starts the game again in VR, straight into the menu's mission (OpenXR has to replace the
/// renderer at startup, so this cannot happen in the running process).
fn relaunch_in_vr(state: &MenuState, settings: &crate::settings::Settings) -> std::io::Result<()> {
    // From an AppImage, the AppImage itself: the mounted copy goes away when this process exits.
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) => appimage.into(),
        None => std::env::current_exe()?,
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["--vr", "--mission", &state.seed.to_string(), "--theater-index", &state.theater.to_string()])
        .args(["--roe", &state.roe.to_string(), "--kind", &(state.kind as u8).to_string()])
        .args(["--opponents", &state.opponents.to_string()])
        .args(["--weather", state.weather.map_or("random", Sky::name)]);
    for (on, flag) in [
        (state.lockheed, "--lockheed"),
        (settings.modern_shading, "--modern"),
        (settings.shadows, "--shadows"),
        (settings.local_lights, "--local-lights"),
        (settings.classic_cockpit, "--classic-cockpit"),
    ] {
        if on {
            cmd.arg(flag);
        }
    }
    cmd.spawn().map(|_| ())
}

fn spawn_menu_camera(mut commands: Commands) {
    commands.spawn((Camera2d, MenuCamera, PrimaryEguiContext));
}

fn despawn_menu_camera(mut commands: Commands, q: Query<Entity, With<MenuCamera>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

#[allow(clippy::too_many_arguments)]
fn menu_ui(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut state: ResMut<MenuState>,
    data: Option<Res<MissionData>>,
    tables: Res<crate::flight::SimTables>,
    mut setup: ResMut<MissionSetup>,
    mut settings: ResMut<crate::settings::Settings>,
    mut next: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
    vr: Res<VrAvailable>,
) -> Result {
    // Generator data is loaded on first use.
    let Some(data) = data else {
        match MissionData::load() {
            Ok(d) => commands.insert_resource(d),
            Err(e) => state.mission = Some(Err(e.to_string())),
        }
        return Ok(());
    };
    if state.dirty {
        state.dirty = false;
        let settings = crate::mission::settings(state.theater, state.roe, state.kind, state.opponents, state.lockheed, state.weather);
        state.mission = Some(crate::mission::generate(&data, &settings, state.seed).map_err(|e| format!("{e:#}")));
    }

    let ctx = contexts.ctx_mut()?;
    let before = (state.theater, state.roe, state.kind, state.opponents, state.lockheed, state.weather);
    egui::Window::new("F-117A Nighthawk")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .show(ctx, |ui| {
            ui.heading("F-117A Nighthawk Stealth Fighter");
            ui.separator();
            ui.columns(3, |cols| {
                cols[0].label("Theater");
                for (i, (_, name)) in THEATERS.iter().enumerate() {
                    cols[0].radio_value(&mut state.theater, i, *name);
                }
                cols[1].label("Situation");
                for (i, n) in ["Cold War", "Limited War", "Conventional War"].iter().enumerate() {
                    cols[1].radio_value(&mut state.roe, i as u8, *n);
                }
                cols[1].separator();
                cols[1].label("Mission");
                cols[1].radio_value(&mut state.kind, MissionKind::Strike, "Strike");
                cols[1].radio_value(&mut state.kind, MissionKind::Air, "Air to air");
                cols[1].radio_value(&mut state.kind, MissionKind::StrikeTraining, "Strike training");
                cols[1].radio_value(&mut state.kind, MissionKind::AirTraining, "Air training");
                cols[1].separator();
                cols[1].label("Weather");
                cols[1].radio_value(&mut state.weather, None, "Random");
                for sky in Sky::ALL {
                    cols[1].radio_value(&mut state.weather, Some(sky), sky.name());
                }
                cols[2].label("Opponents");
                for (i, n) in ["Green", "Regular", "Veteran", "Elite"].iter().enumerate() {
                    cols[2].radio_value(&mut state.opponents, i as u8, *n);
                }
                cols[2].separator();
                cols[2].checkbox(&mut state.lockheed, "Lockheed F-117A");
                cols[2].separator();
                cols[2].label("Graphics");
                let mut modern = settings.modern_shading;
                if cols[2].checkbox(&mut modern, "Modern shading").changed() {
                    settings.modern_shading = modern;
                }
                let mut shadows = settings.shadows;
                if cols[2].checkbox(&mut shadows, "Shadows").changed() {
                    settings.shadows = shadows;
                }
                let mut lights = settings.local_lights;
                if cols[2].checkbox(&mut lights, "Local lighting").changed() {
                    settings.local_lights = lights;
                }
                let mut classic = settings.classic_cockpit;
                if cols[2].checkbox(&mut classic, "Classic cockpit").changed() {
                    settings.classic_cockpit = classic;
                }
            });
            ui.separator();
            match &state.mission {
                Some(Ok(m)) => {
                    let b = &m.briefing;
                    ui.strong(clean_text(&b.title));
                    for l in [&b.primary_objective, &b.secondary_objective, &b.depart_line, &b.land_line] {
                        ui.label(clean_text(l));
                    }
                    ui.label(format!("{}  ·  fuel {} lbs", if m.night { "Night" } else { "Day" }, m.fuel_lbs));
                    ui.label(&b.weather_line);
                    ui.label(b.armament.iter().map(|a| clean_text(a)).collect::<Vec<_>>().join(", "));
                    ui.collapsing("Intelligence", |ui| {
                        ui.label(clean_text(&b.primary_summary));
                        ui.label(clean_text(&b.secondary_summary));
                        ui.label(clean_text(&b.roe_text));
                    });
                }
                Some(Err(e)) => {
                    ui.colored_label(egui::Color32::RED, e);
                }
                None => {
                    ui.label("Generating...");
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("New mission").clicked() {
                    state.seed = rand::random();
                    state.dirty = true;
                }
                let can_fly = matches!(state.mission, Some(Ok(_)));
                if ui.add_enabled(can_fly, egui::Button::new("  Fly  ")).clicked()
                    && let Some(Ok(m)) = &state.mission
                {
                    *setup = crate::mission::setup_from_mission(m, &tables.start_dir);
                    commands.insert_resource(ActiveMission(m.clone()));
                    next.set(AppState::Flight);
                }
                // Already in VR: "Fly" flies in the headset.
                if !settings.vr {
                    let vr_button = ui
                        .add_enabled(can_fly && vr.0, egui::Button::new("Fly in VR"))
                        .on_disabled_hover_text(if vr.0 { "Generate a mission first" } else { "No OpenXR runtime (Monado, WiVRn, SteamVR) is active" });
                    if vr_button.clicked() {
                        match relaunch_in_vr(&state, &settings) {
                            Ok(()) => {
                                exit.write(AppExit::Success);
                            }
                            Err(e) => state.mission = Some(Err(format!("could not start in VR: {e}"))),
                        }
                    }
                }
                if ui.button("Quit").clicked() {
                    exit.write(AppExit::Success);
                }
            });
        });
    if before != (state.theater, state.roe, state.kind, state.opponents, state.lockheed, state.weather) {
        state.dirty = true;
    }
    Ok(())
}
