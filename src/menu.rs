//! Main menu: theater selector and mission generation (replaces the original game's start menus).

use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui,
};

use stealth_mission::{Mission, MissionKind, Sky};

use crate::aircraft::AircraftKind;
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
            .init_resource::<crate::input_dialog::InputDialog>()
            // `--open-loadout`: start in the loadout editor (screenshots).
            .insert_resource(crate::loadout::LoadoutWindow::opened(
                std::env::args().any(|a| a == "--open-loadout"),
            ))
            .insert_resource(VrAvailable(vr_available()))
            .add_systems(OnEnter(AppState::Menu), spawn_menu_camera)
            .add_systems(OnExit(AppState::Menu), despawn_menu_camera)
            .add_systems(OnEnter(AppState::Debrief), spawn_menu_camera)
            .add_systems(OnExit(AppState::Debrief), despawn_menu_camera)
            // `--open-planner`: start in the mission planner (screenshots).
            .insert_resource(crate::planner::Planner::opened(
                std::env::args().any(|a| a == "--open-planner"),
            ))
            // `--open-career`: start in the career window (screenshots).
            .insert_resource(crate::career::CareerWindow::opened(
                std::env::args().any(|a| a == "--open-career"),
            ))
            .add_systems(
                Update,
                crate::planner::prepare_map
                    .run_if(in_state(AppState::Menu).or_else(in_state(AppState::Debrief))),
            )
            .add_systems(EguiPrimaryContextPass, menu_ui.run_if(in_state(AppState::Menu)));
    }
}

/// Theaters in the original's order, then the remake's own (file prefix, display name).
pub const THEATERS: [(&str, &str); 10] = [
    ("lb", "Libya (1986)"),
    ("pg", "Persian Gulf (1984)"),
    ("nc", "North Cape"),
    ("ce", "Central Europe"),
    ("vn", "Vietnam"),
    ("me", "Middle East"),
    ("ku", "Desert Storm"),
    ("cu", "Cuba"),
    ("ko", "Korea"),
    ("ua", "Ukraine (2026)"),
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
    /// Coverage of the main cloud layer; None: rolled.
    pub cloud_cover: Option<u8>,
    pub seed: u16,
    pub mission: Option<Result<Mission, String>>,
    /// The route to fly (mission planner); reset with every new mission.
    pub route: Option<crate::mission::PlannedRoute>,
    /// The stores to carry (loadout editor); reset with every new mission and aircraft.
    pub loadout: Option<crate::loadout::PlannedLoadout>,
    /// Career mode: the pilot (file key) whose tour sortie this is.
    pub career: Option<String>,
    /// Takeoff hour (0-23); None: rolled with the mission.
    pub start_hour: Option<u8>,
    /// The mission screen (single mission or career sortie) instead of the main menu.
    pub mission_screen: bool,
    /// Multiplayer: the player's name, the last server joined ("host[:port]"), and how this
    /// game serves (port, players).
    pub player_name: String,
    pub server_addr: String,
    pub host_port: u16,
    pub max_players: u8,
    dirty: bool,
}

impl MenuState {
    /// Career mode: the next sortie of the pilot's tour.
    pub fn start_career(&mut self, p: &crate::career::Pilot, t: &crate::career::Tour) {
        self.career = Some(p.key());
        (self.theater, self.roe, self.opponents, self.lockheed) =
            (t.theater, t.roe, t.opponents, t.lockheed);
        self.seed = rand::random();
        self.dirty = true;
    }

    /// Realistic weapons: only the F-117A has the real aircraft's limits.
    pub fn realistic(&self, aircraft: AircraftKind) -> bool {
        self.lockheed && aircraft == AircraftKind::F117
    }

    /// Generate a new mission next frame.
    pub fn regenerate(&mut self) {
        self.dirty = true;
    }
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
            cloud_cover: None,
            seed: rand::random(),
            mission: None,
            route: None,
            loadout: None,
            career: None,
            start_hour: Some(8),
            // `--open-mission`: start in the mission screen (screenshots), also where
            // `--open-planner` and `--open-loadout` return to.
            mission_screen: std::env::args()
                .any(|a| a == "--open-mission" || a == "--open-planner" || a == "--open-loadout"),
            player_name: String::new(),
            server_addr: String::new(),
            host_port: stealth_net::DEFAULT_PORT,
            max_players: 8,
            dirty: true,
        }
    }
}

#[derive(Component, Clone)]
struct MenuCamera;

/// An OpenXR runtime is there, so "Fly in VR" can relaunch the game with `--vr`.
#[derive(Resource)]
pub struct VrAvailable(pub bool);

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

/// Flyable aircraft and their notes.
const AIRCRAFT: [(AircraftKind, &[&str]); 2] = [
    (
        AircraftKind::F117,
        &["Stealth attack aircraft", "Subsonic, 7 g", "Modern and classic cockpit"],
    ),
    (
        AircraftKind::F16,
        &["Multirole fighter", "Mach 2 class, 9 g, quick roll", "F-16 cockpit with RWR"],
    ),
];

