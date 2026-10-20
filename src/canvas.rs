//! Palette-indexed pixel canvases (the original's 320x200 mode 13h drawing model), uploaded to
//! textures through the active palette. Cockpit panel, HUD and MFDs draw into canvases; the
//! textures are shown on world-space quads (so they also work in VR).

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::palette::Palette;

pub struct CanvasPlugin;

impl Plugin for CanvasPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, upload_canvases.run_if(resource_exists::<Palette>));
    }
}

/// Palette index treated as transparent when `CanvasTexture::transparent` is set.
pub const TRANSPARENT: u8 = 0;

#[derive(Clone)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    /// Drawing clip rectangle (x0, y0, x1, y1), inclusive, like the original's page
    /// descriptors. `set` (and everything built on it) only writes inside it.
    pub clip: [i32; 4],
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        Self::from_pixels(w, h, vec![0; w * h])
    }

    pub fn from_pixels(w: usize, h: usize, px: Vec<u8>) -> Self {
        assert_eq!(px.len(), w * h);
        Self { w, h, px, clip: [0, 0, w as i32 - 1, h as i32 - 1] }
    }

    pub fn clear(&mut self, c: u8) {
        self.px.fill(c);
    }

    /// Sets the clip rectangle (inclusive), limited to the canvas.
    pub fn set_clip(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.clip = [x0.max(0), y0.max(0), x1.min(self.w as i32 - 1), y1.min(self.h as i32 - 1)];
    }

    pub fn reset_clip(&mut self) {
        self.clip = [0, 0, self.w as i32 - 1, self.h as i32 - 1];
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: u8) {
        let [x0, y0, x1, y1] = self.clip;
        if x >= x0 && y >= y0 && x <= x1 && y <= y1 {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    /// Replaces colour `from` by `to` inside the rectangle (x0,y0)-(x1,y1) inclusive
    /// (the driver's recolour-rectangle used for panel lights).
    pub fn recolor_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, from: u8, to: u8) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if self.get(x, y) == from {
                    self.set(x, y, to);
                }
            }
        }
    }

    /// Copies a rectangle opaquely (the driver's copyRect).
    #[allow(clippy::too_many_arguments)]
    pub fn copy_rect(&mut self, src: &Canvas, sx: i32, sy: i32, w: i32, h: i32, dx: i32, dy: i32) {
        self.blit(src, sx, sy, w, h, dx, dy, None, None);
    }

    pub fn get(&self, x: i32, y: i32) -> u8 {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize]
        } else {
            0
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u8) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u8) {
        self.hline(x, x + w - 1, y, c);
        self.hline(x, x + w - 1, y + h - 1, c);
        for yy in y..y + h {
            self.set(x, yy, c);
            self.set(x + w - 1, yy, c);
        }
    }

    pub fn hline(&mut self, x0: i32, x1: i32, y: i32, c: u8) {
        for x in x0.min(x1)..=x0.max(x1) {
            self.set(x, y, c);
        }
    }

    /// Bresenham line, as the original draws HUD lines.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u8) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.set(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Copies a rectangle from another canvas (e.g. the 256PIT sprite atlas). Pixels equal to
    /// `key` are skipped; `recolor` replaces every drawn pixel's colour if set.
    #[allow(clippy::too_many_arguments)]
    pub fn blit(&mut self, src: &Canvas, sx: i32, sy: i32, w: i32, h: i32, dx: i32, dy: i32, key: Option<u8>, recolor: Option<u8>) {
        for y in 0..h {
            for x in 0..w {
                let c = src.get(sx + x, sy + y);
                if Some(c) == key {
                    continue;
                }
                self.set(dx + x, dy + y, recolor.unwrap_or(c));
            }
        }
    }
}

