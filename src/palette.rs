//! Palettes and colours.
//!
//! - Three flight palettes per theater (day, dusk, night) from the original game; the live
//!   palette steps between them over the mission (one 6-bit unit per channel per step, one
//!   colour range every 16 ticks).
//! - Model colour bytes map through a remap table (by detail level) to colour words; one
//!   `PaletteMaterial` per colour byte (polygons) plus one per colour byte for lines/dots.
//! - Shared shader globals: live palette, sky ring and altitude-dependent ground band offsets.

use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;
use serde::Deserialize;

use crate::palette_material::{FLAG_BACKGROUND, FLAG_LINE, Globals, PaletteMaterial, PaletteParams, pack16};
use crate::settings::{AppState, MissionSetup, SetupSet, Settings};

pub struct PalettePlugin;

impl Plugin for PalettePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PaletteMaterial>::default())
            .init_resource::<TimeOfDay>()
            .init_resource::<Viewpoint>()
            .add_systems(OnEnter(AppState::Flight), setup_palette.in_set(SetupSet::Palette))
            .add_systems(OnEnter(AppState::Viewer), setup_palette.in_set(SetupSet::Palette))
            .init_resource::<DetailLevel>()
            .add_systems(Update, (step_time_of_day, cycle_detail).run_if(resource_exists::<Palette>))
            .add_systems(
                PostUpdate,
                (apply_remap_changes, update_globals.after(crate::lighting::LightingSystems)).run_if(resource_exists::<Palette>),
            )
            .add_systems(PostUpdate, apply_msaa);
    }
}

/// 6-bit VGA DAC values.
#[derive(Debug, Clone, Deserialize)]
pub struct VgaPalette {
    pub colors: Vec<[u8; 3]>,
}

impl VgaPalette {
    pub fn load(name: &str) -> VgaPalette {
        let path = crate::asset_path::asset(&format!("palettes/{name}.ron"));
        let p = path.display();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{p}: {e}"));
        ron::from_str(&text).unwrap_or_else(|e| panic!("{p}: {e}"))
    }

    pub fn color(&self, i: u8) -> Color {
        let [r, g, b] = self.colors[i as usize];
        let s = |v: u8| v as f32 / 63.0;
        Color::srgb(s(r), s(g), s(b))
    }

    /// Moves every channel of colours `first..first+count` one unit towards `target`.
    fn step_towards(&mut self, target: &VgaPalette, first: usize, count: usize) {
        for i in first..(first + count).min(256) {
            for c in 0..3 {
                let (cur, tgt) = (&mut self.colors[i][c], target.colors[i][c]);
                if *cur < tgt {
                    *cur += 1;
                } else if *cur > tgt {
                    *cur -= 1;
                }
            }
        }
    }
}

/// Day/dusk/night palettes of a theater (desert theaters have their ground ramps patched).
#[derive(Debug, Clone, Deserialize)]
pub struct TheaterPalettes {
    pub day: VgaPalette,
    pub dusk: VgaPalette,
    pub night: VgaPalette,
}

/// Model colour remap table: model colour byte -> colour word.
#[derive(Debug, Clone, Deserialize)]
pub struct RemapTable {
    pub name: String,
    pub entries: Vec<u16>,
}

/// Remap tables in assets/palettes/remap.ron order.
pub const REMAP_DETAIL_LOW: usize = 0;
pub const REMAP_DETAIL_HIGH: usize = 1;

#[derive(Resource)]
pub struct Palette {
    /// The live palette.
    pub active: VgaPalette,
    pub theater: TheaterPalettes,
    pub remaps: Vec<RemapTable>,
    /// Index of the active remap table (detail 3 = REMAP_DETAIL_HIGH).
    pub remap: usize,
    /// Polygon material per model colour byte.
    pub materials: Vec<Handle<PaletteMaterial>>,
    /// Line/dot material per model colour byte (always solid, with haze).
    pub line_materials: Vec<Handle<PaletteMaterial>>,
    pub background: Handle<PaletteMaterial>,
    pub background_flir: Handle<PaletteMaterial>,
    pub globals: Handle<ShaderBuffer>,
}

impl Palette {
    pub fn material(&self, c: u8) -> Handle<PaletteMaterial> {
        self.materials[c as usize].clone()
    }

    pub fn line_material(&self, c: u8) -> Handle<PaletteMaterial> {
        self.line_materials[c as usize].clone()
    }

    pub fn color(&self, i: u8) -> Color {
        self.active.color(i)
    }

    pub fn word(&self, c: u8) -> u16 {
        self.remaps.get(self.remap).map_or(0xFF00 | c as u16, |t| t.entries[c as usize])
    }

    /// Lights come on when the sky gets dark (red of colour 136 < 20).
    pub fn lights_on(&self) -> bool {
        self.active.colors[136][0] < 20
    }
}

