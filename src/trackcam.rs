//! Right-MFD tracking camera (VGAME `1A18:BF71`, projection 0xC470): a second 3D camera at the
//! aircraft looking at the locked target, or along the camera direction keys (`/ , . m`) with
//! the photo lens depression, rendered into a small texture with the original focal length
//! `256 << zoom` over the 77x60 MFD rectangle. `cockpit.rs` shows the texture behind the MFD
//! hole (the page's text is drawn over it).

use bevy::camera::RenderTarget;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::camera::{View, ViewState};
use crate::combat::{Combat, CombatSystems, map_wu, unit_position};
use crate::flight::{Aircraft, Player, bam};
use crate::mfd::{CamDir, MfdState, RightMfd, TrackCam};
use crate::settings::{AppState, SetupSet};
use crate::world::{WorldPos, wu_to_bevy};

pub struct TrackCamPlugin;

impl Plugin for TrackCamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, flir_background.run_if(in_state(AppState::Flight)))
            .add_systems(OnEnter(AppState::Flight), spawn_camera.in_set(SetupSet::Spawn))
            .add_systems(OnExit(AppState::Flight), despawn_camera)
            .add_systems(Update, update_camera.after(CombatSystems).run_if(in_state(AppState::Flight)));
    }
}

/// Render scale over the original MFD pixels (77x60).
const SCALE: u32 = 2;
pub const WIDTH: u32 = 77 * SCALE;
pub const HEIGHT: u32 = 60 * SCALE;
/// Photo lens depression below the nose (weapons.md 5.4.6: 0x6EF BAM).
const LENS_DEPRESSION: f32 = 0x6EF as f32;

#[derive(Component)]
pub struct TrackCamera;

/// The texture the tracking camera renders into.
#[derive(Resource)]
pub struct TrackCamImage(pub Handle<Image>);

fn create_image(images: &mut Assets<Image>) -> Handle<Image> {
    let mut image = Image::new_target_texture(WIDTH, HEIGHT, TextureFormat::Rgba8Unorm, Some(TextureFormat::Rgba8UnormSrgb));
    image.sampler = ImageSampler::nearest();
    images.add(image)
}

/// Projection for zoom shift z: f = 256 << z pixels, vertical 0.75 f, 77x60 viewport.
fn projection(z: u8) -> PerspectiveProjection {
    let f = (256u32 << z.min(6)) as f32;
    let half_h = 30.0 / (0.75 * f);
    let half_w = 38.5 / f;
    PerspectiveProjection { fov: 2.0 * half_h.atan(), aspect_ratio: half_w / half_h, near: 20.0, far: 600_000.0, ..default() }
}

fn spawn_camera(
    mut commands: Commands,
    image: Option<Res<TrackCamImage>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Option<Res<crate::palette::Palette>>,
) {
    let image = match image {
        Some(i) => i.0.clone(),
        None => {
            let h = create_image(&mut images);
            commands.insert_resource(TrackCamImage(h.clone()));
            h
        }
    };
    let cam = commands
        .spawn((
            TrackCamera,
            Camera3d::default(),
            Camera { order: -1, is_active: false, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
            RenderTarget::Image(image.into()),
            Projection::Perspective(projection(0)),
            Tonemapping::None,
            Msaa::Off,
            WorldPos::default(),
            Transform::default(),
        ))
        .id();
    if let Some(palette) = palette {
        let sphere = crate::camera::spawn_background(&mut commands, cam, &mut meshes, &palette, crate::camera::TRACKCAM_BACKGROUND_LAYER);
        commands.entity(sphere).insert(TrackCamBackground);
        commands.entity(cam).insert(bevy::camera::visibility::RenderLayers::from_layers(&[0, crate::camera::TRACKCAM_BACKGROUND_LAYER, crate::camera::TRACKCAM_LOD_LAYER]));
    }
}

fn despawn_camera(mut commands: Commands, q: Query<Entity, With<TrackCamera>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

#[allow(clippy::type_complexity)]
fn update_camera(
    combat: Option<Res<Combat>>,
    mfd: Res<MfdState>,
    view: Res<ViewState>,
    mut info: ResMut<TrackCam>,
    player: Query<(&Aircraft, &WorldPos), (With<Player>, Without<TrackCamera>)>,
    mut cam: Query<(&mut Camera, &mut Projection, &mut WorldPos, &mut Transform), With<TrackCamera>>,
) {
    let Ok((mut camera, mut proj, mut pos, mut tf)) = cam.single_mut() else { return };
    let ac = player.single().ok();
    let want = mfd.right == RightMfd::Camera && view.view == View::Cockpit && combat.is_some() && ac.is_some();
    if camera.is_active != want {
        camera.is_active = want;
    }
    info.live = want;
    let (Some(combat), Some((a, apos))) = (combat, ac) else { return };
    if !want {
        return;
    }
    let sim = &combat.sim;
    let t = sim.targeting();
    let eye = apos.0;
    let target = if let Some(o) = t.ground.and_then(|g| sim.objects().get(g)) {
        let (e, n) = map_wu(o.x, o.y);
        Some(wu_to_bevy(e + 16.0, n - 16.0, 0.0))
    } else if let Some((i, u)) = t.air.and_then(|i| sim.flights().get(i).map(|u| (i, *u))) {
        let (e, n, z) = unit_position(combat.prev_flights.get(i), &u, combat.alpha);
        Some(wu_to_bevy(e, n, z))
    } else {
        None
    };
    let dir = match target {
        Some(p) if (p - eye).length() > 1.0 => (p - eye).as_vec3().normalize(),
        _ => {
            let (h, _, _) = a.euler();
            let d = match mfd.cam_dir {
                CamDir::Ahead => 0.0,
                CamDir::Right => 0.25,
                CamDir::Rear => 0.5,
                CamDir::Left => 0.75,
            } * std::f32::consts::TAU;
            let p = -bam(LENS_DEPRESSION);
            let yaw = h + d;
            Vec3::new(yaw.sin() * p.cos(), p.sin(), -yaw.cos() * p.cos())
        }
    };
    let up = if dir.y.abs() > 0.99 { Vec3::NEG_Z } else { Vec3::Y };
    tf.rotation = Transform::IDENTITY.looking_to(dir, up).rotation;
    if pos.0 != eye {
        pos.0 = eye;
    }
    let z = info.zoom.saturating_sub(1);
    let want_proj = projection(z);
    if let Projection::Perspective(p) = proj.as_mut()
        && (p.fov - want_proj.fov).abs() > 1e-6
    {
        *p = want_proj;
    }
}


#[derive(Component)]
struct TrackCamBackground;

/// FLIR: the camera's sky and ground become flat (sky 0x88, ground 0x7C; visuals.md 3).
fn flir_background(
    mfd: Res<MfdState>,
    palette: Option<Res<crate::palette::Palette>>,
    mut q: Query<&mut MeshMaterial3d<crate::palette_material::PaletteMaterial>, With<TrackCamBackground>>,
) {
    let Some(palette) = palette else { return };
    let want = if mfd.flir { &palette.background_flir } else { &palette.background };
    for mut m in &mut q {
        if m.0 != *want {
            m.0 = want.clone();
        }
    }
}
