//! Panel picture, MFD screens, HUD and the fixed cockpit structure.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::geometry::{beam, panel_patch, panel_patch_uv, panel_point, pixel_dir, recess_walls, uv_quad};
use super::{CockpitMaterials, spawn_box};
use crate::canvas::{Canvas, CanvasTexture};
use crate::cockpit::{CockpitArt, Displays};
use crate::font::Fonts;
use crate::mfd::{LEFT, RIGHT, RightMfd};

/// How far the MFD screens sit behind the panel face (metres).
const SCREEN_DEPTH: f32 = 0.014;
/// Distance of the HUD image (metres): far enough to read as "out there", near enough not
/// to sink into the ground while taxiing.
const HUD_DISTANCE: f32 = 4.0;
/// Emissive strength of the displays and lit lights when the cockpit is lit.
const GLOW_NITS: f32 = 1.0;

/// A cockpit material whose `unlit` follows the lighting options (see `sync_materials`).
#[derive(Component)]
pub struct LitMaterial(pub Handle<StandardMaterial>);

#[derive(Component)]
pub(super) struct PanelTexture {
    /// 256PIT without the sprite rows: the panel picture the instruments are drawn onto.
    base: Canvas,
    /// See-through window area of the panel picture.
    window: Vec<bool>,
}

#[derive(Component)]
pub(super) struct GlowTexture;

#[derive(Component)]
pub(super) struct HudTexture;

#[derive(Component)]
pub(super) struct TrackCamScreen;

/// Panel regions that glow (displays, lights, LED readouts, attitude ball, EMV bar):
/// (x, y, w, h).
fn glow_rects() -> Vec<(i32, i32, i32, i32)> {
    let mut v: Vec<_> = crate::panel::LIGHTS.iter().map(|&(x, y, w, h, _)| (x, y, w + 1, h + 1)).collect();
    for r in [LEFT, RIGHT] {
        v.push((r[0], r[1], r[2] - r[0] + 1, r[3] - r[1] + 1));
    }
    v.extend_from_slice(&[(35, 149, 10, 27), (261, 179, 15, 19), (265, 138, 33, 33), (86, 187, 51, 11)]);
    v
}

pub(super) fn spawn_panel(
    commands: &mut Commands,
    root: Entity,
    mats: &CockpitMaterials,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    tc_image: Option<&crate::trackcam::TrackCamImage>,
) {
    let mut base = crate::cockpit::load_idx("256pit");
    for i in 0..91 * base.w {
        base.px[i] = 0;
    }
    let window = crate::cockpit::window_mask(&base, 91);
    let mut panel_tex = CanvasTexture::new(base.clone(), images, true);
    panel_tex.mask = Some(window.clone());
    let glow_tex = CanvasTexture::new(Canvas::new(320, 200), images, false);
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(panel_tex.image.clone()),
        emissive_texture: Some(glow_tex.image.clone()),
        emissive: LinearRgba::rgb(GLOW_NITS, GLOW_NITS, GLOW_NITS),
        alpha_mode: AlphaMode::Mask(0.5),
        perceptual_roughness: 0.75,
        cull_mode: None,
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let holder = commands
        .spawn((PanelTexture { base, window }, panel_tex, LitMaterial(material.clone()), Transform::default(), Visibility::Hidden))
        .id();
    commands.entity(root).add_child(holder);
    let glow = commands.spawn((GlowTexture, glow_tex)).id();
    commands.entity(root).add_child(glow);

    let mut add = |commands: &mut Commands, mesh: Mesh, material: Handle<StandardMaterial>| {
        let e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Transform::default(), NoFrustumCulling)).id();
        commands.entity(root).add_child(e);
        e
    };
    // Panel face around the MFD holes.
    let (l, r) = (LEFT.map(|v| v as f32), RIGHT.map(|v| v as f32));
    for (x0, y0, x1, y1) in [
        (0.0, 91.0, 320.0, l[1]),
        (0.0, l[1], l[0], l[3] + 1.0),
        (l[2] + 1.0, l[1], r[0], l[3] + 1.0),
        (r[2] + 1.0, l[1], 320.0, l[3] + 1.0),
        (0.0, l[3] + 1.0, 320.0, 200.0),
    ] {
        add(commands, panel_patch(x0, y0, x1, y1, 0.0), material.clone());
    }
    // Recessed MFD screens with their wells and raised bezel frames.
    for s in [l, r] {
        let (x0, y0, x1, y1) = (s[0], s[1], s[2] + 1.0, s[3] + 1.0);
        add(commands, panel_patch(x0, y0, x1, y1, -SCREEN_DEPTH), material.clone());
        add(commands, recess_walls(x0, y0, x1, y1, SCREEN_DEPTH), mats.black.clone());
        let c = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        for k in 0..4 {
            let (a, b) = (c[k], c[(k + 1) % 4]);
            let tf = beam(panel_point(a.0, a.1, 0.004), panel_point(b.0, b.1, 0.004), Vec2::splat(0.009));
            spawn_box(commands, root, mats, &mats.frame, tf);
        }
    }
    // Tracking camera picture behind the right screen (seen where the page is see-through).
    if let Some(img) = tc_image {
        let m = materials.add(StandardMaterial {
            base_color_texture: Some(img.0.clone()),
            unlit: true,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        });
        let (x0, y0, x1, y1) = (r[0], r[1], r[2] + 1.0, r[3] + 1.0);
        let mesh = panel_patch_uv(x0, y0, x1, y1, -SCREEN_DEPTH - 0.001, |px, py| Vec2::new((px - x0) / (x1 - x0), (py - y0) / (y1 - y0)));
        let e = add(commands, mesh, m);
        commands.entity(e).insert((TrackCamScreen, Visibility::Hidden));
    }

    // HUD: transparent canvas on a quad sized to the original projection.
    let hud_tex = CanvasTexture::new(Canvas::new(320, 200), images, true);
    let hud_mat = materials.add(StandardMaterial {
        base_color_texture: Some(hud_tex.image.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Mask(0.5),
        cull_mode: None,
        fog_enabled: false,
        ..default()
    });
    let size = Vec2::new(320.0 / crate::cockpit::FOCAL, 200.0 / (crate::cockpit::VSCALE * crate::cockpit::FOCAL)) * HUD_DISTANCE;
    let hud = commands
        .spawn((
            HudTexture,
            hud_tex,
            Mesh3d(meshes.add(uv_quad(size, Vec2::ZERO, Vec2::ONE))),
            MeshMaterial3d(hud_mat),
            Transform::from_translation(pixel_dir(160.0, 100.0) * HUD_DISTANCE),
            NoFrustumCulling,
            NotShadowCaster,
        ))
        .id();
    commands.entity(root).add_child(hud);
}

