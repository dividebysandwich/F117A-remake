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
        Self { canvas, image: images.add(image), transparent, mask: None, dirty: true }
    }
}

fn upload_canvases(palette: Res<Palette>, mut images: ResMut<Assets<Image>>, mut q: Query<&mut CanvasTexture>) {
    let all = palette.is_changed();
    // Palette as 8-bit sRGB.
    let lut: Vec<[u8; 4]> = (0..=255u8)
        .map(|i| {
            let c = palette.color(i).to_srgba();
            [(c.red * 255.0) as u8, (c.green * 255.0) as u8, (c.blue * 255.0) as u8, 255]
        })
        .collect();
    for mut ct in &mut q {
        if !(ct.dirty || all) {
            continue;
        }
        ct.dirty = false;
        let ct = ct.into_inner();
        let Some(mut img) = images.get_mut(&ct.image) else { continue };
        let Some(data) = img.data.as_mut() else { continue };
        for (i, &c) in ct.canvas.px.iter().enumerate() {
            let see = ct.transparent && c == TRANSPARENT && ct.mask.as_ref().is_none_or(|m| m[i]);
            let rgba = if see { [0, 0, 0, 0] } else { lut[c as usize] };
            data[i * 4..i * 4 + 4].copy_from_slice(&rgba);
        }
    }
}