/// Starts the game again in VR, straight into the menu's mission (OpenXR has to replace the
/// renderer at startup, so this cannot happen in the running process).
fn relaunch_in_vr(state: &MenuState, settings: &crate::settings::Settings) -> std::io::Result<()> {
    // From an AppImage, the AppImage itself: the mounted copy goes away when this process exits.
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) => appimage.into(),
        None => std::env::current_exe()?,
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args([
        "--vr",
        "--mission",
        &state.seed.to_string(),
        "--theater-index",
        &state.theater.to_string(),
    ])
    .args(["--roe", &state.roe.to_string(), "--kind", &state.kind.id().to_string()])
    .args(["--opponents", &state.opponents.to_string()])
    .args(["--weather", state.weather.map_or("random", Sky::name)]);
    if let Some(c) = state.cloud_cover {
        cmd.args(["--cloud-cover", &c.to_string()]);
    }
    // Career sorties start at the rolled time.
    if let Some(h) = state.start_hour.filter(|_| state.career.is_none()) {
        cmd.args(["--start-hour", &h.to_string()]);
    }
    if state.realistic(settings.aircraft) {
        cmd.arg("--lockheed");
    }
    settings_args(&mut cmd, settings);
    if let Some(key) = &state.career {
        cmd.args(["--career", key]);
    }
    if let Some(lo) = &state.loadout {
        match crate::prefs::save_json(crate::loadout::LOADOUT_FILE, lo) {
            Ok(path) => {
                cmd.arg("--loadout").arg(path);
            }
            Err(e) => warn!("saving the loadout: {e}"),
        }
    }
    if let Some(route) = &state.route {
        match crate::prefs::save_json(crate::mission::ROUTE_FILE, route) {
            Ok(path) => {
                cmd.arg("--route").arg(path);
            }
            Err(e) => warn!("saving the route: {e}"),
        }
    }
    cmd.spawn().map(|_| ())
}

/// Starts the game again in VR, straight into a saved campaign.
pub fn relaunch_campaign_in_vr(
    name: &str,
    settings: &crate::settings::Settings,
) -> std::io::Result<()> {
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) => appimage.into(),
        None => std::env::current_exe()?,
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["--vr", "--campaign", name]);
    settings_args(&mut cmd, settings);
    cmd.spawn().map(|_| ())
}

/// The settings a relaunched game takes over.
fn settings_args(cmd: &mut std::process::Command, settings: &crate::settings::Settings) {
    for (on, flag) in [
        (settings.modern_shading, "--modern"),
        (settings.shadows, "--shadows"),
        (settings.local_lights, "--local-lights"),
        (settings.continuous_time, "--continuous-time"),
        (settings.classic_cockpit, "--classic-cockpit"),
        (settings.classic_hud, "--classic-hud"),
        (settings.classic_avionics, "--classic-avionics"),
        (settings.classic_flight, "--classic-flight"),
        (settings.no_crashes, "--no-crashes"),
        (settings.realistic_damage, "--realistic-damage"),
        (settings.realistic_guidance, "--realistic-guidance"),
        (settings.tacview, "--tacview"),
        (settings.air_defence, "--air-defence"),
    ] {
        if on {
            cmd.arg(flag);
        }
    }
    cmd.args(["--vr-cockpit-scale", &format!("{:.0}", settings.vr_cockpit_scale * 100.0)]);
    cmd.args(["--aircraft", settings.aircraft.id()]);
}