/// Glare shield, combiner, canopy frame, consoles, seat and the body around the pilot.
pub(super) fn spawn_structure(commands: &mut Commands, root: Entity, mats: &CockpitMaterials, meshes: &mut Assets<Mesh>) {
    let b = |commands: &mut Commands, m: &Handle<StandardMaterial>, c: Vec3, s: Vec3| {
        spawn_box(commands, root, mats, m, Transform::from_translation(c).with_scale(s))
    };
    let beam_box = |commands: &mut Commands, m: &Handle<StandardMaterial>, a: Vec3, z: Vec3, t: f32| {
        spawn_box(commands, root, mats, m, beam(a, z, Vec2::splat(t)))
    };

    // Glare shield over the centre of the panel (art columns 64..256 reach up to row 91).
    let (el, er) = (panel_point(64.0, 91.0, 0.0), panel_point(256.0, 91.0, 0.0));
    let fwd = Vec3::new(0.0, -0.03, -0.32);
    let top = Mesh::from(Rectangle::new(1.0, 1.0));
    let width = er.x - el.x;
    let centre = (el + er) / 2.0 + fwd / 2.0 + Vec3::new(0.0, 0.002, 0.0);
    let tilt = (fwd.y / fwd.z.abs()).atan();
    let e = commands
        .spawn((
            Mesh3d(meshes.add(top)),
            MeshMaterial3d(mats.dark.clone()),
            Transform::from_translation(centre)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2 + tilt))
                .with_scale(Vec3::new(width, fwd.length(), 1.0)),
            NoFrustumCulling,
        ))
        .id();
    commands.entity(root).add_child(e);
    // Coaming along the glare shield edge.
    spawn_box(commands, root, mats, &mats.rubber, beam(el + Vec3::new(0.0, 0.006, 0.0), er + Vec3::new(0.0, 0.006, 0.0), Vec2::new(0.025, 0.02)));
    // Body behind the panel and the knee panel below it.
    b(commands, &mats.dark, Vec3::new(0.0, -0.55, -1.0), Vec3::new(1.1, 0.75, 0.4));
    b(commands, &mats.panel_grey, Vec3::new(0.0, -0.84, -0.75), Vec3::new(0.95, 0.62, 0.14));
    // Side walls, floor and the canopy sills.
    for s in [-1.0f32, 1.0] {
        b(commands, &mats.panel_grey, Vec3::new(0.56 * s, -0.62, -0.25), Vec3::new(0.06, 0.95, 1.6));
        beam_box(commands, &mats.frame, Vec3::new(0.53 * s, -0.15, 0.55), Vec3::new(0.53 * s, -0.15, -1.05), 0.05);
        // Side consoles (throttle left, stick right).
        b(commands, &mats.panel_grey, Vec3::new(0.40 * s, -0.56, 0.02), Vec3::new(0.24, 0.14, 0.75));
        b(commands, &mats.dark, Vec3::new(0.40 * s, -0.487, 0.02), Vec3::new(0.22, 0.01, 0.72));
    }
    b(commands, &mats.dark, Vec3::new(0.0, -1.15, -0.35), Vec3::new(1.1, 0.05, 1.7));

    // HUD combiner frame on two posts (art columns ~70..250 above the panel).
    let d = 0.80;
    let corner = |px: f32, py: f32| pixel_dir(px, py) * d;
    let (tl, tr, bl, br) = (corner(70.0, 2.0), corner(250.0, 2.0), corner(70.0, 90.0), corner(250.0, 90.0));
    for (top, bottom) in [(tl, bl), (tr, br)] {
        beam_box(commands, &mats.frame, top, bottom - Vec3::new(0.0, 0.05, 0.0), 0.012);
    }
    beam_box(commands, &mats.frame, tl, tr, 0.01);

    // Canopy frame: the windscreen's lower edge and the rear arch (the original's inverted V
    // over the HUD and the spine over the head are left out; they cut through the view).
    for s in [-1.0f32, 1.0] {
        let base = Vec3::new(0.24 * s, -0.17, -0.97);
        beam_box(commands, &mats.frame, base, Vec3::new(0.53 * s, -0.15, -0.72), 0.035);
        beam_box(commands, &mats.frame, Vec3::new(0.0, 0.40, 0.50), Vec3::new(0.53 * s, -0.15, 0.50), 0.04);
    }

    // Seat: pan, cushion, back, headrest, ejection rails.
    b(commands, &mats.seat, Vec3::new(0.0, -0.82, 0.08), Vec3::new(0.50, 0.12, 0.52));
    b(commands, &mats.cushion, Vec3::new(0.0, -0.745, 0.08), Vec3::new(0.42, 0.05, 0.44));
    let back = Transform::from_xyz(0.0, -0.36, 0.36).with_rotation(Quat::from_rotation_x(0.18)).with_scale(Vec3::new(0.50, 0.85, 0.10));
    spawn_box(commands, root, mats, &mats.seat, back);
    let pad = Transform::from_xyz(0.0, -0.38, 0.30).with_rotation(Quat::from_rotation_x(0.18)).with_scale(Vec3::new(0.40, 0.65, 0.04));
    spawn_box(commands, root, mats, &mats.cushion, pad);
    b(commands, &mats.cushion, Vec3::new(0.0, 0.10, 0.40), Vec3::new(0.28, 0.22, 0.10));
    for s in [-1.0f32, 1.0] {
        beam_box(commands, &mats.metal, Vec3::new(0.27 * s, -0.88, 0.40), Vec3::new(0.25 * s, 0.25, 0.47), 0.03);
    }
}

