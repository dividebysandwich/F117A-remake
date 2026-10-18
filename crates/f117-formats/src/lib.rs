pub mod font;
pub mod lzexe;
pub mod pal;
pub mod pic;
pub mod terrain;
pub mod wld;
pub mod x3d;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Theater codes in the order the game uses for file names.
pub const THEATERS: [&str; 9] = ["LB", "PG", "NC", "CE", "VN", "ME", "KU", "CU", "KO"];

/// Access to an original game installation (file names are matched case-insensitively).
#[derive(Debug, Clone)]
pub struct GameDir {
    root: PathBuf,
}

impl GameDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `$F117_GAME_DIR` or `~/Games/DOS/F117`, if it exists.
    pub fn default_location() -> Option<Self> {
        let p = std::env::var_os("F117_GAME_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join("Games/DOS/F117")))?;
        p.join("VGAME.EXE").exists().then(|| Self::new(p))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, name: &str) -> Result<PathBuf> {
        let direct = self.root.join(name);
        if direct.exists() {
            return Ok(direct);
        }
        for e in std::fs::read_dir(&self.root)? {
            let e = e?;
            if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
                return Ok(e.path());
            }
        }
        anyhow::bail!("{name} not found in {}", self.root.display())
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let p = self.path(name)?;
        std::fs::read(&p).with_context(|| format!("reading {}", p.display()))
    }

    /// All files with the given extension (upper-case names as stored by DOS).
    pub fn list(&self, ext: &str) -> Result<Vec<String>> {
        let mut v: Vec<String> = std::fs::read_dir(&self.root)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.rsplit('.').next().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
            .collect();
        v.sort();
        Ok(v)
    }

    /// Unpacked VGAME.EXE load module.
    pub fn vgame(&self) -> Result<lzexe::Unpacked> {
        lzexe::unpack(&self.read("VGAME.EXE")?)
    }

    /// Terrain grid of a theater (needs the LOD4 grid table from VGAME.EXE).
    pub fn grid(&self, theater: &str) -> Result<terrain::Grid> {
        let idx = THEATERS.iter().position(|t| *t == theater).context("unknown theater")?;
        let v = self.vgame()?;
        let lod4 = &v.image[terrain::LOD4_GRIDS_OFFSET + idx * 64..][..64];
        terrain::Grid::parse(&self.read(&format!("{theater}.3DG"))?, lod4)
    }

    /// The base flight palette from VGAME.EXE.
    pub fn flight_palette(&self) -> Result<pal::Palette> {
        let v = self.vgame()?;
        pal::Palette::from_vga(&v.image[pal::FLIGHT_PALETTE_OFFSET..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> Option<GameDir> {
        let g = GameDir::default_location();
        if g.is_none() {
            eprintln!("original game not found, skipping");
        }
        g
    }

    /// Optional emulator dumps (tools/re), relative to the workspace root.
    fn dump(name: &str) -> Option<Vec<u8>> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re-work/dumps").join(name);
        std::fs::read(p).ok()
    }

    #[test]
    fn lzexe_matches_emulator_dump() {
        let Some(g) = game() else { return };
        for (exe, dumpname, entry_ip) in
            [("VGAME.EXE", "vgame_1A18.bin", 0xEBB4), ("START.EXE", "start_1A18.bin", 0x8EDC)]
        {
            let u = lzexe::unpack(&g.read(exe).unwrap()).unwrap();
            assert_eq!(u.entry_ip, entry_ip);
            let Some(d) = dump(dumpname) else { continue };
            let img = u.relocated(0x1A18);
            // The C runtime has not run yet at the dump point, so code + initialised data match.
            let n = img.len().min(d.len());
            let diff = (0..n).filter(|&i| img[i] != d[i]).count();
            assert_eq!(diff, 0, "{exe}: {diff} bytes differ from emulator dump");
        }
    }

    #[test]
    fn all_pics_decode() {
        let Some(g) = game() else { return };
        let pics = g.list("PIC").unwrap();
        assert!(pics.len() > 40);
        for name in pics {
            let p = pic::decode(&g.read(&name).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(p.pixels.len(), 64_000);
        }
    }

    #[test]
    fn flight_palette_matches_emulator() {
        let Some(g) = game() else { return };
        let pal = g.flight_palette().unwrap();
        // Index 0 black, 15 white (EGA part of the palette).
        assert_eq!(pal.colors[0], [0, 0, 0]);
        assert_eq!(pal.colors[15], [63, 63, 63]);
        if let Some(m) = dump("runway_1mb.bin") {
            let base = 0x39FFA;
            assert_eq!(Palette::from_vga(&m[base..base + 768]).unwrap(), pal);
        }
    }

    #[test]
    fn all_3d_files_parse() {
        let Some(g) = game() else { return };
        for f in ["STFLT.3DX", "PHOTO.3DX"] {
            let x = x3d::parse(&g.read(f).unwrap()).unwrap_or_else(|e| panic!("{f}: {e:#}"));
            assert!(!x.models.is_empty());
        }
        let mut dots = 0;
        for t in THEATERS {
            let x = x3d::parse(&g.read(&format!("{t}.3DX")).unwrap()).unwrap_or_else(|e| panic!("{t}.3DX: {e:#}"));
            dots += x.bodies.iter().flat_map(|b| b.lists.iter().flatten())
                .filter_map(|p| match &p.kind { x3d::PrimKind::Dots(d) => Some(d.len()), _ => None }).sum::<usize>();
            let tiles = terrain::TileSets::parse(&g.read(&format!("{t}.3DT")).unwrap()).unwrap();
            for o in tiles.tiles.iter().flatten().flatten() {
                assert!((o.shape as usize) < x.models.len(), "{t}: shape {} out of range", o.shape);
            }
            let grid = g.grid(t).unwrap();
            for lod in 1..=4 {
                let n = terrain::GRID_DIM[lod];
                for r in 0..n { for c in 0..n {
                    assert!((grid.tile(lod, c, r) as usize) < tiles.tiles[lod].len());
                }}
            }
            let w = wld::World::parse(&g.read(&format!("{t}.WLD")).unwrap()).unwrap();
            assert_eq!(w.names.len(), wld::TABLE, "{t}: name count");
            assert!(w.objects.iter().all(|o| (o.name as usize) < wld::TABLE));
        }
        assert!(dots > 10_000, "expected the night-light dots, got {dots}");
    }

    use pal::Palette;
}