fn spawn_menu_camera(
    mut commands: Commands,
    settings: Res<crate::settings::Settings>,
    root: Option<Res<crate::camera::XrRoot>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // In the headset: the menu on a panel in front of the player.
    #[cfg(feature = "xr")]
    if settings.vr {
        let root = root.and_then(|r| r.0);
        crate::vr_menu::spawn(
            &mut commands,
            &mut images,
            &mut meshes,
            &mut materials,
            root,
            MenuCamera,
        );
        return;
    }
    let _ = (&settings, &root, &mut images, &mut meshes, &mut materials);
    commands.spawn((
        Camera2d,
        Camera { clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        MenuCamera,
        PrimaryEguiContext,
    ));
}

fn despawn_menu_camera(mut commands: Commands, q: Query<Entity, With<MenuCamera>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// 80s military terminal: green phosphor on black.
pub(crate) const BG: egui::Color32 = egui::Color32::from_rgb(4, 12, 6);
const FIELD: egui::Color32 = egui::Color32::from_rgb(8, 28, 13);
const HOVER: egui::Color32 = egui::Color32::from_rgb(18, 70, 32);
pub(crate) const GREEN: egui::Color32 = egui::Color32::from_rgb(64, 255, 110);
pub(crate) const DIM: egui::Color32 = egui::Color32::from_rgb(30, 130, 60);
pub(crate) const BRIGHT: egui::Color32 = egui::Color32::from_rgb(190, 255, 205);
pub(crate) const AMBER: egui::Color32 = egui::Color32::from_rgb(255, 176, 0);

/// Hover facts per theater ([`THEATERS`] order).
const THEATER_FACTS: [&[&str]; 10] = [
    &[
        "Gulf of Sidra and the Libyan coast",
        "Era of Operation El Dorado Canyon",
        "Clear desert skies most days",
    ],
    &[
        "Iran-Iraq Tanker War era",
        "Over water and coastal desert",
        "Clearest weather of all theaters",
    ],
    &[
        "Arctic Norway and the Kola Peninsula",
        "Home of the Soviet Northern Fleet",
        "Frequent low overcast",
    ],
    &["NATO vs Warsaw Pact over Germany", "Clear skies only 15% of the time"],
    &[
        "Hanoi and Haiphong region",
        "Jungle and mountain terrain",
        "Thunderstorms in 30% of missions",
    ],
    &["Eastern Mediterranean and the Levant", "Mostly clear skies"],
    &["Kuwait and Iraq, 1991", "The F-117A's real combat debut", "Mostly clear desert skies"],
    &["Caribbean island theater", "Sea and land targets", "Tropical: storms in 15% of missions"],
    &[
        "Korean peninsula north of the DMZ",
        "Mountainous terrain",
        "Changeable weather, any sky is likely",
    ],
    &[
        "Eastern Ukraine, Crimea and southern Russia",
        "Front line from Kharkiv to Kherson burns",
        "Open farmland, ruined towns at the front",
    ],
];
const ROE: [(&str, &[&str]); 3] = [
    (
        "Cold War",
        &[
            "Covert peacetime operation",
            "Staying undetected matters most",
            "Fewer enemy aircraft on patrol",
            "Exact rules of engagement are in the briefing",
        ],
    ),
    (
        "Limited War",
        &[
            "Regional shooting conflict",
            "Enemy air defenses on alert",
            "More enemy aircraft than in Cold War",
        ],
    ),
    (
        "Conventional War",
        &["Full-scale war", "All enemy forces active", "More enemy aircraft than in Cold War"],
    ),
];
const KINDS: [(MissionKind, &str, &[&str]); 5] = [
    (
        MissionKind::Strike,
        "Strike",
        &[
            "Destroy, photograph or reach a ground target",
            "Primary and secondary objective",
            "Air-to-ground loadout",
        ],
    ),
    (
        MissionKind::Air,
        "Air to air",
        &[
            "Intercept an enemy aircraft",
            "Transports, bombers or fighter patrols",
            "Air-to-air missile loadout",
        ],
    ),
    (
        MissionKind::StrikeTraining,
        "Strike training",
        &["Practice strike mission", "Learn weapons and navigation"],
    ),
    (
        MissionKind::AirTraining,
        "Air training",
        &["Practice air-to-air mission", "Learn intercepts and missiles"],
    ),
    (
        MissionKind::DroneDefense,
        "Drone defense",
        &[
            "Ukraine only",
            "Stop a wave of Geran attack drones before it hits a city or a nuclear plant",
            "Then bomb their launch site",
        ],
    ),
];

/// Theaters with drone launch sites (drone defense missions).
const DRONE_THEATERS: [&str; 1] = ["ua"];
const OPPONENTS: [(&str, &[&str]); 4] = [
    ("Green", &["Inexperienced enemy crews", "Easiest setting"]),
    ("Regular", &["Average enemy crews", "Default setting"]),
    ("Veteran", &["Skilled enemy crews", "More ships and patrols"]),
    ("Elite", &["Best enemy pilots and SAM crews", "Hardest setting"]),
];
const RANDOM_SKY_FACTS: &[&str] = &[
    "Rolled from the theater's climate",
    "Changes with each new mission",
    "Clouds block line of sight for both sides",
];

fn sky_facts(sky: Sky) -> &'static [&'static str] {
    match sky {
        Sky::Clear => &["No clouds", "Visibility 40 km", "The original game's sky"],
        Sky::Scattered => &[
            "Cumulus 20-45%, base 2500-6000 ft",
            "Sometimes a thin altostratus high above",
            "Visibility 30 km",
        ],
        Sky::Broken => &[
            "Cumulus 45-70%, base 2000-5000 ft",
            "Often a stratocumulus layer above",
            "Visibility 25 km",
        ],
        Sky::Overcast => &[
            "Stratus 80-100%, base 1500-5000 ft",
            "A few cumulus fragments below",
            "Visibility 20 km",
        ],
        Sky::Storm => &[
            "Cumulonimbus towers up to 40000 ft",
            "Low stratus and cumulus from 1000 ft",
            "Visibility 8 km",
        ],
    }
}

/// `--ui-zoom f`: interface scale (screenshots in small windows).
pub(crate) fn apply_ui_zoom(ctx: &egui::Context) {
    if let Some(z) = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|w| w[0] == "--ui-zoom")
        .and_then(|w| w[1].parse::<f32>().ok())
        && ctx.zoom_factor() != z
    {
        ctx.set_zoom_factor(z);
    }
}

pub(crate) fn apply_terminal_style(style: &mut egui::Style) {
    use egui::{CornerRadius, FontFamily::Monospace, FontId, Shadow, Stroke, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(26.0, Monospace)),
        (TextStyle::Body, FontId::new(15.0, Monospace)),
        (TextStyle::Button, FontId::new(15.0, Monospace)),
        (TextStyle::Small, FontId::new(12.0, Monospace)),
        (TextStyle::Monospace, FontId::new(15.0, Monospace)),
    ]
    .into();
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.window_fill = BG;
    v.panel_fill = BG;
    v.extreme_bg_color = egui::Color32::BLACK;
    v.faint_bg_color = FIELD;
    v.window_stroke = Stroke::new(2.0, GREEN);
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.selection.bg_fill = DIM;
    v.selection.stroke = Stroke::new(1.0, BRIGHT);
    for (w, bg, fg, edge) in [
        (&mut v.widgets.noninteractive, BG, GREEN, DIM),
        (&mut v.widgets.inactive, FIELD, GREEN, DIM),
        (&mut v.widgets.hovered, HOVER, BRIGHT, BRIGHT),
        (&mut v.widgets.active, DIM, BRIGHT, BRIGHT),
        (&mut v.widgets.open, FIELD, GREEN, DIM),
    ] {
        w.bg_fill = bg;
        w.weak_bg_fill = bg;
        w.fg_stroke = Stroke::new(1.5, fg);
        w.bg_stroke = Stroke::new(1.0, edge);
        w.corner_radius = CornerRadius::ZERO;
        w.expansion = 0.0;
    }
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.icon_width = 18.0;
    style.spacing.icon_width_inner = 10.0;
}