/// Draws the panel, glow and HUD canvases (as `cockpit::draw_screen` does for the pictures).
#[allow(clippy::type_complexity)]
pub(super) fn update_displays(
    art: Res<CockpitArt>,
    fonts: Res<Fonts>,
    d: Displays,
    mut panel: Query<(&PanelTexture, &mut CanvasTexture), (Without<GlowTexture>, Without<HudTexture>)>,
    mut glow: Query<&mut CanvasTexture, (With<GlowTexture>, Without<HudTexture>)>,
    mut hud: Query<&mut CanvasTexture, With<HudTexture>>,
    mut tc: Query<&mut Visibility, With<TrackCamScreen>>,
) {
    let Ok((pt, mut tex)) = panel.single_mut() else { return };
    let tex = tex.as_mut();
    tex.canvas.px.copy_from_slice(&pt.base.px);
    tex.canvas.reset_clip();
    d.mfd.draw(&mut tex.canvas, &art, &fonts);
    tex.canvas.reset_clip();
    d.panel.draw(&mut tex.canvas, &art, &fonts);
    // The right screen is see-through over the live tracking camera picture.
    let live = d.mfd.cam.live;
    if let Some(mask) = tex.mask.as_mut() {
        mask.copy_from_slice(&pt.window);
        if live {
            for y in RIGHT[1] as usize..=RIGHT[3] as usize {
                for x in RIGHT[0] as usize..=RIGHT[2] as usize {
                    mask[y * 320 + x] = true;
                }
            }
        }
    }
    tex.dirty = true;
    for mut v in &mut tc {
        v.set_if_neq(if live { Visibility::Inherited } else { Visibility::Hidden });
    }

    if let Ok(mut g) = glow.single_mut() {
        let g = g.as_mut();
        g.canvas.px.fill(0);
        for (x, y, w, h) in glow_rects() {
            g.canvas.copy_rect(&tex.canvas, x, y, w, h, x, y);
        }
        g.dirty = true;
    }

    if let Ok(mut h) = hud.single_mut() {
        let h = h.as_mut();
        h.canvas.px.fill(0);
        h.canvas.reset_clip();
        if let Some(frame) = d.hud.frame(d.mfd.state.right == RightMfd::Camera) {
            crate::hud::draw(&mut h.canvas, &art, &fonts, &frame);
        }
        h.canvas.reset_clip();
        d.panel.draw_posts(&mut h.canvas, &art);
        h.dirty = true;
    }
}
