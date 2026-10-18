use anyhow::{Result, bail, ensure};

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 200;
const MAX_BITS: u32 = 11;
const FIRST_FREE: usize = 257;

/// A decoded 320x200 palette-indexed picture.
#[derive(Debug, Clone)]
pub struct Pic {
    pub pixels: Vec<u8>,
}

impl Pic {
    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.pixels[y * WIDTH + x]
    }
}

pub fn decode(data: &[u8]) -> Result<Pic> {
    ensure!(data.first() == Some(&0xF5), "not a PIC file (bad header byte)");
    let lz = lzw(&data[1..])?;
    let mut pixels = unrle(&lz);
    ensure!(pixels.len() >= WIDTH * HEIGHT, "PIC decoded to only {} bytes", pixels.len());
    pixels.truncate(WIDTH * HEIGHT);
    Ok(Pic { pixels })
}

fn lzw(data: &[u8]) -> Result<Vec<u8>> {
    // Dictionary as (prefix code, last byte); strings rebuilt by walking prefixes.
    let mut prefix = vec![0u16; 1 << MAX_BITS];
    let mut suffix = vec![0u8; 1 << MAX_BITS];
    let mut first = vec![0u8; 1 << MAX_BITS];
    for i in 0..256 {
        suffix[i] = i as u8;
        first[i] = i as u8;
    }
    let mut out = Vec::with_capacity(64_000);
    let mut next = FIRST_FREE;
    let mut width = 9;
    let mut prev: Option<u16> = None;
    let (mut pos, mut buf, mut nbits) = (0usize, 0u32, 0u32);
    let mut stack = Vec::with_capacity(1 << MAX_BITS);

    loop {
        while nbits < width {
            let Some(&b) = data.get(pos) else { return Ok(out) };
            buf |= (b as u32) << nbits;
            pos += 1;
            nbits += 8;
        }
        let code = (buf & ((1 << width) - 1)) as usize;
        buf >>= width;
        nbits -= width;

        let emit_from = match prev {
            None => {
                if code >= 256 {
                    bail!("PIC: first code after reset is not a literal ({code})");
                }
                code
            }
            Some(p) => {
                let p = p as usize;
                if code < next && (code < 256 || code >= FIRST_FREE) {
                    prefix[next] = p as u16;
                    suffix[next] = first[code];
                    first[next] = first[p];
                    next += 1;
                    code
                } else if code == next {
                    prefix[next] = p as u16;
                    suffix[next] = first[p];
                    first[next] = first[p];
                    next += 1;
                    code
                } else {
                    bail!("PIC: invalid LZW code {code} (next {next}) at output {}", out.len());
                }
            }
        };
        // Emit string for emit_from.
        let mut c = emit_from;
        stack.clear();
        while c >= 256 {
            stack.push(suffix[c]);
            c = prefix[c] as usize;
        }
        stack.push(c as u8);
        out.extend(stack.iter().rev());
        prev = Some(emit_from as u16);

        if next >= (1 << width) {
            if width < MAX_BITS {
                width += 1;
            } else {
                next = FIRST_FREE;
                width = 9;
                prev = None;
            }
        }
    }
}

fn unrle(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64_000);
    let mut i = 0;
    while i < d.len() {
        let b = d[i];
        if b == 0x90 && i + 1 < d.len() {
            let n = d[i + 1] as usize;
            i += 2;
            if n == 0 {
                out.push(0x90);
            } else if let Some(&last) = out.last() {
                out.extend(std::iter::repeat_n(last, n - 1));
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}