/// Hover text: a short list of facts.
pub(crate) fn facts(resp: egui::Response, facts: &[&str]) -> egui::Response {
    resp.on_hover_ui(|ui| {
        ui.set_max_width(380.0);
        for f in facts {
            ui.label(egui::RichText::new(format!("> {f}")).size(14.0));
        }
    })
}

pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("[ {} ]", title.to_ascii_uppercase())).color(AMBER).size(14.0),
    );
}

/// A labelled CLASSIC / MODERN slide switch; `modern` is the right position.
fn slide_switch(ui: &mut egui::Ui, label: &str, modern: &mut bool) -> egui::Response {
    two_way_switch(ui, label, modern, ("CLASSIC", "MODERN"))
}

/// A labelled slide switch between two settings (`sides`: left, right); `right` is the right
/// position.
fn two_way_switch(
    ui: &mut egui::Ui,
    label: &str,
    modern: &mut bool,
    sides: (&str, &str),
) -> egui::Response {
    let width = ui.available_width().min(200.0);
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::click());
    if resp.clicked() {
        *modern = !*modern;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(resp.id, *modern);
        let hovered = resp.hovered();
        let p = ui.painter();
        let font = egui::FontId::monospace(12.0);
        p.text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label.to_ascii_uppercase(),
            font.clone(),
            if hovered { BRIGHT } else { GREEN },
        );
        // The track: two halves, the knob slides over the selected one.
        let track = egui::Rect::from_min_max(
            egui::pos2(rect.right() - 108.0, rect.top()),
            rect.right_bottom(),
        );
        p.rect_filled(track, 0.0, FIELD);
        p.rect_stroke(
            track,
            0.0,
            egui::Stroke::new(1.0, if hovered { BRIGHT } else { DIM }),
            egui::StrokeKind::Inside,
        );
        let half = track.width() / 2.0;
        let knob = egui::Rect::from_min_size(
            egui::pos2(track.left() + t * half, track.top()),
            egui::vec2(half, track.height()),
        )
        .shrink(2.0);
        p.rect_filled(knob, 0.0, if *modern { DIM } else { AMBER.gamma_multiply(0.55) });
        for (i, (text, on)) in [(sides.0, !*modern), (sides.1, *modern)].into_iter().enumerate() {
            let center = egui::pos2(track.left() + half * (i as f32 + 0.5), track.center().y);
            p.text(
                center,
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::monospace(11.0),
                if on { BRIGHT } else { DIM },
            );
        }
    }
    resp
}

pub(crate) fn big_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).size(22.0)).min_size(egui::vec2(170.0, 50.0))
}

type SettingsMut<'a> = ResMut<'a, crate::settings::Settings>;

pub(crate) fn theater_options(ui: &mut egui::Ui, theater: &mut usize) {
    section(ui, "Theater");
    for (i, (_, name)) in THEATERS.iter().enumerate() {
        facts(ui.radio_value(theater, i, *name), THEATER_FACTS[i]);
    }
}

pub(crate) fn opponent_options(ui: &mut egui::Ui, opponents: &mut u8) {
    section(ui, "Opponents");
    for (i, (name, f)) in OPPONENTS.iter().enumerate() {
        facts(ui.radio_value(opponents, i as u8, *name), f);
    }
}

pub(crate) fn situation_options(ui: &mut egui::Ui, roe: &mut u8) {
    section(ui, "Situation");
    for (i, (name, f)) in ROE.iter().enumerate() {
        facts(ui.radio_value(roe, i as u8, *name), f);
    }
}

fn mission_options(ui: &mut egui::Ui, state: &mut MenuState) {
    section(ui, "Mission");
    let drones = DRONE_THEATERS.contains(&THEATERS[state.theater].0);
    if !drones && state.kind == MissionKind::DroneDefense {
        state.kind = MissionKind::Air;
    }
    for (kind, name, f) in KINDS {
        if kind != MissionKind::DroneDefense || drones {
            facts(ui.radio_value(&mut state.kind, kind, name), f);
        }
    }
}

fn start_time_options(ui: &mut egui::Ui, state: &mut MenuState) {
    section(ui, "Start time");
    let mut random = state.start_hour.is_none();
    let r = facts(
        ui.checkbox(&mut random, "Random"),
        &[
            "Rolled with each new mission, as in the original",
            "06:00-08:00 by day or 18:00-20:00 by night",
            "Uncheck to choose the hour",
        ],
    );
    if r.changed() {
        state.start_hour = if random { None } else { Some(8) };
    }
    if let Some(h) = state.start_hour.as_mut() {
        let slider = egui::Slider::new(h, 0..=23).custom_formatter(|v, _| format!("{v:02.0}:00"));
        facts(
            ui.add(slider),
            &["Takeoff time", "Night from 18:00 to 06:00", "Kept by NEW MISSION"],
        );
    }
}

