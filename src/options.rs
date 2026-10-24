//! The main menu's OPTIONS window: the player's own settings, saved in the preferences with the
//! menu's other choices (`prefs.rs`), on four tabs:
//!
//! - GAME: cockpit, HUD, avionics and flight model style, auto trim, the cockpit tooltips, the
//!   VR cockpit size and Tacview.
//! - GRAPHICS: shaders, anti-aliasing, shadows, point lights, time of day.
//! - SOUND: the master, engine, effects and warning volumes, the engine low-pass inside the
//!   aircraft.
//! - CONTROLS: the key and joystick bindings (the CONTROLS dialog, `input_dialog.rs`) and head
//!   tracking.
//!
//! The realism switches and crash protection belong to each game: they stay in the single
//! mission, career and campaign setups. The in-flight menu has the same pages, but for the
//! flight model and Tacview, which take effect at the next flight (`flight_menu.rs`).

use bevy::prelude::*;
use bevy_egui::egui;

use crate::menu::{
    AMBER, BRIGHT, DIM, GREEN, big_button, facts, menu_window, section, slide_switch,
};
use crate::settings::{Settings, VR_COCKPIT_SCALE_RANGE};

#[derive(Resource, Default)]
pub struct OptionsWindow {
    pub open: bool,
    tab: Tab,
}

impl OptionsWindow {
    /// `--open-options <tab>`: open on that tab (screenshots).
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let Some(i) = args.iter().position(|a| a == "--open-options") else {
            return Self::default();
        };
        let name = args.get(i + 1).map_or("", |s| s.as_str()).to_ascii_uppercase();
        let tab = Tab::ALL.iter().find(|(_, n)| *n == name).map_or(Tab::Game, |t| t.0);
        Self { open: true, tab }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Game,
    Graphics,
    Sound,
    Controls,
}

