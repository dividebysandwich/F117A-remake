//! Instrument panel, MFD screens, HUD and the fixed cockpit structure.
//!
//! The panel is modelled from a `layout::PanelLayout`: the face and solid parts take their
//! colours from a 16x16 palette swatch texture (so they follow the flight palette); lamp,
//! LED, gauge, label and bezel button faces are slots of one instrument atlas canvas redrawn
//! every frame (with a glow twin for the lit ones); each MFD and the attitude ball have their
//! own canvas.

use std::collections::HashMap;

use bevy::prelude::*;

use super::hud_glass::HudGlassMaterial;
use super::layout::{DED_CANVAS, Elem, PanelLayout, PanelStyle, Placed, PxRect};
use super::shading::{CockpitMaterial, cockpit_material, cockpit_material_bright};
use crate::canvas::{Canvas, CanvasTexture};
use crate::cockpit::{CockpitArt, Displays};
use crate::font::Fonts;
use crate::mfd::{CLASSIC, HIRES, MfdSide, osb};
use crate::panel::{
    BUTTON, GREEN, LAMPS, LampId, draw_gauge, draw_label, draw_lamp, draw_led, draw_led_bars,
};

/// Emissive strength of the displays and lit lights when the cockpit is lit.
const GLOW_NITS: f32 = 1.0;
/// Instrument atlas texels per panel pixel, classic panel.
pub(super) const TEXELS: i32 = 3;
/// Instrument atlas texels per millimetre on the F-16 and F-117A panels (their large round
/// instruments would not fit the atlas at 3; 2 is still more than a VR pixel per texel at the
/// panel).
const MM_TEXELS: i32 = 2;
/// Largest label text scale (atlas texels per font pixel), so the labels match in size.
const TEXT_SCALE: i32 = 3;
/// Instrument atlas size (texels).
const ATLAS: usize = 512;

/// A cockpit material whose `unlit` follows the lighting options (see `sync_materials`).
#[derive(Component)]
pub struct LitMaterial(pub Handle<CockpitMaterial>);

/// What a canvas shows (`update_displays` draws them).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CanvasRole {
    Atlas,
    Glow,
    Left,
    Right,
    Third,
    Adi,
    Hud,
    Approach,
    Rwr,
    Hsi,
    Ded,
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

/// The tracking camera picture behind an MFD screen.
#[derive(Component)]
pub(super) struct TrackCamScreen(pub MfdSide);

/// Texels between atlas faces, so the atlas' mip levels (`ATLAS_MIPS`) don't blend neighbours.
const ATLAS_GAP: i32 = 4;
/// Mip levels of the instrument atlas: the full size, half and quarter.
const ATLAS_MIPS: usize = 3;

/// Packs rectangles into the atlas in rows, tallest first (rows of similar heights waste
/// little); the slots come back in the order of `sizes`.
fn pack(sizes: &[(i32, i32)]) -> Vec<[i32; 4]> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
    let (mut x, mut y, mut row) = (0, 0, 0);
    let mut out = vec![[0; 4]; sizes.len()];
    for i in order {
        let (w, h) = sizes[i];
        if x + w > ATLAS as i32 {
            x = 0;
            y += row + ATLAS_GAP;
            row = 0;
        }
        assert!(y + h <= ATLAS as i32, "instrument atlas full");
        out[i] = [x, y, w, h];
        x += w + ATLAS_GAP;
        row = row.max(h);
    }
    out
}