/// The aircraft, with the switches that go with it; `lockheed` shows "Realistic weapons".
pub(crate) fn aircraft_options(
    ui: &mut egui::Ui,
    settings: &mut SettingsMut,
    lockheed: Option<&mut bool>,
) {
    section(ui, "Aircraft");
    let s = settings.bypass_change_detection();
    let mut picked = false;
    for (kind, f) in AIRCRAFT {
        picked |= facts(ui.radio_value(&mut s.aircraft, kind, kind.name()), f).changed();
    }
    if picked {
        settings.set_changed();
    }
    // The F-16 carries AIM-9s and AIM-120s on its pylons anyway.
    if settings.aircraft == AircraftKind::F117
        && let Some(lockheed) = lockheed
    {
        facts(
            ui.checkbox(lockheed, "Realistic weapons"),
            &[
                "The real aircraft's limits",
                "2 weapon bays instead of 4",
                "No AIM-9 or AIM-120 missiles",
                "No carrier takeoffs or landings",
            ],
        );
    }
    let s = settings.bypass_change_detection();
    if facts(
        ui.checkbox(&mut s.no_crashes, "Crash protection"),
        &[
            "Pulls up automatically near the ground (gear up)",
            "Ground contact and collisions are not fatal",
            "Off: the ground can always be hit",
        ],
    )
    .changed()
    {
        settings.set_changed();
    }
    let s = settings.bypass_change_detection();
    if facts(
        ui.checkbox(&mut s.tacview, "Tacview export"),
        &[
            "Saves each flight as a Tacview recording (.acmi)",
            "In the game's data folder, under tacview",
        ],
    )
    .changed()
    {
        settings.set_changed();
    }
}

pub(crate) fn realism_options(ui: &mut egui::Ui, settings: &mut SettingsMut) {
    section(ui, "Realism");
    let s = settings.bypass_change_detection();
    if facts(
        two_way_switch(ui, "Damage", &mut s.realistic_damage, ("GAME", "REAL")),
        &[
            "Weapon damage", "Game: the original's damage; hits add up but never bring the aircraft down",
            "Realistic: a missile hit can destroy the aircraft (eject!), damages more systems and often starts a fire, which burns out or blows the aircraft up",
            "Realistic: damaged control surfaces put the aircraft out of trim or make it oscillate",
        ],
    )
    .changed()
    {
        settings.set_changed();
    }
    let s = settings.bypass_change_detection();
    if facts(
        two_way_switch(ui, "Guidance", &mut s.realistic_guidance, ("CLASSIC", "REAL")),
        &[
            "Missile guidance", "Classic: the original's missiles, flying straight at their target",
            "Realistic: proportional navigation to the collision point; motors burn out and the missiles glide on, losing speed with every turn (smoke only while the motor burns)",
            "Realistic: SAMs loft against distant targets; the AIM-120 flies straight for 3 s (pull up to lob it)",
        ],
    )
    .changed()
    {
        settings.set_changed();
    }
    let s = settings.bypass_change_detection();
    if facts(
        two_way_switch(ui, "Low level", &mut s.air_defence, ("CLASSIC", "REAL")),
        &[
            "Low-level air defences", "Classic: only the original's SAM sites and fighters",
            "Realistic: radar-laid ZSU-23-4 guns (their radar sees you like a SAM's) and S-60 flak batteries guard the airfields and the targets; MANPADS teams in the towns fire SA-14s at what they see",
            "Flying low over defended places becomes dangerous; bombs and guns destroy the guns",
        ],
    )
    .changed()
    {
        settings.set_changed();
    }
}

pub(crate) fn weather_options(
    ui: &mut egui::Ui,
    weather: &mut Option<Sky>,
    cloud_cover: &mut Option<u8>,
) {
    section(ui, "Weather");
    facts(ui.radio_value(weather, None, "Random"), RANDOM_SKY_FACTS);
    for sky in Sky::ALL {
        facts(ui.radio_value(weather, Some(sky), sky.name()), sky_facts(sky));
    }
    // Coverage of the main layer (cumulus, or stratus for overcast and storms).
    let mut auto = cloud_cover.is_none();
    let r = facts(
        ui.checkbox(&mut auto, "Random cover"),
        &[
            "Main cloud layer cover rolled with the weather",
            "Uncheck to set it yourself",
            "Main layer: stratus for overcast and storm, cumulus otherwise",
        ],
    );
    if r.changed() {
        *cloud_cover = if auto { None } else { Some(50) };
    }
    if let Some(c) = cloud_cover.as_mut() {
        facts(
            ui.add(egui::Slider::new(c, 0..=100).suffix("%")),
            &["Share of the sky the main layer covers", "No effect on a clear sky"],
        );
    }
}

