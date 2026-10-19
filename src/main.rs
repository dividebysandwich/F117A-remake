mod asset_path;
mod autoshot;
mod camera;
mod canvas;
mod cockpit;
mod cockpit3d;
mod combat;
mod crash_log;
mod dots;
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
mod player;
mod scenery;
mod script;
mod settings;
mod sound;
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

/// GPU mesh preprocessing (Bevy's compute-built mesh uniforms and indirect draws). Off on ARM:
/// on the Steam Frame (Adreno 750, Turnip) it hangs the GPU while scenery streams in; Bevy's own
/// Adreno blocklist stops at the 730. `--gpu-preprocessing` / `--no-gpu-preprocessing` override.
fn gpu_preprocessing() -> bool {
    let has = |f: &str| std::env::args().any(|a| a == f);
    let on = if has("--gpu-preprocessing") {
        true
    } else if has("--no-gpu-preprocessing") {
        false
    } else {
        !cfg!(target_arch = "aarch64")
    };
    eprintln!("GPU mesh preprocessing: {}", if on { "on" } else { "off" });
    on
}

/// Makes the renderer treat GPU preprocessing as unsupported (as Bevy does for older Adreno and
/// Mali drivers), so the views batch on the CPU like the meshes; with only the meshes switched
/// (`PbrPlugin`), the views still expect GPU-built data and draw nothing.
fn no_gpu_preprocessing(app: &mut App) {
    use bevy::render::batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport};
    use bevy::render::{RenderApp, RenderStartup, init_gpu_resource};
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.add_systems(
            RenderStartup,
            (|mut support: ResMut<GpuPreprocessingSupport>| support.max_supported_mode = GpuPreprocessingMode::None)
                .after(init_gpu_resource::<GpuPreprocessingSupport>),
        );
    }
}

fn main() {
    let settings = Settings::from_args();
    let gpu_preprocessing = gpu_preprocessing();
    let mut app = App::new();
    let plugins = DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Stealth Mission 89".into(),
            // Wayland app id / X11 class, matching packaging/linux/stealth-mission-89.desktop.
            name: Some("stealth-mission-89".into()),
            // --novsync: uncapped frame rate (performance measurements).
            present_mode: if std::env::args().any(|a| a == "--novsync") {
                bevy::window::PresentMode::AutoNoVsync
            } else {
                bevy::window::PresentMode::AutoVsync
            },
            ..default()
        }),
        ..default()
    })
    .set(bevy::pbr::PbrPlugin { use_gpu_instance_buffer_builder: gpu_preprocessing, ..default() });
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
    if !gpu_preprocessing {
        no_gpu_preprocessing(&mut app);
    }
    app.add_plugins(crash_log::CrashLogPlugin);
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
    .add_plugins((lighting::LightingPlugin, cockpit3d::Cockpit3dPlugin, scenery::SceneryPlugin, weather::WeatherPlugin))
    .add_plugins((fires::FiresPlugin, terrain::TerrainPlugin));
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
