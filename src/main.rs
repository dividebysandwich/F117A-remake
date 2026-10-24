mod aircraft;
mod asset_path;
mod autoshot;
mod avionics;
mod boss_key;
mod camera;
mod campaign;
mod canvas;
mod career;
mod chat;
mod cockpit;
mod cockpit3d;
mod combat;
mod crash_log;
mod csar_view;
mod debrief;
#[cfg(not(target_arch = "wasm32"))]
mod dedicated;
mod dots;
mod explosions;
mod fires;
mod flight;
mod flight_menu;
mod font;
mod freecam;
mod headtrack;
mod help;
mod hmd;
mod hotas;
mod hud;
mod input;
mod input_dialog;
mod key_guide;
mod keyboard_layout;
mod kneeboard;
mod lighting;
mod lightning;
mod loadout;
mod map_draw;
mod menu;
mod menu_scene;
mod mfd;
mod mission;
mod models;
mod multiplayer_menu;
mod net;
mod net_interp;
mod options;
mod palette;
mod palette_material;
mod panel;
mod planner;
mod player;
mod player_notices;
mod prefs;
mod profile;
mod recon;
mod recorder;
mod rotor_blur;
mod rwr;
mod scenery;
mod script;
mod settings;
mod sim_render;
mod sky;
mod sound;
mod stage;
mod tacview;
mod tacview_real_time;
mod terrain;
mod text_entry;
mod trackcam;
mod trails;
mod viewer;
#[cfg(feature = "xr")]
mod vr_menu;
mod weather;
#[cfg(windows)]
mod win_console;
mod world;
#[cfg(feature = "xr")]
mod xr;

use bevy::prelude::*;

use settings::{MissionSetup, Settings};

fn main() {
    // `--dedicated`: the coop server, in the console it was started from.
    #[cfg(not(target_arch = "wasm32"))]
    dedicated::run_if_asked();
    // `--help`: the command line options.
    help::run_if_asked();
    #[cfg(windows)]
    win_console::leave_explorer_console();
    let mut settings = Settings::from_args();
    // Started into the menu (also in VR): the menu choices and switches from the last run.
    let mut menu_state = menu::MenuState::default();
    if settings.initial_state == settings::AppState::Menu {
        prefs::Prefs::load().apply(&mut menu_state, &mut settings);
    }
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "Stealth Mission 89".into(),
                // Wayland app id / X11 class, matching packaging/linux/stealth-mission-89.desktop.
                name: Some("stealth-mission-89".into()),
                // `--window-size WxH` (screenshots).
                resolution: std::env::args()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .find(|w| w[0] == "--window-size")
                    .and_then(|w| {
                        w[1].split_once('x').and_then(|(a, b)| {
                            Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
                        })
                    })
                    .map_or_else(Default::default, |(w, h)| {
                        bevy::window::WindowResolution::new(w, h)
                    }),
                // --novsync: uncapped frame rate (performance measurements).
                present_mode: if std::env::args().any(|a| a == "--novsync") {
                    bevy::window::PresentMode::AutoNoVsync
                } else {
                    bevy::window::PresentMode::AutoVsync
                },
                // The browser build: the page's canvas, filling it; the page passes the keys
                // (F1, Tab...) and right clicks (mouse look) to the game.
                #[cfg(target_arch = "wasm32")]
                canvas: Some("#sm89".into()),
                #[cfg(target_arch = "wasm32")]
                fit_canvas_to_parent: true,
                #[cfg(target_arch = "wasm32")]
                prevent_default_event_handling: true,
                ..default()
            }),
            ..default()
        })
        // The browser build fetches the assets from the page's server, which has no `.meta`
        // files (asking for each would log a 404 per asset).
        .set(AssetPlugin {
            #[cfg(target_arch = "wasm32")]
            meta_check: bevy::asset::AssetMetaCheck::Never,
            ..default()
        })
        .set(bevy::log::LogPlugin {
            filter: format!("{}{}", bevy::log::DEFAULT_FILTER, profile::log_filter()),
            custom_layer: profile::log_layer,
            ..default()
        });
    // --vr: OpenXR replaces the render plugin (the window stays for the menu / mirror).
    #[cfg(feature = "xr")]
    if settings.vr {
        app.add_plugins(xr::plugins(plugins)).add_plugins((xr::XrPlugin, vr_menu::VrMenuPlugin));
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
        .add_plugins((
            palette::PalettePlugin,
            models::ModelsPlugin,
            dots::DotsPlugin,
            canvas::CanvasPlugin,
            boss_key::BossKeyPlugin,
        ))
        .add_plugins((
            menu::MenuPlugin,
            menu_scene::MenuScenePlugin,
            debrief::DebriefPlugin,
            viewer::ViewerPlugin,
        ))
        .add_plugins((
            world::WorldPlugin,
            freecam::FreeCamPlugin,
            headtrack::HeadTrackPlugin,
            input::InputPlugin,
            keyboard_layout::KeyboardLayoutPlugin,
            cockpit::CockpitPlugin,
            flight::FlightPlugin,
            camera::CameraPlugin,
            player::PlayerPlugin,
            script::ScriptPlugin,
            sound::SoundPlugin,
        ))
        .add_plugins((
            hud::HudPlugin,
            hmd::HmdPlugin,
            mfd::MfdPlugin,
            avionics::AvionicsPlugin,
            panel::PanelPlugin,
        ))
        .add_plugins((
            combat::CombatPlugin,
            sim_render::SimRenderPlugin,
            trackcam::TrackCamPlugin,
            trails::TrailsPlugin,
            rotor_blur::RotorBlurPlugin,
            explosions::ExplosionsPlugin,
            flight_menu::FlightMenuPlugin,
            kneeboard::KneeboardPlugin,
            recon::ReconPlugin,
            tacview_real_time::TacviewRealTimePlugin,
        ))
        .add_plugins((
            sky::SkyPlugin,
            lighting::LightingPlugin,
            cockpit3d::Cockpit3dPlugin,
            scenery::SceneryPlugin,
            weather::WeatherPlugin,
        ))
        .add_plugins((
            fires::FiresPlugin,
            terrain::TerrainPlugin,
            csar_view::CsarViewPlugin,
            stage::StagePlugin,
            campaign::CampaignPlugin,
            net::NetPlugin,
            player_notices::PlayerNoticesPlugin,
            chat::ChatPlugin,
            multiplayer_menu::MultiplayerMenuPlugin,
        ));
    mission::from_args(&mut app);
    campaign::from_args(&mut app);
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
    // SM89_PROFILE: timings in the log (see profile.rs).
    if let Some(p) = profile::config() {
        app.add_plugins(profile::ProfilePlugin(p));
    }
    if let Some(shot) = autoshot::from_args() {
        app.add_plugins(shot);
    }
    app.run();
}