/// The classic / modern switches and the VR cockpit size.
pub(crate) fn classic_modern_options(ui: &mut egui::Ui, settings: &mut SettingsMut, vr: bool) {
    section(ui, "Classic / Modern");
    // Only mark the settings changed when a switch is flipped.
    let mut toggled = false;
    let s = settings.bypass_change_detection();
    let mut cockpit = !s.classic_cockpit;
    let mut hud = !s.classic_hud;
    let mut avionics = !s.classic_avionics;
    let mut flight = !s.classic_flight;
    for (modern, name, f) in [
        (
            &mut s.modern_shading,
            "Shaders",
            &[
                "Modern: MSAA anti-aliasing, subtle directional shading, smooth distance haze",
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
            "Lighting",
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
        (
            &mut cockpit,
            "Cockpit",
            &[
                "Modern: high-resolution displays with bezel buttons",
                "Classic: the original instrument panel layout and 77x60 displays",
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
    ] {
        toggled |= facts(slide_switch(ui, name, modern), f).changed();
    }
    s.classic_cockpit = !cockpit;
    s.classic_hud = !hud;
    s.classic_avionics = !avionics;
    s.classic_flight = !flight;
    section(ui, "VR");
    // Read every frame by the rig: no change detection needed.
    let range = crate::settings::VR_COCKPIT_SCALE_RANGE;
    let mut pct = (s.vr_cockpit_scale * 100.0).round() as u32;
    let slider = egui::Slider::new(
        &mut pct,
        (*range.start() * 100.0) as u32..=(*range.end() * 100.0) as u32,
    )
    .suffix("%")
    .text("cockpit scale");
    if facts(
        ui.add_enabled(vr || s.vr, slider),
        &[
            "Size of the cockpit in the headset",
            "100% (default): the size that suits the aircraft",
            "Smaller puts more of the panel in view",
        ],
    )
    .changed()
    {
        s.vr_cockpit_scale = pct as f32 / 100.0;
    }
    if toggled {
        settings.set_changed();
    }
}

/// A full-screen-width menu window in the terminal style.
pub(crate) fn menu_window(ctx: &egui::Context, id: &str, width: f32) -> egui::Window<'static> {
    egui::Window::new(id.to_string())
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .min_width(width)
        .max_width(width)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(16.0))
}

/// Width of the menu's wide windows.
pub(crate) fn wide_width(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().width() - 32.0).clamp(600.0, 1400.0)
}

/// The top level: what to play.
fn main_screen(
    ctx: &egui::Context,
    state: &mut MenuState,
    dialog: &mut crate::input_dialog::InputDialog,
    career: &mut crate::career::CareerWindow,
    campaign: &mut crate::campaign::CampaignWindow,
    multiplayer: &mut crate::multiplayer_menu::MultiplayerWindow,
    exit: &mut MessageWriter<AppExit>,
) {
    let wide = |text: &str| big_button(text).min_size(egui::vec2(360.0, 50.0));
    menu_window(ctx, "Stealth Mission 89", 420.0).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("STEALTH MISSION 89").heading().color(BRIGHT));
            ui.label(egui::RichText::new("MISSION PLANNING SYSTEM").color(DIM));
            ui.add_space(10.0);
            if facts(
                ui.add(wide("SINGLE MISSION")),
                &[
                    "One mission in the theater of your choice",
                    "Choose the options, plan the route and the loadout",
                ],
            )
            .clicked()
            {
                // Leaves career mode; the same mission again if the options are the same.
                state.career = None;
                state.dirty = true;
                state.mission_screen = true;
            }
            if facts(
                ui.add(wide("CAREER")),
                &[
                    "Pilot records: rank, decorations, sorties",
                    "Tour of duty: 12 sorties in one theater",
                    "What you destroy stays destroyed",
                ],
            )
            .clicked()
            {
                career.show();
            }
            if facts(
                ui.add(wide("DYNAMIC CAMPAIGN")),
                &[
                    "Dynamic campaign in real time",
                    "Take the enemy's cities and airfields",
                    "Missions from the live situation",
                ],
            )
            .clicked()
            {
                campaign.show();
            }
            if facts(
                ui.add(wide("MULTIPLAYER")),
                &[
                    "Coop in the dynamic campaign",
                    "Join a server, or host a campaign and fly in it",
                ],
            )
            .clicked()
            {
                multiplayer.show();
            }
            if facts(
                ui.add(wide("CONTROLS")),
                &["Map keys, joystick and HOTAS axes and buttons", "Saved for the next flights"],
            )
            .clicked()
            {
                dialog.open = true;
            }
            ui.add_space(10.0);
            if facts(ui.add(wide("QUIT")), &["Exit to the desktop"]).clicked() {
                exit.write(AppExit::Success);
            }
        });
    });
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
    mut dialog: ResMut<crate::input_dialog::InputDialog>,
    mut input_config: ResMut<crate::hotas::InputConfig>,
    hotas: Res<crate::hotas::Hotas>,
    keys: Res<ButtonInput<KeyCode>>,
    mut saved: Local<Option<crate::prefs::Prefs>>,
    windows: (
        ResMut<crate::planner::Planner>,
        ResMut<crate::career::CareerWindow>,
        ResMut<crate::loadout::LoadoutWindow>,
        ResMut<crate::campaign::CampaignWindow>,
        ResMut<crate::multiplayer_menu::MultiplayerWindow>,
    ),
) -> Result {
    let (mut planner, mut career, mut loadout, mut campaign, mut multiplayer) = windows;
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
        let pilot = state.career.as_deref().and_then(crate::career::Pilot::load);
        if let Some(p) = pilot {
            // Career: the tour's next sortie, on targets that still stand.
            match crate::career::next_sortie(&data, &p, &state, settings.aircraft) {
                Some((seed, m)) => {
                    state.seed = seed;
                    state.kind = m.settings.kind;
                    state.mission = Some(Ok(m));
                }
                None => state.mission = Some(Err("No targets left standing for this tour".into())),
            }
        } else {
            state.career = None;
            let mut settings = crate::mission::settings(
                state.theater,
                state.roe,
                state.kind,
                state.opponents,
                state.realistic(settings.aircraft),
                state.weather,
                state.cloud_cover,
            );
            settings.start_hour = state.start_hour;
            state.mission = Some(
                crate::mission::generate(&data, &settings, state.seed)
                    .map_err(|e| format!("{e:#}")),
            );
        }
        state.route = state
            .mission
            .as_ref()
            .and_then(|m| m.as_ref().ok())
            .map(crate::mission::PlannedRoute::of);
    }
    // The briefed stores, on the aircraft flown.
    if let Some(Ok(m)) = &state.mission
        && !state.loadout.as_ref().is_some_and(|l| l.fits(m, settings.aircraft))
    {
        state.loadout = Some(crate::loadout::PlannedLoadout::of(m, settings.aircraft));
    }

    let ctx = contexts.ctx_mut()?;
    apply_ui_zoom(ctx);
    // Both themes, so the system light/dark preference does not matter.
    if ctx.global_style().visuals.window_fill != BG {
        ctx.all_styles_mut(apply_terminal_style);
    }
    let before = (
        state.theater,
        state.roe,
        state.kind,
        state.opponents,
        state.lockheed,
        state.weather,
        state.cloud_cover,
        state.start_hour,
    );
    let realistic_before = state.realistic(settings.aircraft);
    // One window at a time: the open dialog replaces the screen it was opened from.
    let dialogs = dialog.open || career.open || campaign.open || multiplayer.open;
    if !state.mission_screen && !dialogs {
        main_screen(
            ctx,
            &mut state,
            &mut dialog,
            &mut career,
            &mut campaign,
            &mut multiplayer,
            &mut exit,
        );
    }
    if state.mission_screen && !dialogs && !planner.open && !loadout.open {
        let width = wide_width(ctx);
        // Career sorties take their options from the career window.
        let career_mode = state.career.is_some();
        let mut back = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        menu_window(ctx, "Single mission", width).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(if career_mode { "CAREER SORTIE" } else { "SINGLE MISSION" }).heading().color(BRIGHT));
                if let Some(p) = state.career.as_deref().and_then(crate::career::Pilot::load)
                    && let Some(t) = &p.tour
                {
                    ui.label(egui::RichText::new(format!("  {}  //  TOUR SORTIE {} OF {}", p.title().to_ascii_uppercase(), t.sortie + 1, t.length)).color(AMBER));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("MISSION PLANNING SYSTEM  //  MSN {:05}", state.seed)).color(DIM));
                });
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                if !career_mode {
                    // Options: three narrow columns.
                    ui.vertical(|ui| {
                        ui.set_width(210.0);
                        theater_options(ui, &mut state.theater);
                        opponent_options(ui, &mut state.opponents);
                        start_time_options(ui, &mut state);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        situation_options(ui, &mut state.roe);
                        mission_options(ui, &mut state);
                        aircraft_options(ui, &mut settings, Some(&mut state.lockheed));
                        realism_options(ui, &mut settings);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        let state = &mut *state;
                        weather_options(ui, &mut state.weather, &mut state.cloud_cover);
                        classic_modern_options(ui, &mut settings, vr.0);
                    });
                }
                // Mission summary in big type, the full briefing below in small type.
                // What is left beside the option columns, less the frame margins and border.
                let summary_width = (ui.available_width() - 22.0).max(300.0);
                egui::Frame::new().stroke(egui::Stroke::new(1.0, DIM)).inner_margin(10.0).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(summary_width);
                        match &state.mission {
                            Some(Ok(m)) => {
                                mission_summary(ui, &state, m);
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    let plan = ui.button(egui::RichText::new("PLAN ROUTE").size(16.0));
                                    if facts(plan, &["The theater map with the enemy radar and SAM coverage", "Add, move and remove navigation points", "Targets and the landing base stay fixed"]).clicked() {
                                        planner.open = true;
                                    }
                                    let arm = ui.button(egui::RichText::new("LOADOUT").size(16.0));
                                    if facts(arm, &["Choose the stores on each bay or pylon", "Starts with the briefed weapons"]).clicked() {
                                        loadout.open = true;
                                    }
                                });
                            }
                            Some(Err(e)) => {
                                ui.colored_label(AMBER, e);
                            }
                            None => {
                                ui.label("GENERATING...");
                            }
                        }
                    });
                });
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let to = if career_mode { "Back to the pilot records" } else { "Back to the main menu" };
                if facts(ui.add(big_button("BACK")), &[to]).clicked() {
                    back = true;
                }
                let new = if career_mode { "NEW SORTIE" } else { "NEW MISSION" };
                if facts(ui.add(big_button(new)), &["Rolls different objectives", "Keeps all the options and the start time"]).clicked() {
                    state.seed = rand::random();
                    state.dirty = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_fly = matches!(state.mission, Some(Ok(_)));
                    let fly = ui.add_enabled(can_fly, big_button("FLY").fill(DIM));
                    if facts(fly, &["Take off on the briefed mission"]).clicked()
                        && let Some(Ok(m)) = &state.mission
                    {
                        *setup = crate::mission::setup_from_mission(m, &tables.start_dir);
                        commands.insert_resource(ActiveMission(m.clone()));
                        commands.insert_resource(state.route.clone().unwrap_or_else(|| crate::mission::PlannedRoute::of(m)));
                        commands.insert_resource(state.loadout.clone().unwrap_or_else(|| crate::loadout::PlannedLoadout::of(m, settings.aircraft)));
                        match &state.career {
                            Some(key) => commands.insert_resource(crate::career::CareerFlight(key.clone())),
                            None => commands.remove_resource::<crate::career::CareerFlight>(),
                        }
                        next.set(AppState::Flight);
                    }
                    // Already in VR: "Fly" flies in the headset.
                    if !settings.vr {
                        let vr_button = ui
                            .add_enabled(can_fly && vr.0, big_button("FLY IN VR"))
                            .on_disabled_hover_text(if vr.0 { "Generate a mission first" } else { "No OpenXR runtime (Monado, WiVRn, SteamVR) is active" });
                        if facts(vr_button, &["Restarts the game in the VR headset", "Flies this exact mission"]).clicked() {
                            match relaunch_in_vr(&state, &settings) {
                                Ok(()) => {
                                    exit.write(AppExit::Success);
                                }
                                Err(e) => state.mission = Some(Err(format!("could not start in VR: {e}"))),
                            }
                        }
                    }
                });
            });
        });
        if back {
            state.mission_screen = false;
            if career_mode {
                career.show();
            }
        }
    }
    crate::input_dialog::show(
        ctx,
        &mut dialog,
        input_config.bypass_change_detection(),
        &hotas,
        &keys,
    );
    crate::planner::planner_ui(ctx, &mut planner, &mut state);
    crate::loadout::loadout_ui(ctx, &mut loadout, &mut state, settings.aircraft);
    if before
        != (
            state.theater,
            state.roe,
            state.kind,
            state.opponents,
            state.lockheed,
            state.weather,
            state.cloud_cover,
            state.start_hour,
        )
    {
        // Changing the mission's options leaves career mode.
        state.dirty = true;
        state.career = None;
    } else if realistic_before != state.realistic(settings.aircraft) {
        // Another aircraft: the same mission with or without the F-117A's limits.
        state.dirty = true;
    }
    crate::career::career_ui(ctx, &mut career, &mut state, &mut settings, vr.0);
    crate::prefs::Prefs::save_if_changed(&state, &settings, &mut saved);
    Ok(())
}