/// Atlas size of a face: `k` texels per layout unit (the classic panel keeps its tall 320x200
/// pixels).
fn texels(r: &PxRect, k: i32) -> (i32, i32) {
    ((r.w * k as f32).ceil() as i32, (r.h * k as f32).ceil() as i32)
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

/// The gear lights face: an aircraft seen from above, a lamp at the nose wheel and at each main
/// wheel, green with the gear down.
fn gear_lights(c: &mut Canvas, r: [i32; 4], down: bool) {
    let [x, y, w, h] = r;
    c.fill_rect(x, y, w, h, 38);
    let (w, h) = (w as f32, h as f32);
    let p = |u: f32, v: f32| Vec2::new(x as f32 + u * w, y as f32 + v * h);
    let pen = (h / 40.0).max(1.0);
    let outline = [
        p(0.5, 0.06),
        p(0.56, 0.3),
        p(0.95, 0.62),
        p(0.95, 0.7),
        p(0.57, 0.62),
        p(0.56, 0.82),
        p(0.7, 0.94),
        p(0.3, 0.94),
        p(0.44, 0.82),
        p(0.43, 0.62),
        p(0.05, 0.7),
        p(0.05, 0.62),
        p(0.44, 0.3),
        p(0.5, 0.06),
    ];
    c.polyline(&outline, pen, 7);
    let lamp = if down { 47 } else { 44 };
    let s = (h * 0.13).max(2.0);
    for (u, v) in [(0.5, 0.3), (0.32, 0.6), (0.68, 0.6)] {
        let q = p(u, v);
        c.fill_rect((q.x - s) as i32, (q.y - s) as i32, (2.0 * s) as i32, (2.0 * s) as i32, lamp);
    }
}

/// The panel's run-time materials: the palette swatch for the solid parts, and the instrument
/// atlas (with its glow twin) holding every lamp, readout, label and bezel button face.
pub(super) struct PanelMaterials {
    pub solid: Handle<CockpitMaterial>,
    pub atlas: Handle<CockpitMaterial>,
    /// Atlas rectangle (uv min, max) of each face, by face key (`Face`'s `Debug` text).
    pub faces: HashMap<String, (Vec2, Vec2)>,
}

/// Creates the swatch and atlas canvases (children of the cockpit root) and their materials.
pub(super) fn panel_materials(
    commands: &mut Commands,
    root: Entity,
    layout: &PanelLayout,
    materials: &mut Assets<CockpitMaterial>,
    images: &mut Assets<Image>,
) -> PanelMaterials {
    // Palette swatch for the solid parts (their UVs point at one cell each).
    let mut swatch = Canvas::new(16, 16);
    for (i, p) in swatch.px.iter_mut().enumerate() {
        *p = i as u8;
    }
    let swatch = CanvasTexture::new(swatch, images, false);
    let solid = materials.add(cockpit_material(StandardMaterial {
        base_color_texture: Some(swatch.image.clone()),
        perceptual_roughness: 0.8,
        cull_mode: None,
        unlit: true,
        fog_enabled: false,
        ..default()
    }));
    let e = commands.spawn((swatch, LitMaterial(solid.clone()))).id();
    commands.entity(root).add_child(e);

    // Instrument atlas: every lamp, readout, label and bezel button face.
    let faces = atlas_faces(layout);
    let k = atlas_scale(layout.style);
    let slots = pack(&faces.iter().map(|(_, r)| texels(r, k)).collect::<Vec<_>>());
    let uv = |s: &[i32; 4]| {
        let n = ATLAS as f32;
        (
            Vec2::new(s[0] as f32 / n, s[1] as f32 / n),
            Vec2::new((s[0] + s[2]) as f32 / n, (s[1] + s[3]) as f32 / n),
        )
    };
    let atlas_tex =
        CanvasTexture::mipmapped_levels(Canvas::new(ATLAS, ATLAS), images, false, ATLAS_MIPS);
    let glow_tex =
        CanvasTexture::mipmapped_levels(Canvas::new(ATLAS, ATLAS), images, false, ATLAS_MIPS);
    let atlas = materials.add(cockpit_material(StandardMaterial {
        base_color_texture: Some(atlas_tex.image.clone()),
        emissive_texture: Some(glow_tex.image.clone()),
        emissive: LinearRgba::rgb(GLOW_NITS, GLOW_NITS, GLOW_NITS),
        perceptual_roughness: 0.6,
        cull_mode: None,
        unlit: true,
        fog_enabled: false,
        ..default()
    }));
    let info = InstrumentAtlas {
        faces: faces.iter().zip(&slots).map(|((f, _), s)| (*f, *s)).collect(),
        texels: k,
    };
    let e = commands.spawn((CanvasRole::Atlas, info, atlas_tex, LitMaterial(atlas.clone()))).id();
    commands.entity(root).add_child(e);
    let e = commands.spawn((CanvasRole::Glow, glow_tex)).id();
    commands.entity(root).add_child(e);
    let faces = faces.iter().zip(&slots).map(|((f, _), s)| (format!("{f:?}"), uv(s))).collect();
    PanelMaterials { solid, atlas, faces }
}

/// Atlas texels per layout unit.
fn atlas_scale(style: PanelStyle) -> i32 {
    match style {
        PanelStyle::Classic => TEXELS,
        PanelStyle::Modern | PanelStyle::F16 => MM_TEXELS,
    }
}

/// The faces of the instrument atlas and their panel rectangles.
pub(super) fn atlas_faces(layout: &PanelLayout) -> Vec<(Face, PxRect)> {
    let modern = layout.style != PanelStyle::Classic;
    let face_of = |p: &Placed| match p.elem {
        Elem::Lamp(id, label) if modern && p.cap.is_some() => Face::Button(id, label),
        Elem::Gauge if modern => Face::LedBars,
        elem => Face::Item(elem),
    };
    let mut faces: Vec<(Face, PxRect)> =
        layout.items.iter().map(|p| (face_of(p), p.rect)).collect();
    for m in &layout.mfds {
        faces.extend(layout.osbs(m).into_iter().map(|(slot, r)| (Face::Osb(m.side, slot), r)));
    }
    faces
}

/// A self-luminous screen: an MFD (`Left` / `Right` / `Third`), the attitude ball, the RWR scope, the HSI
/// or the approach indexer, with its canvas.
pub(super) fn screen(
    role: CanvasRole,
    layout: &PanelLayout,
    hires: bool,
    materials: &mut Assets<CockpitMaterial>,
    images: &mut Assets<Image>,
) -> (CanvasTexture, Handle<CockpitMaterial>) {
    let (canvas, transparent) = match role {
        CanvasRole::Left | CanvasRole::Right | CanvasRole::Third if hires => {
            (Canvas::scaled(HIRES.0, HIRES.1, crate::mfd::HIRES_SCALE), false)
        }
        CanvasRole::Left | CanvasRole::Right | CanvasRole::Third => {
            (Canvas::new(CLASSIC.0, CLASSIC.1), false)
        }
        CanvasRole::Adi | CanvasRole::Rwr | CanvasRole::Hsi => {
            let r = match role {
                CanvasRole::Rwr => layout.rwr,
                CanvasRole::Hsi => layout.hsi,
                _ => Some(layout.adi),
            };
            let n = (r.map_or(90.0, |r| r.w) * TEXELS as f32) as usize;
            (Canvas::new(n, n), true)
        }
        // The classic cockpit has no data entry display.
        CanvasRole::Ded => (
            if layout.ded.is_some() {
                Canvas::new(DED_CANVAS.0, DED_CANVAS.1)
            } else {
                Canvas::new(8, 8)
            },
            true,
        ),
        _ => (Canvas::new(crate::panel::INDEXER_CANVAS.0, crate::panel::INDEXER_CANVAS.1), true),
    };
    let tex = CanvasTexture::mipmapped(canvas, images, transparent);
    let material = materials.add(cockpit_material(StandardMaterial {
        base_color_texture: Some(tex.image.clone()),
        emissive_texture: Some(tex.image.clone()),
        emissive: LinearRgba::rgb(GLOW_NITS, GLOW_NITS, GLOW_NITS),
        alpha_mode: AlphaMode::Mask(0.5),
        perceptual_roughness: 0.3,
        cull_mode: None,
        unlit: true,
        fog_enabled: false,
        ..default()
    }));
    (tex, material)
}

/// The tracking camera picture behind the right screen (seen where its page is see-through).
pub(super) fn trackcam_material(
    materials: &mut Assets<CockpitMaterial>,
    tc_image: Option<&crate::trackcam::TrackCamImage>,
) -> Handle<CockpitMaterial> {
    materials.add(cockpit_material_bright(StandardMaterial {
        base_color_texture: tc_image.map(|i| i.0.clone()),
        unlit: true,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    }))
}

/// A side panel button label (the button's text on a green cap).
pub(super) fn side_label(
    text: &str,
    fonts: &Fonts,
    materials: &mut Assets<CockpitMaterial>,
    images: &mut Assets<Image>,
) -> (CanvasTexture, Handle<CockpitMaterial>) {
    let mut canvas = Canvas::new(28, 11);
    canvas.clear(44);
    let w = fonts.text(&mut Canvas::new(64, 16), 0, 0, text.as_bytes(), 47);
    fonts.text(&mut canvas, (28 - w) / 2, 3, text.as_bytes(), 47);
    let tex = CanvasTexture::mipmapped(canvas, images, false);
    let m = materials.add(cockpit_material(StandardMaterial {
        base_color_texture: Some(tex.image.clone()),
        emissive_texture: Some(tex.image.clone()),
        emissive: LinearRgba::rgb(1.0, 1.0, 1.0),
        unlit: true,
        fog_enabled: false,
        ..default()
    }));
    (tex, m)
}

/// The HUD combiner glass material and canvas.
pub(super) fn hud_material(
    materials: &mut Assets<HudGlassMaterial>,
    images: &mut Assets<Image>,
    classic: bool,
    scale: f32,
) -> (CanvasTexture, Handle<HudGlassMaterial>) {
    let (tex, proj, style) = hud_canvas(images, classic, scale);
    let material =
        materials.add(HudGlassMaterial { tint: Vec4::ZERO, image: tex.image.clone(), proj, style });
    (tex, material)
}

/// The HUD canvas for the classic (320x200 palette picture) or the modern HUD (large
/// monochrome mask, filtered), with the glass projection and style.
fn hud_canvas(
    images: &mut Assets<Image>,
    classic: bool,
    scale: f32,
) -> (CanvasTexture, Vec4, Vec4) {
    let ((w, h), proj, style) = HudGlassMaterial::layout(classic, scale);
    let tex = CanvasTexture::new(Canvas::new(w, h), images, true);
    if !classic && let Some(mut img) = images.get_mut(&tex.image) {
        img.sampler = bevy::image::ImageSampler::linear();
    }
    (tex, proj, style)
}

/// Switches the HUD canvas between the classic and the modern HUD when the setting changes.
pub(super) fn restyle_hud(
    settings: Res<crate::settings::Settings>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<HudGlassMaterial>>,
    mut q: Query<(&CanvasRole, &mut CanvasTexture, &MeshMaterial3d<HudGlassMaterial>)>,
) {
    if !settings.is_changed() {
        return;
    }
    let classic = settings.classic_hud;
    for (role, mut tex, mat) in &mut q {
        if *role != CanvasRole::Hud || (tex.canvas.w == 320) == classic {
            continue;
        }
        let (new, proj, style) = hud_canvas(&mut images, classic, settings.hud_scale());
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.image = new.image.clone();
            m.proj = proj;
            m.style = style;
        }
        *tex = new;
    }
}