/// Parses a colour byte from a name containing `pal_NNN` (Blender may append `.001`).
pub fn index_from_name(name: &str) -> Option<u8> {
    let i = name.rfind("pal_")?;
    name.get(i + 4..i + 7)?.parse().ok()
}

/// Mission time of day.
#[derive(Resource)]
pub struct TimeOfDay {
    pub night_mission: bool,
    /// Per-mission seed n (0..15): the palette starts 2n steps along.
    pub seed: u8,
    pub ticks: u32,
    timer: f32,
    /// Alt-N style override: jump straight to the target palette.
    pub frozen: bool,
}

impl Default for TimeOfDay {
    fn default() -> Self {
        Self { night_mission: false, seed: 6, ticks: 0, timer: 0.0, frozen: false }
    }
}

impl TimeOfDay {
    fn start_and_target<'a>(&self, p: &'a TheaterPalettes) -> (&'a VgaPalette, &'a VgaPalette) {
        if self.night_mission { (&p.dusk, &p.night) } else { (&p.night, &p.day) }
    }

    /// The live palette at the start of the mission.
    pub fn initial(&self, p: &TheaterPalettes) -> VgaPalette {
        let (start, target) = self.start_and_target(p);
        let mut cur = start.clone();
        for _ in 0..2 * (self.seed & 15) {
            cur.step_towards(target, 0, 256);
        }
        cur
    }
}

/// The eye position used for the sky/ground gradient (set by the camera rig).
#[derive(Resource, Default)]
pub struct Viewpoint {
    /// Altitude in original world units.
    pub altitude_wu: f32,
}