fn mission_summary(ui: &mut egui::Ui, state: &MenuState, m: &Mission) {
    let b = &m.briefing;
    section(ui, "Mission summary");
    ui.label(egui::RichText::new(clean_text(&b.title)).size(20.0).color(BRIGHT));
    ui.add_space(4.0);
    let w = &m.weather;
    let rows = [
        ("THEATER", THEATERS[state.theater].1.to_string(), GREEN),
        ("SITUATION", ROE[state.roe as usize].0.to_string(), GREEN),
        (
            "MISSION",
            KINDS.iter().find(|k| k.0 == state.kind).map_or("", |k| k.1).to_string(),
            GREEN,
        ),
        ("OPPONENTS", OPPONENTS[state.opponents as usize].0.to_string(), GREEN),
        (
            "TIME",
            format!("{}  dep {}", if m.night { "Night" } else { "Day" }, m.time.depart),
            GREEN,
        ),
        (
            "WEATHER",
            format!("{}  vis {} km", w.sky.name(), w.vis_km),
            if w.sky == Sky::Storm { AMBER } else { GREEN },
        ),
        ("FUEL", format!("{} lbs", m.fuel_lbs), GREEN),
    ];
    egui::Grid::new("summary").num_columns(2).spacing([18.0, 4.0]).show(ui, |ui| {
        for (key, value, color) in rows {
            ui.label(egui::RichText::new(key).size(14.0).color(DIM));
            ui.label(egui::RichText::new(value.to_ascii_uppercase()).size(20.0).color(color));
            ui.end_row();
        }
    });
    section(ui, "Briefing");
    egui::ScrollArea::vertical().max_height(230.0).auto_shrink([false, true]).show(ui, |ui| {
        let small = |ui: &mut egui::Ui, s: &str| {
            if !s.trim().is_empty() {
                ui.label(egui::RichText::new(clean_text(s)).small());
            }
        };
        for l in [
            &b.primary_objective,
            &b.secondary_objective,
            &b.depart_line,
            &b.land_line,
            &b.weather_line,
        ] {
            small(ui, l);
        }
        match &state.loadout {
            Some(lo) => small(ui, &format!("Armament: {}", lo.summary())),
            None => small(
                ui,
                &format!(
                    "Armament: {}",
                    b.armament.iter().map(|a| clean_text(a)).collect::<Vec<_>>().join(", ")
                ),
            ),
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new("INTELLIGENCE").small().color(AMBER));
        for l in [&b.primary_summary, &b.secondary_summary, &b.roe_text] {
            small(ui, l);
        }
    });
}
