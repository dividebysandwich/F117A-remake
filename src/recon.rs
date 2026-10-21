//! Recon photos of the mission's ground targets for the kneeboard. Once the world is there,
//! the tile objects and scenery around each target are streamed in for a moment and an
//! offscreen camera photographs it obliquely from the south; the picture is read back once,
//! developed into black and white with a fixed film grain, and kept for the flight.

use std::sync::Arc;

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{TextureFormat, TextureUsages};

use crate::camera::{OffscreenCamera, RECON_BACKGROUND_LAYER, TRACKCAM_LOD_LAYER};
use crate::canvas::Picture;
use crate::mission::ActiveMission;
use crate::scenery::SceneryBacklog;
use crate::settings::AppState;
use crate::world::{StreamSites, Theater, WorldPos, wu_to_bevy};

pub struct ReconPlugin;

impl Plugin for ReconPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ReconPhotos>()
            .add_systems(OnEnter(AppState::Flight), reset)
            .add_systems(Update, (start, shoot).chain().run_if(in_state(AppState::Flight)));
    }
}

/// Picture size on the kneeboard (texels); rendered at `SUPERSAMPLE` times that (a width of
/// whole 64-texel blocks, so the read-back rows carry no padding) and box filtered down.
pub const PHOTO_W: usize = 352;
pub const PHOTO_H: usize = 264;
const SUPERSAMPLE: usize = 2;
/// Slant range (m) to a point target and to an area target, depression below the horizon and
/// horizontal field of view.
const RANGE_M: f64 = 650.0;
const AREA_RANGE_M: f64 = 1400.0;
const DEPRESSION_DEG: f64 = 40.0;
const FOV_DEG: f32 = 24.0;
/// Seconds the camera looks after the world around the targets is in (detail levels and
/// pipelines catch up); a black picture (nothing drawn yet) is taken again a second later, this
/// often.
const SETTLE_S: f32 = 2.0;
const RETRIES: u32 = 4;
/// Taken anyway this long after starting when the scenery is still building.
const GIVE_UP_S: f32 = 20.0;
/// Film grain strength (fraction of full scale) and the vignette's darkening in the corners.
const GRAIN: f32 = 0.11;
const VIGNETTE: f32 = 0.3;
/// Narrowest luminance range stretched to the full scale (out of 255).
const MIN_SPAN: f32 = 64.0;

/// A recon camera (one per target while its picture is taken).
#[derive(Component)]
pub struct ReconCamera;

/// The pictures of the primary and the secondary target.
#[derive(Resource, Default)]
pub struct ReconPhotos {
    pub photos: [Option<Arc<Picture>>; 2],
    /// Pictures still to take.
    shots: Vec<Shot>,
    started: bool,
    /// Seconds since starting, and since the world around the targets is in.
    elapsed: f32,
    settled: f32,
}

struct Shot {
    /// 0 primary, 1 secondary.
    slot: usize,
    image: Handle<Image>,
    /// Settled time at which to read the picture back (None while a read-back is under way).
    due: Option<f32>,
    tries: u32,
}

impl ReconPhotos {
    /// The target's picture is still being taken.
    pub fn developing(&self, slot: usize) -> bool {
        !self.started || self.shots.iter().any(|s| s.slot == slot)
    }
}

/// Aircraft targets (mission types 5-8) have nothing to photograph on the ground.
pub fn airborne(t: &stealth_mission::Target) -> bool {
    (5..=8).contains(&t.mission_type)
}

fn reset(mut recon: ResMut<ReconPhotos>, mut sites: ResMut<StreamSites>) {
    *recon = ReconPhotos::default();
    *sites = StreamSites::default();
}

