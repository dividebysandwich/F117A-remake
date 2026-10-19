//! Instrument panel, MFD screens, HUD and the fixed cockpit structure.
//!
//! The panel is modelled from a `layout::PanelLayout`: the face and solid parts take their
//! colours from a 16x16 palette swatch texture (so they follow the flight palette like the
//! original art did); lamp, LED, gauge, label and bezel button faces are slots of one
//! instrument atlas canvas redrawn every frame (with a glow twin for the lit ones); each MFD
//! and the attitude ball have their own canvas.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::geometry::{
    panel_box,
    beam, panel_patch_uv, panel_point, panel_polygon, panel_rect, panel_rotation, pixel_dir, recess_walls, rect_minus_holes, uv_quad,
    with_swatch,
};
use super::hud_glass::HudGlassMaterial;
use super::layout::{Elem, OUTLINE, PanelLayout, PanelStyle, Placed, PxRect, Shape};
use super::{CockpitMaterials, spawn_box};
use crate::canvas::{Canvas, CanvasTexture};
use crate::cockpit::{CockpitArt, Displays};
use crate::font::Fonts;
use crate::mfd::{CLASSIC, HIRES, MfdSide, osb};
use crate::panel::{BUTTON, LAMPS, LampId, draw_gauge, draw_label, draw_lamp, draw_led, draw_led_bars};

/// How far the MFD screens sit behind the panel face (metres).
const SCREEN_DEPTH: f32 = 0.003;
/// Distance of the HUD combiner glass from the eye (metres).
const COMBINER_DEPTH: f32 = 0.80;
/// Outline of the combiner glass in original screen pixels: the original's HUD frame
/// (`hud::instruments`), posts at columns 45 and 275 and a bevelled top.
const COMBINER: [(f32, f32); 6] = [(45.0, 90.0), (45.0, 14.0), (89.0, 2.0), (232.0, 2.0), (275.0, 14.0), (275.0, 90.0)];
/// Emissive strength of the displays and lit lights when the cockpit is lit.
const GLOW_NITS: f32 = 1.0;
/// Instrument atlas texels per panel pixel (down; see `texels`), classic and modern panel.
pub(super) const TEXELS: i32 = 3;
const MODERN_TEXELS: i32 = 4;
/// Largest label text scale (atlas texels per font pixel), so the labels match in size.
const TEXT_SCALE: i32 = 3;
/// Instrument atlas size (texels).
const ATLAS: usize = 512;
/// Faces stand this far above their surface (metres).
const FACE_LIFT: f32 = 0.0006;

/// A cockpit material whose `unlit` follows the lighting options (see `sync_materials`).
#[derive(Component)]
pub struct LitMaterial(pub Handle<StandardMaterial>);

/// What a canvas shows (`update_displays` draws them).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CanvasRole {
    Atlas,
    Glow,
    Left,
    Right,
    Adi,
    Hud,
    Approach,
}

/// A face in the instrument atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Face {
    Item(Elem),
    /// A lamp that is a button cap on the modern panel (green, `panel::BUTTON`).
    Button(LampId, Option<&'static str>),
    /// The EMV gauge as LED bar graphs on the modern panel (`panel::draw_led_bars`).
    LedBars,
    Osb(MfdSide, u8),
}

/// The instrument atlas' faces and their texel rectangles (x, y, w, h).
#[derive(Component)]
pub(super) struct InstrumentAtlas {
    faces: Vec<(Face, [i32; 4])>,
    /// Texels per panel pixel.
    texels: i32,
}

#[derive(Component)]
pub(super) struct TrackCamScreen;

/// Atlas material and texture rectangles (uv min, max) of the faces the controls carry.
pub(super) struct Faces {
    pub material: Handle<StandardMaterial>,
    /// Per layout item.
    pub items: Vec<(Vec2, Vec2)>,
    pub osbs: HashMap<(u8, u8), (Vec2, Vec2)>,
}

