//! Optional lighting (menu "Shadows" / "Local lighting", `--shadows` / `--local-lights`).
//!
//! - Sun: one `DirectionalLight` from the modern-mode light vector, strength from the live sky
//!   colour (off at night). It lights the 3D cockpit (StandardMaterial) and, through the shadow
//!   maps, the palette-material world (palette.wgsl `lit`), including the background's ground
//!   gradient (shadows taken on the ground plane).
//! - Local lights: `LightEmitter` entities (missile motors, flares, impact flashes, cockpit
//!   lamps) plus nearby runway/city dot lights. The nearest ones go to the world shader
//!   (`Globals.lights`) and a few become Bevy `PointLight`s so they also light the cockpit.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

use crate::dots::DotLight;
use crate::palette::Palette;
use crate::palette_material::{LIGHT_LOCAL, LIGHT_SHADOWS, MAX_LOCAL_LIGHTS, PaletteMaterial};
use crate::settings::{AppState, Settings};

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LightingState>()
            .add_systems(OnEnter(AppState::Flight), spawn_sun)
            .add_systems(OnExit(AppState::Flight), despawn_lights)
            // Before the cascades are built (PostUpdate) so a shadow toggle applies at once.
            .add_systems(
                Update,
                (update_sun, sync_mirror_lights).run_if(in_state(AppState::Flight)),
            )
            .add_systems(
                PostUpdate,
                (collect_local_lights, mirror_point_lights)
                    .chain()
                    .in_set(LightingSystems)
                    .after(TransformSystems::Propagate)
                    .run_if(in_state(AppState::Flight)),
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
    pub ambient: Vec4,
    /// (position, range), (colour, 0) pairs.
    pub lights: Vec<(Vec4, Vec4)>,
    /// Daylight 0 (night) .. 1 (day), from the sky colour.
    pub daylight: f32,
}

#[derive(Component)]
struct Sun;

/// A `PointLight` mirroring one of the nearest emitters (for StandardMaterial surfaces).
#[derive(Component)]
struct MirrorLight(usize);

/// Point lights mirrored for the cockpit.
const MIRRORED: usize = 4;

/// Direction to the sun: the original's (unused) light vector (-36, 36, 36), as in modern mode.
pub fn sun_direction() -> Vec3 {
    Vec3::new(-36.0, 36.0, 36.0).normalize()
}

fn spawn_sun(mut commands: Commands, settings: Res<Settings>) {
    commands.spawn((
        Sun,
        DirectionalLight { illuminance: 0.0, shadow_maps_enabled: false, ..default() },
        Transform::IDENTITY.looking_to(-sun_direction(), Vec3::Y),
        // Detail levels picked by only one view are moved off layer 0 (models::LodShown); they
        // still cast shadows.
        bevy::camera::visibility::RenderLayers::from_layers(&[
            0,
            crate::camera::MAIN_LOD_LAYER,
            crate::camera::TRACKCAM_LOD_LAYER,
        ]),
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            minimum_distance: 0.1,
            first_cascade_far_bound: 6.0,
            maximum_distance: 3000.0,
            overlap_proportion: 0.2,
        }
        .build(),
    ));
    // Only with the local lighting option: point lights put the PBR shader's clustered light
    // loop to work in every lit pixel, and on the Steam Frame (Turnip, Adreno 750) that loop
    // hangs the GPU.
    if settings.local_lights {
        spawn_mirror_lights(&mut commands);
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
fn despawn_lights(mut commands: Commands, q: Query<Entity, Or<(With<Sun>, With<MirrorLight>)>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

/// Bevy light units for a lit-surface brightness of 1.0 at the default camera exposure.
pub const LUX_PER_UNIT: f32 = 2600.0;

fn update_sun(
    settings: Res<Settings>,
    palette: Option<Res<Palette>>,
    celestial: Res<crate::sky::Sky>,
    mut state: ResMut<LightingState>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    weather: Res<crate::weather::WeatherState>,
) {
    let Some(palette) = palette else { return };
    // Sky colour 0x8F (top band) as a daylight measure.
    let sky = palette.color(0x8F).to_linear();
    let lum = 0.2126 * sky.red + 0.7152 * sky.green + 0.0722 * sky.blue;
    let daylight = ((lum - 0.02) / 0.25).clamp(0.0, 1.0);
    state.daylight = daylight;
    // World shader: ambient + sun keeps sunlit faces near the original brightness and
    // darkens shadowed ones. Under an overcast the sun is hidden.
    let overcast = weather.shaded || weather.whiteout > 0.5;
    let strength = 0.45 * daylight * if overcast { 0.15 } else { 1.0 };
    // Continuous time of day: the moving sun, kept a little above the horizon for the shading.
    let dir = if celestial.on {
        let s = celestial.sun;
        Vec3::new(s.x, s.y.max(0.08), s.z).normalize()
    } else {
        sun_direction()
    };
    state.sun = dir.extend(strength);
    let flags =
        (settings.shadows as u32 * LIGHT_SHADOWS) | (settings.local_lights as u32 * LIGHT_LOCAL);
    state.ambient =
        Vec4::new(1.0 - strength * 0.6, 1.0 - strength * 0.6, 1.0 - strength * 0.55, flags as f32);
    if let Ok((mut light, mut tf)) = sun.single_mut() {
        if celestial.on {
            *tf = Transform::IDENTITY.looking_to(-dir, Vec3::Y);
        }
        light.illuminance = LUX_PER_UNIT * (0.15 + 0.85 * daylight) * daylight.max(0.05);
        light.shadow_maps_enabled = settings.shadows && !overcast;
        light.color = Color::linear_rgb(1.0, 0.97, 0.9);
    }
    // Cockpit ambient: palette-independent, dim at night (the instrument lights take over).
    ambient.color = Color::linear_rgb(0.8, 0.85, 1.0);
    // Lightning lights the cockpit too.
    ambient.brightness = 600.0 + 800.0 * daylight + 6000.0 * weather.flash;
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