impl Tab {
    const ALL: [(Tab, &'static str); 4] = [
        (Tab::Game, "GAME"),
        (Tab::Graphics, "GRAPHICS"),
        (Tab::Sound, "SOUND"),
        (Tab::Controls, "CONTROLS"),
    ];
}

type SettingsMut<'a> = ResMut<'a, Settings>;

/// The window (while open and the bindings dialog is not).
pub fn options_ui(
    ctx: &egui::Context,
    w: &mut OptionsWindow,
    settings: &mut SettingsMut,
    head: Option<crate::headtrack::Source>,
    dialog: &mut crate::input_dialog::InputDialog,
) {
    if !w.open || dialog.open {
        return;
    }
    let mut back = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    menu_window(ctx, "Options", 460.0).show(ctx, |ui| {
        ui.label(egui::RichText::new("OPTIONS").heading().color(BRIGHT));
        ui.horizontal(|ui| {
            for (tab, name) in Tab::ALL {
                ui.selectable_value(&mut w.tab, tab, egui::RichText::new(name).size(16.0));
            }
        });
        ui.separator();
        match w.tab {
            Tab::Game => game_options(ui, settings),
            Tab::Graphics => graphics_options(ui, settings),
            Tab::Sound => sound_options(ui, settings),
            Tab::Controls => control_options(ui, settings, head, dialog),
        }
        ui.add_space(8.0);
        ui.separator();
        ui.label(
            egui::RichText::new(
                "Saved for the next flights; the realism switches are chosen with each game",
            )
            .small()
            .color(DIM),
        );
        if facts(ui.add(big_button("BACK")), &["Back to the main menu"]).clicked() {
            back = true;
        }
    });
    if back {
        w.open = false;
    }
}

/// CLASSIC / MODERN switches with their facts: true if one is flipped.
fn switches<const N: usize>(ui: &mut egui::Ui, list: [(&mut bool, &str, &[&str]); N]) -> bool {
    let mut toggled = false;
    for (modern, name, f) in list {
        toggled |= facts(slide_switch(ui, name, modern), f).changed();
    }
    toggled
}

fn game_options(ui: &mut egui::Ui, settings: &mut SettingsMut) {
    section(ui, "Classic / Modern");
    let s = settings.bypass_change_detection();
    let (mut cockpit, mut hud) = (!s.classic_cockpit, !s.classic_hud);
    let (mut avionics, mut flight) = (!s.classic_avionics, !s.classic_flight);
    let mut toggled = switches(
        ui,
        [
            (
                &mut cockpit,
                "Cockpit",
                &[
                    "Modern: high-resolution displays with bezel buttons",
                    "Classic: the original instrument panel layout and 77x60 displays (F-117A)",
                ][..],
            ),
            (
                &mut hud,
                "HUD",
                &[
                    "Modern: high-resolution F-16 style HUD in green with NAV, AA and AG modes",
                    "Classic: the original HUD",
                ][..],
            ),
            (
                &mut avionics,
                "Avionics",
                &[
                    "Modern: F-16 style MFD formats on either MFD (situation display, HARM, targeting pod, map, stores) with a slewable cursor",
                    "Classic: the original's map and TAC left, camera, WAY, WPN and ORD right",
                ][..],
            ),
            (
                &mut flight,
                "Flight model",
                &[
                    "Modern: aerodynamic flight model with fly-by-wire: the stick commands pitch and roll rate; released, the nose stays put and the flight path is held",
                    "Classic: the original game's flight model",
                ][..],
            ),
        ],
    );
    let s = settings.bypass_change_detection();
    s.classic_cockpit = !cockpit;
    s.classic_hud = !hud;
    s.classic_avionics = !avionics;
    s.classic_flight = !flight;
    section(ui, "Flying");
    toggled |= facts(
        ui.checkbox(&mut s.auto_trim, "Auto trim"),
        &[
            "With the stick released, the trim holds the attitude where you left it",
            "Helicopters: no more nose rising with speed; jets: trims out damaged control surfaces",
            "Manual trim: Alt + arrow keys, Alt + Home centres it",
        ],
    )
    .changed();
    toggled |= facts(
        ui.checkbox(&mut s.tooltips, "Cockpit tooltips"),
        &["What a cockpit control or display is, under the mouse or a VR fingertip"],
    )
    .changed();
    section(ui, "VR cockpit size");
    let mut percent = s.vr_cockpit_scale * 100.0;
    let range = VR_COCKPIT_SCALE_RANGE.start() * 100.0..=VR_COCKPIT_SCALE_RANGE.end() * 100.0;
    if facts(
        ui.add(egui::Slider::new(&mut percent, range).step_by(5.0).fixed_decimals(0).suffix("%")),
        &["Size of the cockpit (and the world) in the headset", "100%: life size"],
    )
    .changed()
    {
        s.vr_cockpit_scale = percent / 100.0;
        toggled = true;
    }
    if toggled {
        settings.set_changed();
    }
    crate::menu::tacview_options(ui, settings);
}

fn graphics_options(ui: &mut egui::Ui, settings: &mut SettingsMut) {
    section(ui, "Rendering");
    let s = settings.bypass_change_detection();
    let mut toggled = false;
    for (modern, name, f) in [
        (
            &mut s.modern_shading,
            "Shaders",
            &[
                "Modern: subtle directional shading, smooth distance haze",
                "Classic: the original's flat shading",
            ][..],
        ),
        (
            &mut s.shadows,
            "Shadows",
            &[
                "Modern: sun shadows in the cockpit and the world (costs GPU time)",
                "Classic: no shadows",
            ][..],
        ),
        (
            &mut s.local_lights,
            "Point lights",
            &[
                "Modern: cockpit flood and display lights; explosions, plumes and runway lights light the scene",
                "Classic: sun and sky light only",
            ][..],
        ),
        (
            &mut s.continuous_time,
            "Time of day",
            &[
                "Modern: the sun and the moon move with the mission clock, the sky darkens and brightens with the sun; stars and the moon's phase at night",
                "Modern: moonlight sets how far enemy pilots see you at night (2 km on a moonless night, 4 km under a full moon)",
                "Classic: the original's slow change of colours and a fixed sun",
            ][..],
        ),
    ] {
        toggled |= facts(slide_switch(ui, name, modern), f).changed();
    }
    toggled |= facts(
        ui.checkbox(&mut s.msaa, "Anti-aliasing (MSAA)"),
        &["Smooth edges: 4x MSAA, 2x in VR (costs GPU time)", "Off: the original's hard pixel edges"],
    )
    .changed();
    // The classic time of day can still show the sun and the moon.
    if !s.continuous_time {
        ui.indent("sun_moon", |ui| {
            toggled |= facts(
                ui.checkbox(&mut s.sun_moon, "Sun and moon"),
                &[
                    "The sun in the sky by day, the moon and the stars by night",
                    "The moon shows the mission's phase",
                ],
            )
            .changed();
        });
    }
    if toggled {
        settings.set_changed();
    }
}

fn sound_options(ui: &mut egui::Ui, settings: &mut SettingsMut) {
    section(ui, "Volume");
    let s = settings.bypass_change_detection();
    let mut changed = false;
    for (v, name, f) in [
        (&mut s.master_volume, "Master", &["Every sound"][..]),
        (
            &mut s.engine_volume,
            "Engine",
            &["Your engine or rotor, and the aircraft heard around you"][..],
        ),
        (&mut s.effects_volume, "Effects", &["Weapons, explosions, gear, radio and the rest"][..]),
        (
            &mut s.warning_volume,
            "Warnings",
            &["Stall horn, radar and missile warnings, the fire siren, the Sidewinder tones"][..],
        ),
    ] {
        let mut percent = (*v * 100.0).round();
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = 220.0;
            ui.label(format!("{:<9}", name.to_ascii_uppercase()));
            let slider = egui::Slider::new(&mut percent, 0.0..=100.0)
                .step_by(5.0)
                .fixed_decimals(0)
                .suffix("%");
            if facts(ui.add(slider), f).changed() {
                *v = percent / 100.0;
                changed = true;
            }
        });
    }
    section(ui, "Cockpit");
    changed |= facts(
        ui.checkbox(&mut s.engine_low_pass, "Engine low-pass"),
        &[
            "Inside the aircraft the engine sounds muffled (the canopy and helmet take the highs off)",
            "Off: the same as outside",
        ],
    )
    .changed();
    if changed {
        settings.set_changed();
    }
}

