//! Optional lighting (menu "Shadows" / "Local lighting", `--shadows` / `--local-lights`).
//!
//! - Sun: from the modern-mode light vector (or the moving sun), strength from the live sky
//!   colour (off at night). It lights the 3D cockpit (StandardMaterial) and, through the shadow
//!   maps, the palette-material world (palette.wgsl `lit`), including the background's ground
//!   gradient (shadows taken on the ground plane).
//! - Moon (continuous time of day, modern shading or shadows): its light by the moon's phase
//!   and height.
//! - Two `DirectionalLight`s carry them: the key light (the brighter of the two, with the
//!   shadow maps) and the fill light (the other one, no shadows), so the moon casts the
//!   shadows at night.
//! - Local lights: `LightEmitter` entities (missile motors, flares, impact flashes, cockpit
//!   lamps) plus nearby runway/city dot lights. The nearest ones go to the world shader
//!   (`Globals.lights`) and a few become Bevy `PointLight`s so they also light the cockpit.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

use crate::dots::DotLight;
use crate::palette::Palette;
use crate::palette_material::{
    LIGHT_LOCAL, LIGHT_MOON_KEY, LIGHT_SHADOWS, MAX_LOCAL_LIGHTS, PaletteMaterial,
};
use crate::settings::{AppState, Settings};

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        if let Some((.., size)) = shadow_tuning() {
            app.insert_resource(bevy::light::DirectionalLightShadowMap { size });
        }
        app.init_resource::<LightingState>()
            .add_systems(OnEnter(AppState::Flight), spawn_sun)
            .add_systems(OnExit(AppState::Flight), despawn_lights)
            // The scene behind the menu (menu_scene.rs) has them too.
            .add_systems(OnExit(AppState::Menu), despawn_lights)
            // Before the cascades are built (PostUpdate) so a shadow toggle applies at once.
            .add_systems(
                Update,
                (update_sun, sync_mirror_lights).run_if(crate::settings::world_shown),
            )
            // Before the light frusta and shadow visibility are worked out from the cascades.
            .add_systems(
                PostUpdate,
                no_offscreen_shadows
                    .after(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades)
                    .before(bevy::light::SimulationLightSystems::UpdateLightFrusta),
            )
            .add_systems(
                PostUpdate,
                (collect_local_lights, mirror_point_lights)
                    .chain()
                    .in_set(LightingSystems)
                    .after(TransformSystems::Propagate)
                    .run_if(crate::settings::world_shown),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LightingSystems;

/// A light source in the world (render-space position from its `GlobalTransform`).
#[derive(Component, Clone, Copy)]
pub struct LightEmitter {
    /// Linear colour times intensity (1.0 ~ doubles the brightness of a surface next to it).
    pub color: Vec3,
    /// Range in metres (light falls to zero there).
    pub range: f32,
}

/// What the world shader gets this frame (copied into `Globals` by `palette::update_globals`).
#[derive(Resource, Default)]
pub struct LightingState {
    pub sun: Vec4,
    /// Direction to the moon (xyz) and its strength (w); zero but in modern shading or with
    /// shadows.
    pub moon: Vec4,
    /// `Globals::shadow` and `Globals::shadow_bounds`.
    pub shadow: Vec4,
    pub shadow_bounds: Vec4,
    pub ambient: Vec4,
    /// (position, range), (colour, 0) pairs.
    pub lights: Vec<(Vec4, Vec4)>,
    /// Daylight 0 (night) .. 1 (day), from the sky colour.
    pub daylight: f32,
}

/// The directional light with the shadow maps: the sun, or the moon when it is the brighter.
#[derive(Component)]
struct KeyLight;

/// The other one of the sun and the moon (no shadows).
#[derive(Component)]
struct FillLight;

/// The light balance of modern shading and of the shadows option: the world shader's sun
/// strength (classic shaders without shadows: 0.45) and the
/// moon's in a full moon high up; shadowed flat ground as a share of the sunlit (the ambient
/// light follows the direct light on the ground, so shadows keep their depth with the sun
/// low); the cockpit's ambient light (night, day) and sun illuminance (lux).
#[derive(Clone, Copy)]
struct Balance {
    sun: f32,
    moon: f32,
    shadow: f32,
    cockpit_ambient: (f32, f32),
    cockpit_sun: f32,
}

const MODERN: Balance = Balance {
    sun: 1.1,
    moon: 0.7,
    shadow: 0.4,
    cockpit_ambient: (300.0, 550.0),
    cockpit_sun: 5000.0,
};

/// `SM89_LIGHT=sun,moon,shadow,ambient_night,ambient_day,cockpit_sun` overrides [`MODERN`]
/// (tuning).
fn modern_balance() -> Balance {
    let v: Vec<f32> = std::env::var("SM89_LIGHT")
        .ok()
        .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_default();
    match v[..] {
        [sun, moon, shadow, an, ad, cs] => {
            Balance { sun, moon, shadow, cockpit_ambient: (an, ad), cockpit_sun: cs }
        }
        _ => MODERN,
    }
}

/// A `PointLight` mirroring one of the nearest emitters (for StandardMaterial surfaces).
#[derive(Component)]
struct MirrorLight(usize);

/// Point lights mirrored for the cockpit.
const MIRRORED: usize = 4;

/// Direction to the sun: the sim data's (unused) light vector (-36, 36, 36), as in modern mode.
pub fn sun_direction() -> Vec3 {
    Vec3::new(-36.0, 36.0, 36.0).normalize()
}

/// `SM89_SHADOW=cascades,max_m,first_m,map_size` (tuning).
fn shadow_tuning() -> Option<(usize, f32, f32, usize)> {
    let v: Vec<f32> =
        std::env::var("SM89_SHADOW").ok()?.split(',').filter_map(|x| x.parse().ok()).collect();
    match v[..] {
        [c, m, f, s] => Some((c as usize, m, f, s as usize)),
        _ => None,
    }
}

/// The sun's shadow cascades: the first one for the cockpit. VR: only one more, to 800 m
/// (each cascade is rendered once per eye; on the Steam Frame four cost about 3 ms more).
fn shadow_cascades(vr: bool) -> bevy::light::CascadeShadowConfig {
    let default = if vr { (2, 800.0, 6.0, 2048) } else { (4, 3000.0, 6.0, 2048) };
    let (num_cascades, maximum_distance, first_cascade_far_bound, _) =
        shadow_tuning().unwrap_or(default);
    CascadeShadowConfigBuilder {
        num_cascades,
        minimum_distance: 0.1,
        first_cascade_far_bound,
        maximum_distance,
        overlap_proportion: 0.2,
    }
    .build()
}

fn spawn_sun(mut commands: Commands, settings: Res<Settings>) {
    spawn_lights(&mut commands, &settings);
}

/// The sun and the moon's lights (and the mirrored point lights with the local lighting).
pub fn spawn_lights(commands: &mut Commands, settings: &Settings) {
    commands.spawn((
        KeyLight,
        DirectionalLight { illuminance: 0.0, shadow_maps_enabled: false, ..default() },
        Transform::IDENTITY.looking_to(-sun_direction(), Vec3::Y),
        // Detail levels picked by only one view are moved off layer 0 (models::LodShown); they
        // still cast shadows.
        bevy::camera::visibility::RenderLayers::from_layers(&[
            0,
            crate::camera::MAIN_LOD_LAYER,
            crate::camera::TRACKCAM_LOD_LAYER,
        ]),
        shadow_cascades(settings.vr),
    ));
    commands.spawn((
        FillLight,
        DirectionalLight { illuminance: 0.0, shadow_maps_enabled: false, ..default() },
        Transform::default(),
    ));
    // Only with the local lighting option: point lights put the PBR shader's clustered light
    // loop to work in every lit pixel, and on the Steam Frame (Turnip, Adreno 750) that loop
    // hangs the GPU.
    if settings.local_lights {
        spawn_mirror_lights(commands);
    }
}

fn spawn_mirror_lights(commands: &mut Commands) {
    for i in 0..MIRRORED {
        commands.spawn((
            MirrorLight(i),
            PointLight { intensity: 0.0, range: 1.0, shadow_maps_enabled: false, ..default() },
            Transform::default(),
        ));
    }
}

/// Adds or removes the mirrored point lights when the local lights option changes in flight.
fn sync_mirror_lights(
    mut commands: Commands,
    settings: Res<Settings>,
    lights: Query<Entity, With<MirrorLight>>,
) {
    if !settings.is_changed() || settings.local_lights != lights.is_empty() {
        return;
    }
    if settings.local_lights {
        spawn_mirror_lights(&mut commands);
    } else {
        for e in &lights {
            commands.entity(e).despawn();
        }
    }
}

#[allow(clippy::type_complexity)]
fn despawn_lights(
    mut commands: Commands,
    q: Query<Entity, Or<(With<KeyLight>, With<FillLight>, With<MirrorLight>)>>,
) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

/// The offscreen cameras (targeting pod, recon photos) get no shadow cascades: Bevy would
/// render the shadow maps once more for each of them (palette.wgsl `offscreen_view` skips the
/// lookup there).
fn no_offscreen_shadows(
    mut lights: Query<&mut bevy::light::Cascades, With<KeyLight>>,
    cams: Query<Entity, With<crate::camera::OffscreenCamera>>,
) {
    for mut cascades in &mut lights {
        for cam in &cams {
            if let Some(c) = cascades.cascades.get_mut(&cam) {
                c.clear();
            }
        }
    }
}

/// Bevy light units for a lit-surface brightness of 1.0 at the default camera exposure.
pub const LUX_PER_UNIT: f32 = 2600.0;

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_sun(
    settings: Res<Settings>,
    palette: Option<Res<Palette>>,
    celestial: Res<crate::sky::Sky>,
    mut state: ResMut<LightingState>,
    mut key: Query<
        (&mut DirectionalLight, &mut Transform, &bevy::light::CascadeShadowConfig),
        (With<KeyLight>, Without<FillLight>),
    >,
    mut fill: Query<(&mut DirectionalLight, &mut Transform), With<FillLight>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    weather: Res<crate::weather::WeatherState>,
    scene: Option<Res<crate::menu_scene::MenuScene>>,
) {
    // The scene behind the menu always has them.
    let shadows_on = settings.shadows || scene.is_some();
    let Some(palette) = palette else { return };
    // Sky colour 0x8F (top band) as a daylight measure.
    let sky = palette.color(0x8F).to_linear();
    let lum = 0.2126 * sky.red + 0.7152 * sky.green + 0.0722 * sky.blue;
    let daylight = ((lum - 0.02) / 0.25).clamp(0.0, 1.0);
    state.daylight = daylight;
    // World shader: ambient + sun keeps sunlit faces near the classic brightness and
    // darkens shadowed ones. Under an overcast the sun is hidden.
    let shade = weather.shade.max(weather.whiteout);
    // The balance balance, also for classic shaders with shadows (the classic one leaves
    // shadows faint, the more so with the sun low).
    let balance = (settings.modern_shading || shadows_on).then(modern_balance);
    // How far the sun is up: 0 below the horizon .. 1 a few degrees above it (without the
    // moving sun, from the sky colour).
    let sun_up = if celestial.on {
        let t = ((celestial.sun.y + 0.03) / 0.09).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    } else {
        (daylight * 4.0).min(1.0)
    };
    // The sun's strength follows its height, not the sky's brightness (the palette has
    // that), so a low sun still casts deep shadows.
    let strength = match balance {
        Some(b) => b.sun * sun_up,
        None => 0.45 * daylight,
    } * (1.0 - 0.85 * shade);
    // Continuous time of day: the moving sun, kept a little above the horizon for the shading.
    let dir = if celestial.on {
        let s = celestial.sun;
        Vec3::new(s.x, s.y.max(0.08), s.z).normalize()
    } else {
        sun_direction()
    };
    state.sun = dir.extend(strength);
    // Modern shading or shadows: the moon lights the night (`Sky::moonlight` fades it out as
    // it sets).
    let moon_strength = match balance.filter(|_| celestial.on) {
        Some(b) => b.moon * celestial.moonlight * (1.0 - daylight) * (1.0 - 0.85 * shade),
        None => 0.0,
    };
    let m = celestial.moon.normalize_or(Vec3::Y);
    let moon_dir = Vec3::new(m.x, m.y.max(0.08), m.z).normalize();
    state.moon = moon_dir.extend(moon_strength);
    // The moon casts the shadows when it is the brighter one.
    let moon_key = moon_strength > strength;
    let flags = (shadows_on as u32 * LIGHT_SHADOWS)
        | (settings.local_lights as u32 * LIGHT_LOCAL)
        | (moon_key as u32 * LIGHT_MOON_KEY);
    state.ambient = match balance {
        // The ambient light as a share of the direct light on flat ground: shadowed ground at
        // `Balance::shadow` of the sunlit, more under an overcast; little direct light (dusk)
        // leaves little to shadow.
        Some(b) => {
            let r = b.shadow + (0.9 - b.shadow) * shade;
            let on_ground = strength * dir.y.max(0.15) + moon_strength * moon_dir.y.max(0.15);
            let a = r / (1.0 - r) * on_ground + 0.02;
            Vec4::new(a, a, a * 1.05, flags as f32)
        }
        None => {
            let l = strength + moon_strength;
            Vec4::new(1.0 - l * 0.6, 1.0 - l * 0.6, 1.0 - l * 0.55, flags as f32)
        }
    };
    // Cockpit: (direction, illuminance, colour) of the sun and the moon, the moon in the same
    // proportion to the sun as in the world shader.
    let sun_light = (
        dir,
        match balance {
            Some(b) => b.cockpit_sun * daylight * (1.0 - 0.85 * shade),
            // A little light at night.
            None => LUX_PER_UNIT * (0.15 + 0.85 * daylight) * daylight.max(0.05),
        },
        Color::linear_rgb(1.0, 0.97, 0.9),
    );
    let moon_light = (
        moon_dir,
        balance.map_or(0.0, |b| b.cockpit_sun * moon_strength / b.sun),
        Color::linear_rgb(0.75, 0.85, 1.0),
    );
    let (k, f) = if moon_key { (moon_light, sun_light) } else { (sun_light, moon_light) };
    let shadows = shadows_on && shade < 0.5;
    let set = |light: &mut DirectionalLight,
               tf: &mut Transform,
               (dir, lux, colour): (Vec3, f32, Color)| {
        *tf = Transform::IDENTITY.looking_to(-dir, Vec3::Y);
        light.illuminance = lux;
        light.color = colour;
    };
    if let Ok((mut light, mut tf)) = fill.single_mut() {
        set(&mut light, &mut tf, f);
        light.shadow_maps_enabled = false;
    }
    state.shadow = Vec4::ZERO;
    if let Ok((mut light, mut tf, config)) = key.single_mut() {
        set(&mut light, &mut tf, k);
        light.shadow_maps_enabled = shadows;
        // Bevy sorts the directional lights without shadow maps (the fill light) first.
        if shadows {
            let n = config.bounds.len().min(4);
            state.shadow = Vec4::new(fill.iter().len() as f32, n as f32, 0.0, 0.0);
            let mut b = [f32::MAX; 4];
            b[..n].copy_from_slice(&config.bounds[..n]);
            state.shadow_bounds = Vec4::from_array(b);
        }
    }
    // Cockpit ambient: palette-independent, dim at night (the instrument lights take over).
    ambient.color = Color::linear_rgb(0.8, 0.85, 1.0);
    // Lightning lights the cockpit too.
    let flash = 6000.0 * weather.flash;
    ambient.brightness = match balance {
        // The night's light fades out as the sun comes up (it would soften the shadows of a
        // low sun); an overcast turns the hidden sun's light into ambient light.
        Some(b) => {
            let (night, day) = b.cockpit_ambient;
            let day = day + b.cockpit_sun / std::f32::consts::PI * 0.85 * shade * 0.7;
            night * (1.0 - sun_up) + day * daylight + flash
        }
        None => 600.0 + 800.0 * daylight + flash,
    };
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn collect_local_lights(
    settings: Res<Settings>,
    palette: Option<Res<Palette>>,
    mut state: ResMut<LightingState>,
    cams: Query<&GlobalTransform, With<crate::camera::EyeCamera>>,
    emitters: Query<(&LightEmitter, &GlobalTransform, &InheritedVisibility)>,
    dots: Query<
        (
            &GlobalTransform,
            &InheritedVisibility,
            &MeshMaterial3d<PaletteMaterial>,
            Option<&bevy::camera::visibility::RenderLayers>,
        ),
        With<DotLight>,
    >,
    materials: Res<Assets<PaletteMaterial>>,
    lamps: Query<(&crate::scenery::LampBatch, &GlobalTransform, &InheritedVisibility)>,
) {
    state.lights.clear();
    if !settings.local_lights {
        return;
    }
    let Ok(eye) = cams.single() else { return };
    let eye = eye.translation();
    let mut all: Vec<(f32, Vec4, Vec4)> = Vec::new();
    for (em, tf, vis) in &emitters {
        if !vis.get() {
            continue;
        }
        let p = tf.translation();
        // Score: distance relative to range (bright, big lights win).
        let d = p.distance(eye);
        all.push((d / em.range, p.extend(em.range), em.color.extend(0.0)));
    }
    // Dot lights (runway, approach, city lights) within 600 m, only when they are lit.
    if let Some(palette) = palette.filter(|p| p.lights_on()) {
        let track_only =
            bevy::camera::visibility::RenderLayers::layer(crate::camera::TRACKCAM_LOD_LAYER);
        for (tf, vis, mat, layers) in &dots {
            // Skip dots of a detail level only the tracking camera draws.
            if !vis.get() || layers == Some(&track_only) {
                continue;
            }
            let p = tf.translation();
            let d = p.distance(eye);
            if d > 600.0 {
                continue;
            }
            let Some(m) = materials.get(&mat.0) else { continue };
            let c = palette.color((m.params.word & 0xFF) as u8).to_linear();
            let range = 25.0;
            all.push((d / range, p.extend(range), Vec4::new(c.red, c.green, c.blue, 0.0) * 0.8));
        }
        // Street lamps and lit windows of the extra scenery (hidden by day).
        for (batch, tf, vis) in &lamps {
            if !vis.get() || tf.translation().distance(eye) > 600.0 + batch.radius {
                continue;
            }
            for (&l, &colour) in batch.points.iter().zip(&batch.colours) {
                let p = tf.transform_point(l);
                let d = p.distance(eye);
                if d < 600.0 {
                    let c = palette.color(colour).to_linear();
                    let colour = Vec4::new(c.red, c.green, c.blue, 0.0) * 0.8;
                    all.push((d / 25.0, p.extend(25.0), colour));
                }
            }
        }
    }
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    state.lights.extend(all.into_iter().take(MAX_LOCAL_LIGHTS).map(|(_, p, c)| (p, c)));
}

/// The nearest emitters as Bevy point lights (so explosions and flares light the cockpit).
fn mirror_point_lights(
    settings: Res<Settings>,
    state: Res<LightingState>,
    mut q: Query<(&MirrorLight, &mut PointLight, &mut Transform)>,
) {
    for (m, mut light, mut tf) in &mut q {
        match state.lights.get(m.0).filter(|_| settings.local_lights) {
            Some((p, c)) => {
                tf.translation = p.truncate();
                light.range = p.w;
                light.color = Color::linear_rgb(c.x, c.y, c.z);
                // Point light intensity in lumens for roughly the same effect as the shader.
                light.intensity = LUX_PER_UNIT
                    * 4.0
                    * std::f32::consts::PI
                    * (p.w * 0.3).powi(2)
                    * c.truncate().max_element();
            }
            None => light.intensity = 0.0,
        }
    }
}
