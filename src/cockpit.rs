//! Cockpit art and display drawing shared by the 3D cockpit.
//!
//! The classic view projects with `sx = cx + f*x/z`, `sy = cy - 0.75*f*y/z`, f = 256, so each pixel
//! of the 320x200 screen is a fixed direction from the eye. The 3D cockpit keeps that mapping
//! for its panel picture and HUD (`cockpit3d::geometry`).
//!
//! The instrument panel itself is built from meshes (`cockpit3d`); HUD symbols, LED digits, TAC
//! map icons, the approach indexer and the weapon side views come from a sprite atlas,
//! `images/sprites.png`, at the positions of the 256PIT atlas (drawn by
//! `tools/gen_images.py`).

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

/// Classic focal length (pixels) and vertical squash.
pub const FOCAL: f32 = 256.0;
pub const VSCALE: f32 = 0.75;

/// The sprite atlas, the source of every sprite the displays draw.
#[derive(Resource)]
pub struct CockpitArt {
    pub sprites: Canvas,
}

/// Loads an indexed PNG as palette indices.
fn load_indexed_png(name: &str) -> Canvas {
    let path = crate::asset_path::asset(&format!("images/{name}.png"));
    let fail = |e: &dyn std::fmt::Display| -> ! {
        panic!("{}: {e} (run tools/gen_images.py)", path.display())
    };
    let file = std::fs::File::open(&path).unwrap_or_else(|e| fail(&e));
    let mut reader =
        png::Decoder::new(std::io::BufReader::new(file)).read_info().unwrap_or_else(|e| fail(&e));
    let mut px = vec![0; reader.output_buffer_size().unwrap_or_else(|| fail(&"image too large"))];
    let frame = reader.next_frame(&mut px).unwrap_or_else(|e| fail(&e));
    if (frame.color_type, frame.bit_depth) != (png::ColorType::Indexed, png::BitDepth::Eight) {
        fail(&"not an 8-bit indexed PNG");
    }
    px.truncate(frame.buffer_size());
    Canvas::from_pixels(frame.width as usize, frame.height as usize, px)
}

fn setup(mut commands: Commands) {
    commands.insert_resource(CockpitArt { sprites: load_indexed_png("sprites") });
    commands.insert_resource(Fonts::builtin());
}

/// Everything the HUD, MFDs and panel instruments draw from.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Displays<'w, 's> {
    pub hud: HudParams<'w, 's>,
    pub mfd: MfdParams<'w, 's>,
    pub panel: PanelParams<'w, 's>,
}