fn control_options(
    ui: &mut egui::Ui,
    settings: &mut SettingsMut,
    head: Option<crate::headtrack::Source>,
    dialog: &mut crate::input_dialog::InputDialog,
) {
    section(ui, "Bindings");
    if facts(
        ui.add(big_button("KEYS AND JOYSTICK").min_size(egui::vec2(300.0, 44.0))),
        &["Map keys, joystick and HOTAS axes and buttons", "Saved for the next flights"],
    )
    .clicked()
    {
        dialog.open = true;
    }
    // The browser build hears no tracking program.
    if cfg!(target_arch = "wasm32") {
        return;
    }
    section(ui, "Head/eye tracking");
    // Read every frame by `headtrack`: no change detection needed.
    let s = settings.bypass_change_detection();
    facts(
        ui.add_enabled(!s.vr, egui::Checkbox::new(&mut s.head_tracking, "Head/Eye Tracking")),
        &[
            "Your head turns and moves the view in the cockpit (desktop; in VR the headset does)",
            "Tracking program (OpenTrack...): FreeTrack output, or UDP to 127.0.0.1 port 4242",
            "Home: the current head position becomes the centre",
        ],
    );
    if s.head_tracking && !s.vr {
        match head {
            Some(src) => ui.colored_label(GREEN, format!("Tracking ({})", src.name())),
            None => ui.colored_label(AMBER, "No tracker heard: start the tracking program"),
        };
    }
}