/// Key of an MFD side for `Faces::osbs`.
pub(super) fn side_key(s: MfdSide) -> u8 {
    match s {
        MfdSide::Left => 0,
        MfdSide::Right => 1,
    }
}

/// Packs rectangles into the atlas in rows.
fn pack(sizes: &[(i32, i32)]) -> Vec<[i32; 4]> {
    let (mut x, mut y, mut row) = (0, 0, 0);
    let mut out = Vec::new();
    for &(w, h) in sizes {
        if x + w > ATLAS as i32 {
            x = 0;
            y += row + 1;
            row = 0;
        }
        assert!(y + h <= ATLAS as i32, "instrument atlas full");
        out.push([x, y, w, h]);
        x += w + 1;
        row = row.max(h);
    }
    out
}

/// Atlas size of a face: `k` texels per panel pixel down, and across either the same (the
/// classic panel keeps the original's tall pixels) or as many as make the texels square on the
/// panel.
fn texels(r: &PxRect, k: i32, square: bool) -> (i32, i32) {
    let h = (r.h * k as f32).ceil();
    if !square {
        return ((r.w * k as f32).ceil() as i32, h as i32);
    }
    let (_, size) = panel_rect(r.x, r.y, r.w, r.h, 0.0);
    ((h * size.x / size.y).round() as i32, h as i32)
}