/// Puts a camera over each ground target and streams the world in around it.
#[allow(clippy::too_many_arguments)]
fn start(
    mut commands: Commands,
    mut recon: ResMut<ReconPhotos>,
    mut sites: ResMut<StreamSites>,
    mission: Option<Res<ActiveMission>>,
    theater: Option<Res<Theater>>,
    palette: Option<Res<crate::palette::Palette>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if recon.started {
        return;
    }
    let (Some(m), Some(_), Some(palette)) = (mission, theater, palette) else { return };
    recon.started = true;
    let (w, h) = (PHOTO_W * SUPERSAMPLE, PHOTO_H * SUPERSAMPLE);
    let tan_half_w = (FOV_DEG.to_radians() / 2.0).tan();
    let fov = 2.0 * (tan_half_w * h as f32 / w as f32).atan();
    let dep = DEPRESSION_DEG.to_radians();
    for (slot, t) in [&m.0.primary, &m.0.secondary].into_iter().enumerate() {
        let p = t.position;
        if airborne(t) || (p.x, p.y) == (0, 0) {
            continue;
        }
        // The middle of the target's map unit, as the tracking camera aims at it.
        let east = p.x as f64 * 32.0 + 16.0;
        let north = (0x8000 - p.y) as f64 * 32.0 - 16.0;
        let ground = wu_to_bevy(east, north, 0.0);
        let range = if t.area_target { AREA_RANGE_M } else { RANGE_M };
        let eye = ground + bevy::math::DVec3::new(0.0, dep.sin(), dep.cos()) * range;
        let mut image = Image::new_target_texture(
            w as u32,
            h as u32,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        );
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image = images.add(image);
        let cam = commands
            .spawn((
                ReconCamera,
                OffscreenCamera,
                Camera3d::default(),
                Camera {
                    order: -2,
                    clear_color: ClearColorConfig::Custom(Color::BLACK),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Projection::Perspective(PerspectiveProjection {
                    fov,
                    aspect_ratio: w as f32 / h as f32,
                    near: 50.0,
                    far: 600_000.0,
                    ..default()
                }),
                Tonemapping::None,
                Msaa::Off,
                WorldPos(eye),
                Transform::IDENTITY.looking_to((ground - eye).as_vec3(), Vec3::Y),
                RenderLayers::from_layers(&[0, TRACKCAM_LOD_LAYER, RECON_BACKGROUND_LAYER[slot]]),
                DespawnOnExit(AppState::Flight),
            ))
            .id();
        crate::camera::spawn_background(
            &mut commands,
            cam,
            &mut meshes,
            &palette,
            RECON_BACKGROUND_LAYER[slot],
        );
        sites.points.push(ground);
        recon.shots.push(Shot { slot, image, due: Some(SETTLE_S), tries: 0 });
    }
    sites.streamed = false;
}

/// Reads the pictures back once the world around the targets has settled; when all are taken
/// the cameras go and the world there is streamed out again.
fn shoot(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut recon: ResMut<ReconPhotos>,
    mut sites: ResMut<StreamSites>,
    backlog: Res<SceneryBacklog>,
    cams: Query<Entity, With<ReconCamera>>,
) {
    if recon.shots.is_empty() {
        if !sites.points.is_empty() {
            sites.points.clear();
            for e in &cams {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    let dt = time.delta_secs();
    recon.elapsed += dt;
    if (sites.streamed && backlog.0 == Some(0)) || recon.elapsed > GIVE_UP_S {
        recon.settled += dt;
    }
    let settled = recon.settled;
    for shot in &mut recon.shots {
        if shot.due.is_none_or(|t| settled < t) {
            continue;
        }
        shot.due = None;
        let slot = shot.slot;
        commands
            .spawn((Readback::texture(shot.image.clone()), DespawnOnExit(AppState::Flight)))
            .observe(
                move |ev: On<ReadbackComplete>,
                      mut commands: Commands,
                      mut recon: ResMut<ReconPhotos>| {
                    commands.entity(ev.entity).despawn();
                    let picture = develop(&ev.data, slot as u32);
                    let settled = recon.settled;
                    let Some(i) = recon.shots.iter().position(|s| s.slot == slot) else { return };
                    let shot = &mut recon.shots[i];
                    if picture.is_none() && shot.tries < RETRIES {
                        shot.tries += 1;
                        shot.due = Some(settled + 1.0);
                        return;
                    }
                    recon.shots.remove(i);
                    recon.photos[slot] = picture.map(Arc::new);
                },
            );
    }
}

/// The rendered picture (sRGB RGBA at `SUPERSAMPLE` times the size) developed into black and
/// white: box filtered down, its levels stretched between the darkest and the brightest
/// percent (a flat one is not stretched further than `MIN_SPAN`), a slight vignette and a
/// fixed grain. None for a black picture (nothing drawn yet).
fn develop(rgba: &[u8], seed: u32) -> Option<Picture> {
    let (w, h) = (PHOTO_W * SUPERSAMPLE, PHOTO_H * SUPERSAMPLE);
    if rgba.len() < w * h * 4 {
        return None;
    }
    let mut lum = vec![0.0f32; PHOTO_W * PHOTO_H];
    let n = (SUPERSAMPLE * SUPERSAMPLE) as f32;
    for (i, px) in rgba[..w * h * 4].as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i % w, i / w);
        let l = 0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32;
        lum[y / SUPERSAMPLE * PHOTO_W + x / SUPERSAMPLE] += l / n;
    }
    let mut sorted = lum.clone();
    sorted.sort_by(f32::total_cmp);
    if sorted.last().is_none_or(|&l| l < 2.0) {
        return None;
    }
    let (mut lo, mut hi) = (sorted[sorted.len() / 100], sorted[sorted.len() * 99 / 100]);
    if hi - lo < MIN_SPAN {
        let mid = (lo + hi) / 2.0;
        (lo, hi) = (mid - MIN_SPAN / 2.0, mid + MIN_SPAN / 2.0);
    }
    let mut out = Vec::with_capacity(PHOTO_W * PHOTO_H * 4);
    for (i, l) in lum.iter().enumerate() {
        let (x, y) = (i % PHOTO_W, i / PHOTO_W);
        // A print's paper white and its deepest black stay off the ends.
        let v = 0.05 + 0.88 * ((l - lo) / (hi - lo)).clamp(0.0, 1.0);
        let (dx, dy) = (x as f32 / PHOTO_W as f32 - 0.5, y as f32 / PHOTO_H as f32 - 0.5);
        let v = v * (1.0 - VIGNETTE * 2.0 * (dx * dx + dy * dy));
        // Two uniform samples: a triangular grain, finer than a uniform one at the same spread.
        let grain = noise(x as u32, y as u32, seed) + noise(x as u32, y as u32, seed + 7) - 1.0;
        let g = ((v + GRAIN * grain).clamp(0.0, 1.0) * 255.0) as u8;
        out.extend([g, g, g, 255]);
    }
    Some(Picture { w: PHOTO_W, h: PHOTO_H, rgba: out })
}

/// Hash noise in 0..1 for a texel.
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x8DA6_B343) ^ y.wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn develops_black_and_white_with_grain() {
        let (w, h) = (PHOTO_W * SUPERSAMPLE, PHOTO_H * SUPERSAMPLE);
        // Nothing drawn: no picture; a flat one is still a picture.
        assert!(develop(&vec![0; w * h * 4], 0).is_none());
        assert!(develop(&[0, 60, 0, 255].repeat(w * h), 0).is_some());
        // A coloured gradient: grey, every texel's channels equal, grain on a flat area.
        let px: Vec<u8> = (0..w * h).flat_map(|i| [(i % w * 255 / w) as u8, 90, 30, 255]).collect();
        let pic = develop(&px, 0).unwrap();
        assert_eq!(pic.rgba.len(), PHOTO_W * PHOTO_H * 4);
        assert!(pic.rgba.chunks(4).all(|c| c[0] == c[1] && c[1] == c[2] && c[3] == 255));
        let column: Vec<u8> = (0..PHOTO_H).map(|y| pic.rgba[(y * PHOTO_W + 100) * 4]).collect();
        assert!(column.iter().any(|&c| c != column[PHOTO_H / 2]));
        // The same picture every time.
        assert_eq!(develop(&px, 0).unwrap().rgba, pic.rgba);
    }
}
