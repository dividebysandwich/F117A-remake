//! Cockpit art and display drawing shared by the 3D cockpit (docs/re/visuals.md 4.1/4.2).
//!
//! The original projects with `sx = cx + f*x/z`, `sy = cy - 0.75*f*y/z`, f = 256, so each pixel
//! of the 320x200 screen is a fixed direction from the eye. The 3D cockpit keeps that mapping
//! for its panel picture and HUD (`cockpit3d::geometry`).
//!
//! Palette index 0 in 256PIT is see-through where it is connected to the window; enclosed
//! index-0 holes (MFD screens, instruments) stay opaque and the displays are drawn into them.

use bevy::prelude::*;

use crate::canvas::Canvas;
use crate::font::Fonts;
use crate::hud::HudParams;
use crate::mfd::MfdParams;
use crate::panel::PanelParams;

pub struct CockpitPlugin;

impl Plugin for CockpitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup);
    }
}

/// Original focal length (pixels) and vertical squash.
pub const FOCAL: f32 = 256.0;
pub const VSCALE: f32 = 0.75;

/// Original pictures used as sprite sources: 256PIT as loaded (the driver's page 2, with the
/// sprite atlas in rows 0..90), its night-HUD variant and ORDNANCE.PIC (weapon side views).
#[derive(Resource)]
pub struct CockpitArt {
    pub pit: Canvas,
    /// 256PIT with the HUD sprites recoloured 15 -> 10 (night HUD, VGAME `1A18:01BF`).
    pub atlas_night: Canvas,
    pub ordnance: Canvas,
}

pub(crate) fn load_idx(name: &str) -> Canvas {
    let path = crate::asset_path::asset(&format!("images/{name}.idx"));
    let px = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e} (run tools/build-assets.sh)", path.display()));
    Canvas::from_pixels(320, 200, px)
}

/// Marks index-0 pixels reachable from the picture's top edge (the window) as see-through;
/// enclosed holes stay opaque.
pub(crate) fn window_mask(c: &Canvas, first_row: usize) -> Vec<bool> {
    let mut see = vec![false; c.w * c.h];
    let mut stack: Vec<(usize, usize)> = (0..c.w).map(|x| (x, first_row)).collect();
    while let Some((x, y)) = stack.pop() {
        let i = y * c.w + x;
        if see[i] || c.px[i] != 0 {
            continue;
        }
        see[i] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < c.w {
            stack.push((x + 1, y));
        }
        if y + 1 < c.h {
            stack.push((x, y + 1));
        }
        if y > first_row {
            stack.push((x, y - 1));
        }
    }
    // Everything above the first row is window too.
    see[..first_row * c.w].fill(true);
    see
}

/// The night-HUD atlas: 15 -> 10 in the HUD sprite rectangles.
pub fn night_atlas(pit: &Canvas) -> Canvas {
    let mut c = pit.clone();
    for (x0, y0, x1, y1) in [(0, 0, 140, 29), (0, 30, 64, 37), (38, 75, 91, 87)] {
        c.recolor_rect(x0, y0, x1, y1, 15, 10);
    }
    c
}

fn setup(mut commands: Commands) {
    let pit = load_idx("256pit");
    commands.insert_resource(CockpitArt { atlas_night: night_atlas(&pit), pit, ordnance: load_idx("ordnance") });
    commands.insert_resource(Fonts::load());
}

/// Everything the HUD, MFDs and panel instruments draw from.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Displays<'w, 's> {
    pub hud: HudParams<'w, 's>,
    pub mfd: MfdParams<'w, 's>,
    pub panel: PanelParams<'w, 's>,
}
