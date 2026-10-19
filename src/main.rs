mod asset_path;
mod autoshot;
mod camera;
mod canvas;
mod cockpit;
mod cockpit3d;
mod combat;
mod dots;
mod flight;
mod font;
mod freecam;
mod hud;
mod input;
mod lighting;
mod lightning;
mod mfd;
mod menu;
mod mission;
mod models;
mod palette;
mod palette_material;
mod panel;
mod player;
mod scenery;
mod script;
mod settings;
mod sound;
mod sim_render;
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
    let settings = Settings::from_args();
    let mut app = App::new();
    let plugins = DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "F-117A Nighthawk".into(),
            // Wayland app id / X11 class, matching packaging/linux/f117.desktop.
            name: Some("f117".into()),
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
    app.insert_state(settings.initial_state)
    .insert_resource(settings.clone())
    .insert_resource(MissionSetup::from_args())
    .insert_resource(flight::SimTables::load())
    .add_plugins(settings::configure)
    .add_plugins((palette::PalettePlugin, models::ModelsPlugin, dots::DotsPlugin, canvas::CanvasPlugin))
    .add_plugins((menu::MenuPlugin, viewer::ViewerPlugin))
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
    .add_plugins((combat::CombatPlugin, sim_render::SimRenderPlugin, trackcam::TrackCamPlugin, trails::TrailsPlugin))
    .add_plugins((lighting::LightingPlugin, cockpit3d::Cockpit3dPlugin, scenery::SceneryPlugin, weather::WeatherPlugin));
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
