//! LZEXE 0.91 unpacker. All main F-117A executables (START, VGAME, END) are packed with it.
//!
//! Produces the original load module (image as it is before DOS relocation, i.e. at base
//! segment 0) plus the relocation list and register entry values, so data at known
//! DGROUP offsets can be read straight from the original EXE without running it.

use anyhow::{Context, Result, bail, ensure};

#[derive(Debug, Clone)]
pub struct Unpacked {
    /// Load module (relative segments, not relocated).
    pub image: Vec<u8>,
    /// Relocation entries as (segment, offset) relative to the image start.
    pub relocs: Vec<(u16, u16)>,
    pub entry_cs: u16,
    pub entry_ip: u16,
    pub ss: u16,
    pub sp: u16,
}

impl Unpacked {
    /// Byte offset in `image` for a segment relative to the load module.
    pub fn linear(&self, seg: u16, off: u16) -> usize {
        seg as usize * 16 + off as usize
    }

    /// Apply relocations for a given load segment (to compare against emulator memory dumps).
    pub fn relocated(&self, load_seg: u16) -> Vec<u8> {
        let mut img = self.image.clone();
        for &(s, o) in &self.relocs {
            let p = self.linear(s, o);
            if p + 1 < img.len() {
                let v = u16::from_le_bytes([img[p], img[p + 1]]).wrapping_add(load_seg);
                img[p..p + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        img
    }
}

fn rd16(d: &[u8], p: usize) -> Result<u16> {
    d.get(p..p + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .context("lzexe: unexpected end of file")
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u16,
    count: u32,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8], pos: usize) -> Result<Self> {
        Ok(Self { d, pos: pos + 2, buf: rd16(d, pos)?, count: 16 })
    }
    fn bit(&mut self) -> Result<u16> {
        let b = self.buf & 1;
        self.count -= 1;
        if self.count == 0 {
            self.buf = rd16(self.d, self.pos)?;
            self.pos += 2;
            self.count = 16;
        } else {
            self.buf >>= 1;
        }
        Ok(b)
    }
    fn byte(&mut self) -> Result<u8> {
        let b = *self.d.get(self.pos).context("lzexe: unexpected end of data")?;
        self.pos += 1;
        Ok(b)
    }
}

pub fn is_lzexe(exe: &[u8]) -> bool {
    exe.len() > 0x20 && &exe[0x1c..0x20] == b"LZ91"
}

pub fn unpack(exe: &[u8]) -> Result<Unpacked> {
    ensure!(exe.len() > 0x20 && &exe[0..2] == b"MZ", "not an MZ executable");
    ensure!(is_lzexe(exe), "not LZEXE 0.91 packed");
    let header_paras = rd16(exe, 0x08)? as usize;
    let init_cs = rd16(exe, 0x16)? as usize;
    let stub = (header_paras + init_cs) * 16;
    let info: Vec<u16> = (0..8).map(|i| rd16(exe, stub + i * 2)).collect::<Result<_>>()?;
    let (ip, cs, sp, ss, cmp_paras) = (info[0], info[1], info[2], info[3], info[4] as usize);

    // Decompress.
    let data_start = (header_paras + init_cs - cmp_paras) * 16;
    let mut bits = Bits::new(exe, data_start)?;
    let mut out: Vec<u8> = Vec::with_capacity(256 * 1024);
    loop {
        if bits.bit()? == 1 {
            out.push(bits.byte()?);
            continue;
        }
        let (span, mut len): (i32, usize);
        if bits.bit()? == 0 {
            let l = (bits.bit()? << 1) | bits.bit()?;
            len = l as usize + 2;
            span = bits.byte()? as i32 - 256;
        } else {
            let lo = bits.byte()? as i32;
            let hi = bits.byte()? as i32;
            span = (lo | ((hi & !7) << 5) | 0xE000) - 0x10000;
            len = (hi & 7) as usize + 2;
            if len == 2 {
                len = bits.byte()? as usize;
                match len {
                    0 => break,
                    1 => continue,
                    _ => len += 1,
                }
            }
        }
        let start = out.len() as i64 + span as i64;
        if start < 0 {
            bail!("lzexe: back reference before start of output");
        }
        for i in 0..len {
            out.push(out[start as usize + i]);
        }
    }

    // Relocation table (v0.91: at stub + 0x158).
    let mut relocs = Vec::new();
    let mut p = stub + 0x158;
    let (mut rel_seg, mut rel_off) = (0u32, 0u32);
    loop {
        let mut span = *exe.get(p).context("lzexe: reloc table truncated")? as u32;
        p += 1;
        if span == 0 {
            span = rd16(exe, p)? as u32;
            p += 2;
            if span == 0 {
                rel_seg += 0x0FFF;
                continue;
            } else if span == 1 {
                break;
            }
        }
        rel_off += span;
        rel_seg += (rel_off & !0x0F) >> 4;
        rel_off &= 0x0F;
        relocs.push((rel_seg as u16, rel_off as u16));
    }

    Ok(Unpacked { image: out, relocs, entry_cs: cs, entry_ip: ip, ss, sp })
}
