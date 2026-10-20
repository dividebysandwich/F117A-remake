mod asset_path;
mod autoshot;
mod camera;
mod canvas;
mod career;
mod cockpit;
mod cockpit3d;
mod combat;
mod crash_log;
mod csar_view;
mod debrief;
mod dots;
mod explosions;
mod fires;
mod flight;
mod font;
mod freecam;
mod hotas;
mod hud;
mod input;
mod input_dialog;
mod lighting;
mod lightning;
mod mfd;
mod menu;
mod mission;
mod models;
mod palette;
mod palette_material;
mod panel;
mod planner;
mod player;
mod prefs;
mod scenery;
mod script;
mod settings;
mod sound;
mod stage;
mod sim_render;
mod terrain;
mod trackcam;
mod trails;
mod viewer;
mod weather;
mod world;
#[cfg(feature = "xr")]
mod xr;

use bevy::prelude::*;

use settings::{MissionSetup, Settings};

fn main() {
    let mut settings = Settings::from_args();
    // Started into the menu: the menu choices and switches from the last run.
    let mut menu_state = menu::MenuState::default();
    if settings.initial_state == settings::AppState::Menu && !settings.vr {
        prefs::Prefs::load().apply(&mut menu_state, &mut settings);
    }
    let mut app = App::new();
    let plugins = DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Stealth Mission 89".into(),
            // Wayland app id / X11 class, matching packaging/linux/stealth-mission-89.desktop.
            name: Some("stealth-mission-89".into()),
            // `--window-size WxH` (screenshots).
            resolution: std::env::args()
                .collect::<Vec<_>>()
                .windows(2)
                .find(|w| w[0] == "--window-size")
                .and_then(|w| w[1].split_once('x').and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))))
                .map_or_else(Default::default, |(w, h)| bevy::window::WindowResolution::new(w, h)),
            // --novsync: uncapped frame rate (performance measurements).
            present_mode: if std::env::args().any(|a| a == "--novsync") {
                bevy::window::PresentMode::AutoNoVsync
            } else {
                bevy::window::PresentMode::AutoVsync
            },
            ..default()
        }),
        ..default()
    });
    // --vr: OpenXR replaces the render plugin (the window stays for the menu / mirror).
    #[cfg(feature = "xr")]
    if settings.vr {
        app.add_plugins(xr::plugins(plugins)).add_plugins(xr::XrPlugin);
    } else {
        app.add_plugins(plugins);
    }
    #[cfg(not(feature = "xr"))]
    {
        if settings.vr {
            warn!("built without the `xr` feature: --vr ignored");
        }
        app.add_plugins(plugins);
    }
    app.add_plugins(crash_log::CrashLogPlugin);
    app.insert_state(settings.initial_state)
    .insert_resource(settings.clone())
    .insert_resource(menu_state)
    .insert_resource(MissionSetup::from_args())
    .insert_resource(flight::SimTables::load())
    .add_plugins(settings::configure)
    .add_plugins((palette::PalettePlugin, models::ModelsPlugin, dots::DotsPlugin, canvas::CanvasPlugin))
    .add_plugins((menu::MenuPlugin, debrief::DebriefPlugin, viewer::ViewerPlugin))
    .add_plugins((
        world::WorldPlugin,
        freecam::FreeCamPlugin,
        input::InputPlugin,
        cockpit::CockpitPlugin,
        flight::FlightPlugin,
        camera::CameraPlugin,
        player::PlayerPlugin,
        script::ScriptPlugin,
        sound::SoundPlugin,
    ))
    .add_plugins((hud::HudPlugin, mfd::MfdPlugin, panel::PanelPlugin))
    .add_plugins((combat::CombatPlugin, sim_render::SimRenderPlugin, trackcam::TrackCamPlugin, trails::TrailsPlugin, explosions::ExplosionsPlugin))
    .add_plugins((lighting::LightingPlugin, cockpit3d::Cockpit3dPlugin, scenery::SceneryPlugin, weather::WeatherPlugin))
    .add_plugins((fires::FiresPlugin, terrain::TerrainPlugin, csar_view::CsarViewPlugin, stage::StagePlugin));
    mission::from_args(&mut app);
    if let Some(scale) = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|w| w[0] == "--time-scale")
        .and_then(|w| w[1].parse::<f32>().ok())
    {
        app.add_systems(Startup, move |mut t: ResMut<Time<Virtual>>| {
            t.set_max_delta(std::time::Duration::from_secs(1));
            t.set_relative_speed(scale);
        });
    }
    if std::env::args().any(|a| a == "--diag") {
        app.add_plugins((
            bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
            bevy::diagnostic::EntityCountDiagnosticsPlugin::default(),
            bevy::diagnostic::LogDiagnosticsPlugin::default(),
        ));
    }
    if let Some(shot) = autoshot::from_args() {
        app.add_plugins(shot);
    }
    app.run();
}