/// Yellow and black diagonal stripes.
fn hazard(c: &mut Canvas, r: [i32; 4], k: i32) {
    let band = 4 * k;
    for y in 0..r[3] {
        for x in 0..r[2] {
            let colour = if (x + y) / band % 2 == 0 { 18 } else { 38 };
            c.set(r[0] + x, r[1] + y, colour);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_panel(
    commands: &mut Commands,
    root: Entity,
    layout: &PanelLayout,
    mats: &CockpitMaterials,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    tc_image: Option<&crate::trackcam::TrackCamImage>,
) -> Faces {
    let add = |commands: &mut Commands, mesh: Handle<Mesh>, material: Handle<StandardMaterial>, tf: Transform| {
        let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), tf, NoFrustumCulling)).id();
        commands.entity(root).add_child(e);
        e
    };

    // Palette swatch for the solid parts.
    let mut swatch = Canvas::new(16, 16);
    for (i, p) in swatch.px.iter_mut().enumerate() {
        *p = i as u8;
    }
    let swatch = CanvasTexture::new(swatch, images, false);
    let solid = materials.add(StandardMaterial {
        base_color_texture: Some(swatch.image.clone()),
        perceptual_roughness: 0.8,
        cull_mode: None,
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let e = commands.spawn((swatch, LitMaterial(solid.clone()))).id();
    commands.entity(root).add_child(e);

    // Panel face: the top edge's trapezoid, the rest around the MFD screens.
    let holes: Vec<[f32; 4]> = layout.mfds.iter().map(|m| [m.screen.x, m.screen.y, m.screen.x + m.screen.w, m.screen.y + m.screen.h]).collect();
    let top = with_swatch(panel_polygon(&OUTLINE[..4], 0.0), layout.face);
    add(commands, meshes.add(top), solid.clone(), Transform::default());
    for r in rect_minus_holes(0.0, OUTLINE[0].1, 320.0, 200.0, &holes) {
        let quad = panel_polygon(&[(r[0], r[1]), (r[2], r[1]), (r[2], r[3]), (r[0], r[3])], 0.0);
        add(commands, meshes.add(with_swatch(quad, layout.face)), solid.clone(), Transform::default());
    }

    // Solid parts.
    let mut knobs: HashMap<u8, Handle<Mesh>> = HashMap::new();
    for b in &layout.blocks {
        let (cx, cy) = b.rect.centre();
        let (_, size) = panel_rect(b.rect.x, b.rect.y, b.rect.w, b.rect.h, b.base);
        let centre = panel_point(cx, cy, b.base + b.height / 2.0);
        let (mesh, tf) = match b.shape {
            Shape::Box => (
                meshes.add(with_swatch(panel_box(b.rect.x, b.rect.y, b.rect.w, b.rect.h, b.base, b.height), b.colour)),
                Transform::default(),
            ),
            Shape::Knob => (
                knobs.entry(b.colour).or_insert_with(|| meshes.add(with_swatch(Cylinder::new(0.5, 1.0).into(), b.colour))).clone(),
                Transform::from_translation(centre)
                    .with_rotation(panel_rotation() * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::new(size.x, b.height, size.x)),
            ),
        };
        add(commands, mesh, solid.clone(), tf);
    }

    // Instrument atlas: every lamp, readout, label and bezel button face.
    let modern = layout.style == PanelStyle::Modern;
    let face_of = |p: &Placed| match p.elem {
        Elem::Lamp(id, label) if modern && p.cap.is_some() => Face::Button(id, label),
        Elem::Gauge if modern => Face::LedBars,
        elem => Face::Item(elem),
    };
    let mut faces: Vec<(Face, PxRect)> = layout.items.iter().map(|p| (face_of(p), p.rect)).collect();
    for m in &layout.mfds {
        faces.extend(layout.osbs(m).into_iter().map(|(slot, r)| (Face::Osb(m.side, slot), r)));
    }
    let k = if modern { MODERN_TEXELS } else { TEXELS };
    let slots = pack(&faces.iter().map(|(_, r)| texels(r, k, modern)).collect::<Vec<_>>());
    let uv = |s: &[i32; 4]| {
        let n = ATLAS as f32;
        (Vec2::new(s[0] as f32 / n, s[1] as f32 / n), Vec2::new((s[0] + s[2]) as f32 / n, (s[1] + s[3]) as f32 / n))
    };
    let atlas_tex = CanvasTexture::new(Canvas::new(ATLAS, ATLAS), images, false);
    let glow_tex = CanvasTexture::new(Canvas::new(ATLAS, ATLAS), images, false);
    let atlas_mat = materials.add(StandardMaterial {
        base_color_texture: Some(atlas_tex.image.clone()),
        emissive_texture: Some(glow_tex.image.clone()),
        emissive: LinearRgba::rgb(GLOW_NITS, GLOW_NITS, GLOW_NITS),
        perceptual_roughness: 0.6,
        cull_mode: None,
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let atlas = InstrumentAtlas { faces: faces.iter().zip(&slots).map(|((f, _), s)| (*f, *s)).collect(), texels: k };
    let e = commands.spawn((CanvasRole::Atlas, atlas, atlas_tex, LitMaterial(atlas_mat.clone()))).id();
    commands.entity(root).add_child(e);
    let e = commands.spawn((CanvasRole::Glow, glow_tex)).id();
    commands.entity(root).add_child(e);
    let mut out = Faces { material: atlas_mat.clone(), items: Vec::new(), osbs: HashMap::new() };
    for ((face, _), slot) in faces.iter().zip(&slots) {
        let (uv0, uv1) = uv(slot);
        match face {
            Face::Item(_) | Face::Button(..) | Face::LedBars => out.items.push((uv0, uv1)),
            Face::Osb(side, s) => {
                out.osbs.insert((side_key(*side), *s), (uv0, uv1));
            }
        }
    }
    // Faces that are not button caps sit on the panel.
    for (p, &(uv0, uv1)) in layout.items.iter().zip(&out.items) {
        if p.cap.is_some() {
            continue;
        }
        let (cx, cy) = p.rect.centre();
        let (_, size) = panel_rect(p.rect.x, p.rect.y, p.rect.w, p.rect.h, 0.0);
        let tf = Transform::from_translation(panel_point(cx, cy, layout.surface(cx, cy) + FACE_LIFT)).with_rotation(panel_rotation());
        let e = add(commands, meshes.add(uv_quad(size, uv0, uv1)), atlas_mat.clone(), tf);
        commands.entity(e).insert(NotShadowCaster);
    }

    // MFD screens recessed into the panel, each with its own canvas.
    let res = if layout.style == PanelStyle::Classic { CLASSIC } else { HIRES };
    for m in &layout.mfds {
        let s = m.screen;
        let (x0, y0, x1, y1) = (s.x, s.y, s.x + s.w, s.y + s.h);
        let tex = CanvasTexture::new(Canvas::new(res.0, res.1), images, false);
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(tex.image.clone()),
            emissive_texture: Some(tex.image.clone()),
            emissive: LinearRgba::rgb(GLOW_NITS, GLOW_NITS, GLOW_NITS),
            alpha_mode: AlphaMode::Mask(0.5),
            perceptual_roughness: 0.3,
            cull_mode: None,
            unlit: true,
            fog_enabled: false,
            ..default()
        });
        let role = if m.side == MfdSide::Left { CanvasRole::Left } else { CanvasRole::Right };
        let mesh = panel_patch_uv(x0, y0, x1, y1, -SCREEN_DEPTH, |px, py| Vec2::new((px - x0) / (x1 - x0), (py - y0) / (y1 - y0)));
        let e = add(commands, meshes.add(mesh), material, Transform::default());
        // Self-luminous: always unlit.
        commands.entity(e).insert((role, tex));
        add(commands, meshes.add(recess_walls(x0, y0, x1, y1, SCREEN_DEPTH)), mats.black.clone(), Transform::default());
        // Tracking camera picture behind the right screen (seen where the page is see-through).
        if m.side == MfdSide::Right
            && let Some(img) = tc_image
        {
            let tc = materials.add(StandardMaterial {
                base_color_texture: Some(img.0.clone()),
                unlit: true,
                cull_mode: None,
                fog_enabled: false,
                ..default()
            });
            let mesh = panel_patch_uv(x0, y0, x1, y1, -SCREEN_DEPTH - 0.001, |px, py| Vec2::new((px - x0) / (x1 - x0), (py - y0) / (y1 - y0)));
            let e = add(commands, meshes.add(mesh), tc, Transform::default());
            commands.entity(e).insert((TrackCamScreen, Visibility::Hidden));
        }
    }

    // Attitude ball: its own canvas in a ring bezel, round on the panel (`adi` is as wide as
    // it is high in panel pixels, which are taller than wide).
    let a = layout.adi;
    let (cx, cy) = a.centre();
    let n = (a.w * TEXELS as f32) as usize;
    let tex = CanvasTexture::new(Canvas::new(n, n), images, true);
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(tex.image.clone()),
        emissive_texture: Some(tex.image.clone()),
        emissive: LinearRgba::rgb(GLOW_NITS, GLOW_NITS, GLOW_NITS),
        alpha_mode: AlphaMode::Mask(0.5),
        cull_mode: None,
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let size = Vec2::splat(panel_rect(a.x, a.y, a.w, a.h, 0.0).1.x);
    let base = layout.surface(cx, cy);
    let tf = Transform::from_translation(panel_point(cx, cy, base + FACE_LIFT)).with_rotation(panel_rotation());
    let e = add(commands, meshes.add(uv_quad(size, Vec2::ZERO, Vec2::ONE)), material, tf);
    commands.entity(e).insert((CanvasRole::Adi, tex));
    let r = size.x / 2.0;
    let ring = with_swatch(Annulus::new(r * 0.98, r * 1.2).into(), 79);
    let tf = Transform::from_translation(panel_point(cx, cy, base + 0.002)).with_rotation(panel_rotation());
    add(commands, meshes.add(ring), solid, tf);

    out
}

/// HUD: a transparent canvas seen through the combiner glass, collimated to infinity.
pub(super) fn spawn_hud(commands: &mut Commands, root: Entity, meshes: &mut Assets<Mesh>, materials: &mut Assets<HudGlassMaterial>, images: &mut Assets<Image>) {
    let hud_tex = CanvasTexture::new(Canvas::new(320, 200), images, true);
    let material = materials.add(HudGlassMaterial { tint: Vec4::ZERO, image: hud_tex.image.clone() });
    // Flat glass in the plane z = -COMBINER_DEPTH (a fan around the centre).
    let corners: Vec<Vec3> = COMBINER.iter().map(|&(x, y)| pixel_dir(x, y) * COMBINER_DEPTH).collect();
    let centre = corners.iter().sum::<Vec3>() / corners.len() as f32;
    let mut pos = vec![centre.to_array()];
    pos.extend(corners.iter().map(|c| c.to_array()));
    let n = corners.len() as u32;
    let idx: Vec<u32> = (0..n).flat_map(|k| [0, 1 + k, 1 + (k + 1) % n]).collect();
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_indices(Indices::U32(idx));
    let hud = commands
        .spawn((CanvasRole::Hud, hud_tex, Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Transform::default(), NoFrustumCulling, NotShadowCaster))
        .id();
    commands.entity(root).add_child(hud);
}

/// Approach indexer beside the left combiner post (where the original draws it, pixels
/// (33, 70) 10x21).
pub(super) fn spawn_approach_indexer(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let tex = CanvasTexture::new(Canvas::new(10, 21), images, true);
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(tex.image.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Mask(0.5),
        cull_mode: None,
        fog_enabled: false,
        ..default()
    });
    let size = Vec2::new(10.0 / crate::cockpit::FOCAL, 21.0 / (crate::cockpit::VSCALE * crate::cockpit::FOCAL)) * COMBINER_DEPTH;
    let e = commands
        .spawn((
            CanvasRole::Approach,
            tex,
            Mesh3d(meshes.add(uv_quad(size, Vec2::ZERO, Vec2::ONE))),
            MeshMaterial3d(material),
            Transform::from_translation(pixel_dir(38.0, 80.5) * COMBINER_DEPTH),
            NoFrustumCulling,
            NotShadowCaster,
        ))
        .id();
    commands.entity(root).add_child(e);
}

/// Glare shield, combiner, canopy frame, consoles, seat and the body around the pilot.
pub(super) fn spawn_structure(commands: &mut Commands, root: Entity, mats: &CockpitMaterials, meshes: &mut Assets<Mesh>) {
    let b = |commands: &mut Commands, m: &Handle<StandardMaterial>, c: Vec3, s: Vec3| {
        spawn_box(commands, root, mats, m, Transform::from_translation(c).with_scale(s))
    };
    let beam_box = |commands: &mut Commands, m: &Handle<StandardMaterial>, a: Vec3, z: Vec3, t: f32| {
        spawn_box(commands, root, mats, m, beam(a, z, Vec2::splat(t)))
    };

    // Glare shield over the centre of the panel (art columns 64..256 reach up to row 91),
    // wide enough to carry the combiner posts.
    let (el, er) = (panel_point(36.0, 91.0, 0.0), panel_point(284.0, 91.0, 0.0));
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
    // Coaming along the glare shield edge (kept low: the HUD weapon line sits just above it).
    spawn_box(commands, root, mats, &mats.rubber, beam(el + Vec3::new(0.0, -0.006, 0.0), er + Vec3::new(0.0, -0.006, 0.0), Vec2::new(0.025, 0.02)));
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

    // HUD combiner frame around the glass, on two posts down into the glare shield.
    let c: Vec<Vec3> = COMBINER.iter().map(|&(x, y)| pixel_dir(x, y) * COMBINER_DEPTH).collect();
    let post = Vec3::new(0.0, 0.05, 0.0);
    beam_box(commands, &mats.frame, c[1], c[0] - post, 0.012);
    beam_box(commands, &mats.frame, c[4], c[5] - post, 0.012);
    for k in 1..4 {
        beam_box(commands, &mats.frame, c[k], c[k + 1], 0.01);
    }

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

/// Draws the instrument atlas, the MFD, attitude ball, HUD and approach indexer canvases.
pub(super) fn update_displays(
    art: Res<CockpitArt>,
    fonts: Res<Fonts>,
    d: Displays,
    roles: Query<(Entity, &CanvasRole)>,
    mut texes: Query<&mut CanvasTexture>,
    atlas: Query<&InstrumentAtlas>,
    mut tc: Query<&mut Visibility, With<TrackCamScreen>>,
) {
    let find = |r: CanvasRole| roles.iter().find(|(_, x)| **x == r).map(|(e, _)| e);
    let see_through = d.mfd.camera_see_through();
    for mut v in &mut tc {
        v.set_if_neq(if see_through { Visibility::Inherited } else { Visibility::Hidden });
    }
    for role in [CanvasRole::Left, CanvasRole::Right, CanvasRole::Adi, CanvasRole::Hud, CanvasRole::Approach] {
        let Some(mut tex) = find(role).and_then(|e| texes.get_mut(e).ok()) else { continue };
        let tex = tex.as_mut();
        tex.canvas.reset_clip();
        match role {
            CanvasRole::Left => d.mfd.draw_left(&mut tex.canvas, &art, &fonts),
            CanvasRole::Right => {
                tex.transparent = see_through;
                d.mfd.draw_right(&mut tex.canvas, &art, &fonts);
            }
            CanvasRole::Adi => d.panel.draw_adi(&mut tex.canvas),
            CanvasRole::Hud => {
                tex.canvas.px.fill(0);
                if let Some(frame) = d.hud.frame(d.mfd.state.right == crate::mfd::RightMfd::Camera) {
                    crate::hud::draw(&mut tex.canvas, &art, &fonts, &frame);
                }
            }
            _ => d.panel.draw_approach(&mut tex.canvas, &art),
        }
        tex.dirty = true;
    }

    let (Some(a), Some(g)) = (find(CanvasRole::Atlas), find(CanvasRole::Glow)) else { return };
    let Ok(info) = atlas.get(a) else { return };
    let Ok([mut a, mut g]) = texes.get_many_mut([a, g]) else { return };
    let (c, glow) = (&mut a.canvas, &mut g.canvas);
    glow.px.fill(0);
    let lit = d.panel.lamp_states();
    let (emv, egr, ear) = d.panel.gauge_values();
    let (k, t) = (info.texels, TEXT_SCALE);
    for &(face, r) in &info.faces {
        let glows = match face {
            Face::Item(Elem::Lamp(id, label)) => {
                let on = lit[id as usize];
                let style = LAMPS[id as usize].2;
                draw_lamp(c, &fonts, r, label.unwrap_or_else(|| d.panel.lamp_label(id)), if on { style.on } else { style.off }, k, t);
                on
            }
            Face::Button(id, label) => {
                let on = lit[id as usize];
                draw_lamp(c, &fonts, r, label.unwrap_or_else(|| d.panel.lamp_label(id)), if on { BUTTON.on } else { BUTTON.off }, k, t);
                on
            }
            Face::Item(Elem::Led(id)) => {
                draw_led(c, &art.sprites, r, &d.panel.led_text(id), k);
                true
            }
            Face::Item(Elem::Gauge) => {
                draw_gauge(c, r, emv, egr, ear);
                true
            }
            Face::LedBars => {
                draw_led_bars(c, glow, r, emv, egr, ear);
                false
            }
            Face::Item(Elem::Label(text, fg, bg)) => {
                draw_label(c, &fonts, r, text, fg, bg, t);
                true
            }
            Face::Item(Elem::Hazard) => {
                hazard(c, r, k);
                false
            }
            Face::Osb(side, slot) => match osb(side, slot, &d.mfd.state) {
                Some((label, _, true)) => {
                    draw_lamp(c, &fonts, r, label, (38, 47), k, t);
                    true
                }
                // Unlit: light grey label on a dark cap.
                Some((label, _, false)) => {
                    draw_lamp(c, &fonts, r, "", (35, 38), k, t);
                    draw_label(c, &fonts, [r[0] + k, r[1] + k, r[2] - 2 * k, r[3] - 2 * k], label, 7, 38, t);
                    false
                }
                None => {
                    draw_lamp(c, &fonts, r, "", (35, 38), k, t);
                    false
                }
            },
        };
        if glows {
            glow.copy_rect(c, r[0], r[1], r[2], r[3], r[0], r[1]);
        }
    }
    a.dirty = true;
    g.dirty = true;
}