impl Canvas {
    /// `blit` with every source pixel drawn as a `scale` x `scale` block.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_scaled(&mut self, src: &Canvas, sx: i32, sy: i32, w: i32, h: i32, dx: i32, dy: i32, scale: i32, key: Option<u8>, recolor: Option<u8>) {
        for y in 0..h {
            for x in 0..w {
                let c = src.get(sx + x, sy + y);
                if Some(c) == key {
                    continue;
                }
                self.fill_rect(dx + x * scale, dy + y * scale, scale, scale, recolor.unwrap_or(c));
            }
        }
    }

    /// Filled triangle: every pixel whose centre is inside (or on an edge).
    pub fn triangle(&mut self, p: [Vec2; 3], c: u8) {
        let edge = |a: Vec2, b: Vec2, q: Vec2| (b - a).perp_dot(q - a);
        let area = edge(p[0], p[1], p[2]);
        if area == 0.0 {
            return;
        }
        let lo = p[0].min(p[1]).min(p[2]).floor();
        let hi = p[0].max(p[1]).max(p[2]).ceil();
        for y in lo.y as i32..hi.y as i32 {
            for x in lo.x as i32..hi.x as i32 {
                let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w = [edge(p[1], p[2], q), edge(p[2], p[0], q), edge(p[0], p[1], q)];
                if w.iter().all(|&w| w * area >= 0.0) {
                    self.set(x, y, c);
                }
            }
        }
    }

    /// Circle outline (polyline).
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: u8) {
        let steps = ((r * 1.5) as i32).clamp(12, 256);
        let mut prev = (cx + r, cy);
        for i in 1..=steps {
            let t = i as f32 / steps as f32 * std::f32::consts::TAU;
            let p = (cx + r * t.cos(), cy + r * t.sin());
            self.line(prev.0 as i32, prev.1 as i32, p.0 as i32, p.1 as i32, c);
            prev = p;
        }
    }
}

/// Thick strokes for the high-resolution displays (sub-pixel endpoints, round pens).
impl Canvas {
    /// Round pen of width `w` stamped at (x, y).
    fn dot(&mut self, x: f32, y: f32, w: f32, c: u8) {
        let r = w * 0.5;
        let (x0, x1) = ((x - r).round() as i32, (x + r).round() as i32);
        let (y0, y1) = ((y - r).round() as i32, (y + r).round() as i32);
        let rr = r * r + 0.25;
        for yy in y0..=y1 {
            for xx in x0..=x1 {
                let (dx, dy) = (xx as f32 + 0.5 - x, yy as f32 + 0.5 - y);
                if dx * dx + dy * dy <= rr {
                    self.set(xx, yy, c);
                }
            }
        }
    }

    /// Line of width `w` from a to b.
    pub fn stroke(&mut self, a: Vec2, b: Vec2, w: f32, c: u8) {
        let [x0, y0, x1, y1] = self.clip;
        let (lo, hi) = (a.min(b) - w, a.max(b) + w);
        if hi.x < x0 as f32 || hi.y < y0 as f32 || lo.x > x1 as f32 + 1.0 || lo.y > y1 as f32 + 1.0 {
            return;
        }
        let len = a.distance(b);
        // Far off-canvas endpoints (projected cues): clip the segment to a padded canvas first.
        let (a, b) = if len > 4.0 * (self.w + self.h) as f32 {
            match clip_segment(a, b, Vec2::new(x0 as f32 - w, y0 as f32 - w), Vec2::new(x1 as f32 + w, y1 as f32 + w)) {
                Some(s) => s,
                None => return,
            }
        } else {
            (a, b)
        };
        let len = a.distance(b);
        let steps = (len / (w * 0.35).max(0.5)).ceil().max(1.0) as i32;
        for i in 0..=steps {
            let p = a.lerp(b, i as f32 / steps as f32);
            self.dot(p.x, p.y, w, c);
        }
    }

    /// Polyline of width `w`.
    pub fn polyline(&mut self, pts: &[Vec2], w: f32, c: u8) {
        for p in pts.windows(2) {
            self.stroke(p[0], p[1], w, c);
        }
    }

    /// Dashed line: `dash` pixels drawn, `gap` pixels skipped.
    pub fn dashed(&mut self, a: Vec2, b: Vec2, dash: f32, gap: f32, w: f32, c: u8) {
        let len = a.distance(b);
        if len <= 0.0 {
            return;
        }
        let d = (b - a) / len;
        let mut t = 0.0;
        while t < len {
            let e = (t + dash).min(len);
            self.stroke(a + d * t, a + d * e, w, c);
            t = e + gap;
        }
    }

    /// Circle outline of radius r and width w.
    pub fn ring(&mut self, centre: Vec2, r: f32, w: f32, c: u8) {
        self.arc(centre, r, 0.0, std::f32::consts::TAU, w, c);
    }

