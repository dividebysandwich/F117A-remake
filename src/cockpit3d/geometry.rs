//! Cockpit geometry in the cockpit frame: origin at the design eye point (`camera::EYE_OFFSET`),
//! -Z forward, +Y up, +X right, metres.
//!
//! The instrument panel layouts (`layout`) are in the original 320x200 screen's pixels,
//! projected onto a tilted panel plane: each panel pixel keeps the direction it had from the
//! original eye (`sx = cx + f*x/z`, `sy = cy - 0.75*f*y/z`). The cockpit models were built on
//! that plane; here it gives the faces' sizes for the instrument atlas.

use bevy::prelude::*;

use crate::cockpit::{FOCAL, VSCALE};

/// Original 3D window centre (cockpit view) in screen pixels.
pub const CX: f32 = 159.5;
pub const CY: f32 = 52.0;

/// Panel plane: through the point seen at screen row 150 at 0.72 m, tilted back by 0.3 rad.
const PANEL_DEPTH: f32 = 0.72;
const PANEL_ROW: f32 = 150.0;
const PANEL_TILT: f32 = 0.30;

/// Direction from the eye through an original screen pixel (z = -1).
pub fn pixel_dir(px: f32, py: f32) -> Vec3 {
    Vec3::new((px - CX) / FOCAL, -(py - CY) / (VSCALE * FOCAL), -1.0)
}

/// Panel normal (towards the pilot).
pub fn panel_normal() -> Vec3 {
    Vec3::new(0.0, PANEL_TILT.sin(), PANEL_TILT.cos())
}

/// Point on the panel plane (moved `offset` metres along the normal) seen at pixel (px, py).
pub fn panel_point(px: f32, py: f32, offset: f32) -> Vec3 {
    let n = panel_normal();
    let p0 = pixel_dir(CX, PANEL_ROW) * PANEL_DEPTH + n * offset;
    let d = pixel_dir(px, py);
    d * (n.dot(p0) / n.dot(d))
}

/// Centre, size (metres, along panel x/y) of a panel pixel rectangle (x, y, w, h).
pub fn panel_rect(x: f32, y: f32, w: f32, h: f32, offset: f32) -> (Vec3, Vec2) {
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    let c = panel_point((x0 + x1) / 2.0, (y0 + y1) / 2.0, offset);
    let width = panel_point(x0, (y0 + y1) / 2.0, offset).distance(panel_point(x1, (y0 + y1) / 2.0, offset));
    let height = panel_point((x0 + x1) / 2.0, y0, offset).distance(panel_point((x0 + x1) / 2.0, y1, offset));
    (c, Vec2::new(width, height))
}