pub fn load_theater_palettes(theater: &str) -> TheaterPalettes {
    let path = crate::asset_path::asset(&format!("palettes/tod_{theater}.ron"));
    let p = path.display();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{p}: {e}"));
    ron::from_str(&text).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn setup_palette(
    mut commands: Commands,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    setup: Res<MissionSetup>,
    mut tod: ResMut<TimeOfDay>,
) {
    *tod = TimeOfDay { night_mission: setup.night, seed: setup.tod_seed, ..default() };
    let theater = load_theater_palettes(&setup.theater);
    let remap = crate::asset_path::asset("palettes/remap.ron");
    let text = std::fs::read_to_string(&remap).unwrap_or_else(|e| panic!("{}: {e}", remap.display()));
    let remaps: Vec<RemapTable> = ron::from_str(&text).expect("remap.ron");
    let globals = buffers.add(ShaderBuffer::from(Globals::default()));
    let active = tod.initial(&theater);
    let mut palette = Palette {
        active,
        theater,
        remaps,
        remap: REMAP_DETAIL_HIGH,
        materials: vec![],
        line_materials: vec![],
        background: Handle::default(),
        background_flir: Handle::default(),
        globals: globals.clone(),
    };
    let mk = |word: u16, flags: u32| PaletteMaterial {
        params: PaletteParams { word: word as u32, flags, ..default() },
        globals: globals.clone(),
    };
    palette.materials = (0..=255u8).map(|c| materials.add(mk(palette.word(c), 0))).collect();
    palette.line_materials = (0..=255u8).map(|c| materials.add(mk(0xFF00 | (palette.word(c) & 0xFF), FLAG_LINE))).collect();
    palette.background = materials.add(mk(0, FLAG_BACKGROUND));
    palette.background_flir = materials.add(mk(0, FLAG_BACKGROUND | crate::palette_material::FLAG_FLIR));
    commands.insert_resource(palette);
}

fn step_time_of_day(time: Res<Time>, mut tod: ResMut<TimeOfDay>, mut palette: ResMut<Palette>) {
    if tod.frozen {
        return;
    }
    tod.timer += time.delta_secs();
    while tod.timer >= 1.0 {
        tod.timer -= 1.0;
        tod.ticks += 1;
        let t = tod.ticks;
        if t & 15 != 0 {
            continue;
        }
        let target = if tod.night_mission { palette.theater.night.clone() } else { palette.theater.day.clone() };
        let p = &mut palette.active;
        match t & 0x30 {
            0x00 => {
                if t & 0x40 != 0 {
                    p.step_towards(&target, 0x10, 0x40)
                }
            }
            0x10 => p.step_towards(&target, 0x60, 0x40),
            0x20 => p.step_towards(&target, 0xA0, 0x30),
            _ => p.step_towards(&target, 0xD0, 0x30),
        }
    }
}

fn apply_remap_changes(palette: Res<Palette>, mut materials: ResMut<Assets<PaletteMaterial>>) {
    if !palette.is_changed() {
        return;
    }
    for c in 0..=255u8 {
        let word = palette.word(c) as u32;
        if let Some(mut m) = materials.get_mut(&palette.materials[c as usize])
            && m.params.word != word
        {
            m.params.word = word;
        }
        let line = 0xFF00 | (word & 0xFF);
        if let Some(mut m) = materials.get_mut(&palette.line_materials[c as usize])
            && m.params.word != line
        {
            m.params.word = line;
        }
    }
}

/// Faithful mode renders without anti-aliasing like the original; modern mode uses 4x MSAA.
fn apply_msaa(
    settings: Res<Settings>,
    mut cams: Query<&mut Msaa, (With<Camera3d>, Without<crate::trackcam::TrackCamera>)>,
    added: Query<(), Added<Camera3d>>,
) {
    if !settings.is_changed() && added.is_empty() {
        return;
    }
    let want = if settings.modern_shading { Msaa::Sample4 } else { Msaa::Off };
    for mut m in &mut cams {
        if *m != want {
            *m = want;
        }
    }
}

/// Sky band boundaries, original screen units at f = 256.
const SKY_RING: [f32; 16] = [0., 2., 3., 4., 6., 8., 11., 15., 20., 27., 36., 48., 64., 87., 119., 164.];

#[allow(clippy::too_many_arguments)]
fn update_globals(
    palette: Res<Palette>,
    viewpoint: Res<Viewpoint>,
    settings: Res<Settings>,
    lighting: Res<crate::lighting::LightingState>,
    weather: Res<crate::weather::WeatherState>,
    origin: Res<crate::world::FloatingOrigin>,
    hud: Option<Res<crate::hud::HudState>>,
    mfd: Option<Res<crate::mfd::MfdState>>,
    trackcam: Option<Res<crate::trackcam::TrackCamImage>>,
    images: Res<Assets<Image>>,
    time: Res<Time>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let Some(mut buf) = buffers.get_mut(&palette.globals) else { return };
    let mut g = Globals::default();
    for i in 0..256 {
        let c = palette.active.color(i as u8).to_linear();
        g.palette[i] = Vec4::new(c.red, c.green, c.blue, 1.0);
    }
    g.sky_ring = pack16(SKY_RING);
    // Ground bands: g[k] = alt/((16-k)*32) - alt/512 (integer maths as in the original).
    let alt = viewpoint.altitude_wu.max(0.0) as i32;
    let mut ground = [0f32; 16];
    for (k, v) in ground.iter_mut().enumerate().skip(1) {
        *v = (alt / ((16 - k as i32) * 32) - alt / 512) as f32;
    }
    g.ground_g = pack16(ground);
    let flir = mfd.is_some_and(|m| m.flir);
    g.params = Vec4::new(1.0, crate::world::WU as f32, flir as u32 as f32, settings.modern_shading as u32 as f32);
    g.sun = lighting.sun;
    g.ambient = lighting.ambient;
    g.light_info.x = lighting.lights.len() as f32;
    g.light_info.y = lighting.daylight;
    g.light_info.z = hud.is_some_and(|h| h.nvg) as u32 as f32;
    g.dots.x = if settings.vr { crate::dots::XR_ANGLE } else { 0.0 };
    g.dots.y = time.elapsed_secs_wrapped() % 3600.0;
    if let Some(img) = trackcam.and_then(|t| images.get(&t.0)) {
        (g.dots.z, g.dots.w) = (img.width() as f32, img.height() as f32);
    }
    for (i, (p, c)) in lighting.lights.iter().enumerate() {
        g.local_lights[2 * i] = *p;
        g.local_lights[2 * i + 1] = *c;
    }
    (g.clouds, g.weather, g.noise, g.stratus) = weather.globals(origin.origin);
    (g.bolt_a, g.bolt_b) = weather.bolt_light(origin.origin);
    buf.set_data(g);
}

/// Original detail level 0..3 and the model LOD range reduction 0..3. Alt-D first halves the
/// last detail level's range three times, then cycles the original level 3 -> 2 -> 1 -> 0 (at
/// the shortest range), then back to full detail. Below 3 the original uses the simpler remap
/// table (solid terrain colours instead of gradient AND/OR overlays).
#[derive(Resource)]
pub struct DetailLevel(pub u8, pub u8);

impl Default for DetailLevel {
    fn default() -> Self {
        Self(3, 0)
    }
}

impl DetailLevel {
    /// Factor on the range of the models' last detail level.
    pub fn lod_range(&self) -> f32 {
        0.5f32.powi(self.1 as i32)
    }
}

fn cycle_detail(
    mut actions: MessageReader<crate::input::ActionEvent>,
    mut detail: ResMut<DetailLevel>,
    mut palette: ResMut<Palette>,
) {
    for crate::input::ActionEvent(a) in actions.read() {
        if *a == crate::input::Action::Detail {
            *detail = match *detail {
                DetailLevel(3, r) if r < 3 => DetailLevel(3, r + 1),
                DetailLevel(0, _) => DetailLevel(3, 0),
                DetailLevel(l, r) => DetailLevel(l - 1, r),
            };
            palette.remap = if detail.0 == 3 { REMAP_DETAIL_HIGH } else { REMAP_DETAIL_LOW };
        }
    }
}
