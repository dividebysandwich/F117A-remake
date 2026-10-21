//! There is no height field: the ground is a flat plane plus models placed per tile. Each LOD
//! level L (1..=4) tiles the 2^20-unit world with tiles of `TILE_WORLD[L]` world units; an
//! object in a level-L tile is positioned and modelled in level-L units (4^(L-1) world units).

use serde::{Deserialize, Serialize};

/// World units per level unit for LOD 0..=4.
pub const LEVEL_UNIT: [f64; 5] = [0.5, 1.0, 4.0, 16.0, 64.0];
/// Tiles per side for LOD 0..=4 (LOD4 includes a 2-cell border on each side).
pub const GRID_DIM: [i32; 5] = [0x400, 0x100, 0x40, 0x10, 8];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grid {
    /// 8x8 LOD4 grid (row-major), indexed with (col+2, row+2).
    pub lod4: Vec<u8>,
    /// 16x16 LOD3 grid.
    pub lod3: Vec<u8>,
    /// 32 blocks of 4x4, indexed by the parent LOD3 tile type.
    pub lod2: Vec<u8>,
    /// 32 blocks of 4x4, indexed by the parent LOD2 tile type.
    pub lod1: Vec<u8>,
}

impl Grid {
    /// Tile type at (col, row) on a LOD level ; 0 outside the grid.
    pub fn tile(&self, lod: usize, col: i32, row: i32) -> u8 {
        let (mut col, mut row) = (col, row);
        if lod == 4 {
            col += 2;
            row += 2;
        }
        if col < 0 || row < 0 || col >= GRID_DIM[lod] || row >= GRID_DIM[lod] {
            return 0;
        }
        let block = |tab: &Vec<u8>, parent: u8| {
            tab[(col & 3) as usize + ((row & 3) as usize) * 4 + parent as usize * 16]
        };
        match lod {
            4 => self.lod4[(col + row * 8) as usize],
            3 => self.lod3[(col + row * 16) as usize],
            2 => block(&self.lod2, self.tile(3, col >> 2, row >> 2)),
            1 => block(&self.lod1, self.tile(2, col >> 2, row >> 2)),
            _ => 0,
        }
    }
}

/// An object placed relative to a tile centre, in level units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileObject {
    pub x: i16,
    /// Horizontal, the second map axis (map y grows southward).
    pub y: i16,
    /// Altitude above ground.
    pub z: i16,
    /// Index into the shape -> model handle table (low byte only).
    pub shape: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileSets {
    /// `tiles[lod][tile_type]` = objects of that tile type.
    pub tiles: Vec<Vec<Vec<TileObject>>>,
}
