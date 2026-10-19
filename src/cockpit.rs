//! Cockpit art and display drawing shared by the 3D cockpit.
//!
//! The original projects with `sx = cx + f*x/z`, `sy = cy - 0.75*f*y/z`, f = 256, so each pixel
//! of the 320x200 screen is a fixed direction from the eye. The 3D cockpit keeps that mapping
//! for its panel picture and HUD (`cockpit3d::geometry`).
//!
//! The instrument panel itself is built from meshes (`cockpit3d`); HUD symbols, LED digits, TAC
//! map icons and the approach indexer come from a sprite atlas, `images/sprites.idx`, at the
//! positions of the original 256PIT atlas (drawn by `tools/gen_images.py`).

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

/// Pictures used as sprite sources: the sprite atlas, its night-HUD variant and the weapon side
/// views.
#[derive(Resource)]
pub struct CockpitArt {
    pub sprites: Canvas,
    /// The atlas with the HUD sprites recoloured 15 -> 10 (night HUD).
    pub atlas_night: Canvas,
    pub ordnance: Canvas,
}

pub(crate) fn load_idx(name: &str) -> Canvas {
    let path = crate::asset_path::asset(&format!("images/{name}.idx"));
    let px = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e} (run tools/gen_images.py)", path.display()));
    assert_eq!(px.len() % 320, 0, "{}: not 320 pixels wide", path.display());
    Canvas::from_pixels(320, px.len() / 320, px)
}

/// The night-HUD atlas: 15 -> 10 in the HUD sprite rectangles.
pub fn night_atlas(sprites: &Canvas) -> Canvas {
    let mut c = sprites.clone();
    for (x0, y0, x1, y1) in [(0, 0, 140, 29), (38, 75, 93, 87)] {
        c.recolor_rect(x0, y0, x1, y1, 15, 10);
    }
    c
}

fn setup(mut commands: Commands) {
    let sprites = load_idx("sprites");
    commands.insert_resource(CockpitArt { atlas_night: night_atlas(&sprites), sprites, ordnance: load_idx("ordnance") });
    commands.insert_resource(Fonts::builtin());
}

/// Everything the HUD, MFDs and panel instruments draw from.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Displays<'w, 's> {
    pub hud: HudParams<'w, 's>,
    pub mfd: MfdParams<'w, 's>,
    pub panel: PanelParams<'w, 's>,
}