    /// Arc from angle a0 to a1 (radians, 0 = right, positive = clockwise on screen).
    pub fn arc(&mut self, centre: Vec2, r: f32, a0: f32, a1: f32, w: f32, c: u8) {
        let steps = ((r * (a1 - a0).abs() / 3.0) as i32).clamp(8, 256);
        let mut prev = centre + Vec2::new(a0.cos(), a0.sin()) * r;
        for i in 1..=steps {
            let t = a0 + (a1 - a0) * i as f32 / steps as f32;
            let p = centre + Vec2::new(t.cos(), t.sin()) * r;
            self.stroke(prev, p, w, c);
            prev = p;
        }
    }

    /// Rectangle outline (corners a, b) of width w.
    pub fn frame(&mut self, a: Vec2, b: Vec2, w: f32, c: u8) {
        let (p, q) = (a.min(b), a.max(b));
        self.polyline(&[p, Vec2::new(q.x, p.y), q, Vec2::new(p.x, q.y), p], w, c);
    }
}

/// Liang-Barsky clip of segment a-b to the box lo..hi.
fn clip_segment(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> Option<(Vec2, Vec2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x - lo.x), (d.x, hi.x - a.x), (-d.y, a.y - lo.y), (d.y, hi.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then(|| (a + d * t0, a + d * t1))
}

/// A canvas shown through a texture. Set `dirty` after drawing.
#[derive(Component)]
pub struct CanvasTexture {
    pub canvas: Canvas,
    pub image: Handle<Image>,
    /// Index 0 becomes fully transparent (overlays such as the HUD or the panel's window area).
    pub transparent: bool,
    /// Optional per-pixel mask: index 0 is only transparent where the mask is true.
    pub mask: Option<Vec<bool>>,
    pub dirty: bool,
    /// The pixels and transparency last uploaded: a redraw that changed nothing (most frames
    /// for the instrument atlas) skips the conversion and the texture upload.
    shown: Option<(Vec<u8>, bool)>,
}

impl CanvasTexture {
    pub fn new(canvas: Canvas, images: &mut Assets<Image>, transparent: bool) -> Self {
        let mut image = Image::new_fill(
            Extent3d { width: canvas.w as u32, height: canvas.h as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.sampler = ImageSampler::nearest();
        Self { canvas, image: images.add(image), transparent, mask: None, dirty: true, shown: None }
    }
}

fn upload_canvases(palette: Res<Palette>, mut images: ResMut<Assets<Image>>, mut q: Query<&mut CanvasTexture>) {
    let all = palette.is_changed();
    // Palette as 8-bit sRGB RGBA (in memory order), and with index 0 see-through.
    let mut lut = [0u32; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        let c = palette.color(i as u8).to_srgba();
        *v = u32::from_le_bytes([(c.red * 255.0) as u8, (c.green * 255.0) as u8, (c.blue * 255.0) as u8, 255]);
    }
    let mut see_lut = lut;
    see_lut[TRANSPARENT as usize] = 0;
    for mut ct in &mut q {
        if !(ct.dirty || all) {
            continue;
        }
        let ct = ct.bypass_change_detection();
        ct.dirty = false;
        let unchanged = ct.shown.as_ref().is_some_and(|(px, t)| *t == ct.transparent && *px == ct.canvas.px);
        if !all && ct.mask.is_none() && unchanged {
            continue;
        }
        let Some(mut img) = images.get_mut(&ct.image) else { continue };
        let Some(data) = img.data.as_mut() else { continue };
        let out = data.as_chunks_mut::<4>().0.iter_mut().zip(&ct.canvas.px);
        match (&ct.mask, ct.transparent) {
            (Some(mask), true) => {
                for (i, (o, &c)) in out.enumerate() {
                    let l = if mask[i] { &see_lut } else { &lut };
                    *o = l[c as usize].to_le_bytes();
                }
            }
            (_, t) => {
                let l = if t { &see_lut } else { &lut };
                for (o, &c) in out {
                    *o = l[c as usize].to_le_bytes();
                }
            }
        }
        match &mut ct.shown {
            Some((px, t)) => {
                px.clone_from(&ct.canvas.px);
                *t = ct.transparent;
            }
            None => ct.shown = Some((ct.canvas.px.clone(), ct.transparent)),
        }
    }
}