/// Draws the instrument atlas, the MFD, attitude ball, HUD and approach indexer canvases.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_displays(
    art: Res<CockpitArt>,
    fonts: Res<Fonts>,
    d: Displays,
    roles: Query<(Entity, &CanvasRole)>,
    mut texes: Query<&mut CanvasTexture>,
    atlas: Query<&InstrumentAtlas>,
    mut tc: Query<(&TrackCamScreen, &mut Visibility)>,
    settings: Res<crate::settings::Settings>,
    sky: Res<crate::sky::Sky>,
    palette: Option<Res<crate::palette::Palette>>,
) {
    // Instrument lighting: with continuous time of day the dials and the unlit green lamps and
    // buttons glow once it is night (the darkness of a moonless night leaves them lit).
    let backlit = sky.on && palette.is_some_and(|p| p.lights_on());
    let find = |r: CanvasRole| roles.iter().find(|(_, x)| **x == r).map(|(e, _)| e);
    for (screen, mut v) in &mut tc {
        v.set_if_neq(if d.mfd.camera_see_through(screen.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for role in [
        CanvasRole::Left,
        CanvasRole::Right,
        CanvasRole::Third,
        CanvasRole::Adi,
        CanvasRole::Hud,
        CanvasRole::Approach,
        CanvasRole::Rwr,
        CanvasRole::Hsi,
        CanvasRole::Ded,
    ] {
        let Some(mut tex) = find(role).and_then(|e| texes.get_mut(e).ok()) else { continue };
        let tex = tex.as_mut();
        tex.canvas.reset_clip();
        match role {
            CanvasRole::Left => {
                tex.transparent = d.mfd.camera_see_through(MfdSide::Left);
                d.mfd.draw_left(&mut tex.canvas, &art, &fonts);
            }
            CanvasRole::Right => {
                tex.transparent = d.mfd.camera_see_through(MfdSide::Right);
                d.mfd.draw_right(&mut tex.canvas, &art, &fonts);
            }
            CanvasRole::Third => {
                tex.transparent = d.mfd.camera_see_through(MfdSide::Third);
                d.mfd.draw_third(&mut tex.canvas, &art, &fonts);
            }
            CanvasRole::Adi => {
                d.panel.draw_adi(&mut tex.canvas, settings.cockpit().0 != PanelStyle::Classic)
            }
            CanvasRole::Rwr => d.panel.draw_rwr(&mut tex.canvas),
            CanvasRole::Hsi => d.panel.draw_hsi(&mut tex.canvas),
            CanvasRole::Ded => d.panel.draw_ded(&mut tex.canvas, &fonts),
            CanvasRole::Hud => {
                tex.canvas.px.fill(0);
                if let Some(frame) = d.hud.frame(d.mfd.state.camera_shown()) {
                    if tex.canvas.w == 320 {
                        crate::hud::draw(&mut tex.canvas, &art, &fonts, &frame);
                    } else {
                        crate::hud::modern::draw(&mut tex.canvas, &frame);
                    }
                }
            }
            _ => d.panel.draw_approach(&mut tex.canvas),
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
                draw_lamp(
                    c,
                    &fonts,
                    r,
                    label.unwrap_or_else(|| d.panel.lamp_label(id)),
                    if on { style.on } else { style.off },
                    k,
                    t,
                );
                on || (backlit && style.off == GREEN.off)
            }
            Face::Button(id, label) => {
                let on = lit[id as usize];
                draw_lamp(
                    c,
                    &fonts,
                    r,
                    label.unwrap_or_else(|| d.panel.lamp_label(id)),
                    if on { BUTTON.on } else { BUTTON.off },
                    k,
                    t,
                );
                on || backlit
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
            Face::Item(Elem::GearLights) => {
                let down = lit[LampId::Gear as usize];
                gear_lights(c, r, down);
                down
            }
            Face::Item(Elem::Dial(dial)) => {
                crate::panel::draw_dial(c, r, dial, d.panel.dial_value(dial));
                backlit
            }
            // Modern avionics: blank caps, the labels are on the display.
            Face::Osb(..) if d.mfd.state.modern => {
                draw_lamp(c, &fonts, r, "", (35, 38), k, t);
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
                    draw_label(
                        c,
                        &fonts,
                        [r[0] + k, r[1] + k, r[2] - 2 * k, r[3] - 2 * k],
                        label,
                        7,
                        38,
                        t,
                    );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every layout's faces fit the instrument atlas (`pack` asserts).
    #[test]
    fn atlas_fits() {
        for style in [PanelStyle::Classic, PanelStyle::Modern, PanelStyle::F16] {
            let layout = PanelLayout::for_style(style);
            let k = atlas_scale(style);
            let slots =
                pack(&atlas_faces(&layout).iter().map(|(_, r)| texels(r, k)).collect::<Vec<_>>());
            let used = slots.iter().map(|s| s[1] + s[3]).max().unwrap_or(0);
            println!("{style:?}: {} faces, atlas rows used {used}/{ATLAS}", slots.len());
        }
    }
}
