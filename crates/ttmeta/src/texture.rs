//! T3Texture (`.d3dtx`, and the pages embedded in `.font`), version 5 / PC as used by this game.
//!
//! Field order follows TTG Tools' `GetNewTextures` (GPL-3.0, see CREDITS.md), narrowed to the
//! single version this game ships: u32 version, block flags, block platform, block-string name,
//! block-string import name, f32 importance, [u32 when version > 4 and next byte is not a bool],
//! bool has-extra, u32 mip count, u32 width, u32 height, u32 format, u32 unknown, 0x34 bytes,
//! sized sub-block, u32 stored mip count, u32 region count, u32 total size, then per stored mip
//! { u32 sub, u32 mip index, [u32 1], u32 size, u32 pitch }, then region sub-blocks.
//! Pixel data lives in the async section, smallest mip first.

use crate::stream::{MetaStream, Reader};
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// 32-bit, bytes B, G, R, A.
    Bgra8,
    /// 16-bit A4 R4 G4 B4 (little-endian word).
    Argb4,
    /// 8-bit alpha only.
    A8,
    /// 8-bit intensity (luminance).
    L8,
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
}

impl Format {
    fn from_code(code: u32) -> Option<Format> {
        Some(match code {
            0x00 => Format::Bgra8,
            0x04 => Format::Argb4,
            0x10 => Format::A8,
            0x11 => Format::L8,
            0x40 => Format::Bc1,
            0x41 => Format::Bc2,
            0x42 => Format::Bc3,
            0x43 => Format::Bc4,
            0x44 => Format::Bc5,
            _ => return None,
        })
    }

    pub fn is_block(self) -> bool {
        matches!(self, Format::Bc1 | Format::Bc2 | Format::Bc3 | Format::Bc4 | Format::Bc5)
    }

    /// Bytes for one mip level of `w` x `h` pixels.
    pub fn level_size(self, w: u32, h: u32) -> usize {
        let blocks = |b: usize| ((w as usize).div_ceil(4)).max(1) * ((h as usize).div_ceil(4)).max(1) * b;
        match self {
            Format::Bgra8 => (w * h * 4) as usize,
            Format::Argb4 => (w * h * 2) as usize,
            Format::A8 | Format::L8 => (w * h) as usize,
            Format::Bc1 | Format::Bc4 => blocks(8),
            Format::Bc2 | Format::Bc3 | Format::Bc5 => blocks(16),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    /// Mip levels, largest (level 0) first.
    pub mips: Vec<Vec<u8>>,
}

struct Header {
    name: String,
    width: u32,
    height: u32,
    format: Format,
    /// (mip index, size, row pitch) in storage order.
    stored: Vec<(u32, usize, usize)>,
}

fn read_header(r: &mut Reader) -> Result<Header, Error> {
    let version = r.u32()?;
    if version != 5 {
        return Err(Error::Format(format!("T3Texture version {version} (only 5 is supported)")));
    }
    r.skip_block()?; // flags
    r.skip_block()?; // platform
    let name = r.block_string()?;
    let _import_name = r.block_string()?;
    let _importance = r.f32()?;
    let peek = *r.buf.get(r.pos).ok_or_else(|| Error::Format("texture header truncated".into()))?;
    if peek != b'0' && peek != b'1' {
        r.skip(4)?;
    }
    if r.bool()? {
        // has-extra-data: { u32 count, u32 ?, sized block } per TTG Tools; not seen in this game.
        return Err(Error::Format(format!("{name}: texture extra-data block is not supported")));
    }
    let _mips = r.u32()?;
    let width = r.u32()?;
    let height = r.u32()?;
    let code = r.u32()?;
    let format = Format::from_code(code).ok_or_else(|| Error::Format(format!("{name}: texture format {code:#x}")))?;
    let _unknown = r.u32()?;
    r.skip(0x34)?;
    r.skip_block()?;
    let stored_count = r.u32()?;
    let regions = r.u32()?;
    let _total = r.u32()?;
    let mut stored = Vec::with_capacity(stored_count as usize);
    for _ in 0..stored_count {
        let _sub = r.u32()?;
        let mip = r.u32()?;
        let mut size = r.u32()?;
        if size == 1 {
            size = r.u32()?;
        }
        let pitch = r.u32()?;
        stored.push((mip, size as usize, pitch as usize));
    }
    for _ in 0..regions {
        r.skip_block()?;
    }
    Ok(Header { name, width, height, format, stored })
}

/// Assemble the mip chain from `pixels` (the texture's slice of an async section).
fn assemble(h: Header, pixels: &[u8]) -> Result<Texture, Error> {
    let total: usize = h.stored.iter().map(|(_, s, _)| s).sum();
    if pixels.len() < total {
        return Err(Error::Format(format!("{}: pixel data {} < declared {total}", h.name, pixels.len())));
    }
    let mut levels: Vec<(u32, Vec<u8>)> = Vec::with_capacity(h.stored.len());
    let mut at = 0;
    for &(mip, size, pitch) in &h.stored {
        let data = &pixels[at..at + size];
        at += size;
        // Uncompressed rows may be padded to a 4-byte pitch; drop the padding.
        let w = (h.width >> mip).max(1);
        let row = h.format.level_size(w, 1);
        let rows = (h.height >> mip).max(1) as usize;
        let data = if !h.format.is_block() && pitch > row && size == pitch * rows {
            data.chunks_exact(pitch).flat_map(|r| &r[..row]).copied().collect()
        } else {
            data.to_vec()
        };
        levels.push((mip, data));
    }
    levels.sort_by_key(|(mip, _)| *mip);
    // Keep only the contiguous chain from level 0 whose sizes match the format, so the GPU
    // upload never sees a short level.
    let mut mips = Vec::new();
    for (i, (mip, data)) in levels.into_iter().enumerate() {
        let (w, hgt) = ((h.width >> i).max(1), (h.height >> i).max(1));
        if mip as usize != i || data.len() != h.format.level_size(w, hgt) {
            break;
        }
        mips.push(data);
    }
    if mips.is_empty() {
        return Err(Error::Format(format!("{}: no usable mip level", h.name)));
    }
    Ok(Texture { name: h.name, width: h.width, height: h.height, format: h.format, mips })
}

impl Texture {
    /// A standalone `.d3dtx` file.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let ms = MetaStream::parse(bytes)?;
        let h = read_header(&mut ms.reader())?;
        assemble(h, ms.async_data)
    }

    /// A texture embedded in another object; returns it and the number of async bytes it used.
    pub(crate) fn read_embedded(r: &mut Reader, async_data: &[u8]) -> Result<(Self, usize), Error> {
        let h = read_header(r)?;
        let used = h.stored.iter().map(|(_, s, _)| s).sum();
        Ok((assemble(h, async_data)?, used))
    }

    /// Expand to RGBA8 when the GPU path cannot use the stored format directly.
    pub fn level0_rgba(&self) -> Option<Vec<u8>> {
        let px = &self.mips[0];
        match self.format {
            Format::Bgra8 => Some(px.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()),
            Format::A8 => Some(px.iter().flat_map(|&a| [255, 255, 255, a]).collect()),
            Format::L8 => Some(px.iter().flat_map(|&l| [l, l, l, 255]).collect()),
            Format::Argb4 => Some(
                px.chunks_exact(2)
                    .flat_map(|p| {
                        let v = u16::from_le_bytes([p[0], p[1]]);
                        let n = |s: u16| (((v >> s) & 0xf) as u8) * 17;
                        [n(8), n(4), n(0), n(12)]
                    })
                    .collect(),
            ),
            Format::Bc1 | Format::Bc2 | Format::Bc3 => Some(decode_bc(self.format, px, self.width, self.height)),
            _ => None,
        }
    }
}

/// RGB565 to 8-bit channels.
fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

/// Decode BC1 / BC2 / BC3 (DXT1/3/5) blocks to RGBA8 (the standard block formats: 4x4 texels,
/// two RGB565 endpoints with 2-bit indices, plus explicit 4-bit or interpolated 8-bit alpha).
fn decode_bc(format: Format, data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let block_size = if format == Format::Bc1 { 8 } else { 16 };
    let blocks_x = w.div_ceil(4);
    let mut out = vec![0u8; w * h * 4];
    for (bi, block) in data.chunks_exact(block_size).enumerate() {
        let (bx, by) = (bi % blocks_x * 4, bi / blocks_x * 4);
        if by >= h {
            break;
        }
        let (alpha, color) = if block_size == 16 { (Some(&block[..8]), &block[8..]) } else { (None, block) };
        let c0 = u16::from_le_bytes([color[0], color[1]]);
        let c1 = u16::from_le_bytes([color[2], color[3]]);
        let (e0, e1) = (rgb565(c0), rgb565(c1));
        let mix = |a: u8, b: u8, wa: u32, wb: u32, d: u32| ((a as u32 * wa + b as u32 * wb) / d) as u8;
        let mut palette = [[0u8; 4]; 4];
        palette[0] = [e0[0], e0[1], e0[2], 255];
        palette[1] = [e1[0], e1[1], e1[2], 255];
        if c0 > c1 || block_size == 16 {
            palette[2] = [mix(e0[0], e1[0], 2, 1, 3), mix(e0[1], e1[1], 2, 1, 3), mix(e0[2], e1[2], 2, 1, 3), 255];
            palette[3] = [mix(e0[0], e1[0], 1, 2, 3), mix(e0[1], e1[1], 1, 2, 3), mix(e0[2], e1[2], 1, 2, 3), 255];
        } else {
            palette[2] = [mix(e0[0], e1[0], 1, 1, 2), mix(e0[1], e1[1], 1, 1, 2), mix(e0[2], e1[2], 1, 1, 2), 255];
            palette[3] = [0, 0, 0, 0];
        }
        let indices = u32::from_le_bytes([color[4], color[5], color[6], color[7]]);
        // Alpha per texel: BC2 explicit 4-bit, BC3 two endpoints with 3-bit indices.
        let mut alphas = [255u8; 16];
        if let Some(a) = alpha {
            if format == Format::Bc2 {
                for (i, x) in alphas.iter_mut().enumerate() {
                    let nib = (a[i / 2] >> ((i % 2) * 4)) & 0xf;
                    *x = nib * 17;
                }
            } else {
                let (a0, a1) = (a[0] as u32, a[1] as u32);
                let mut table = [0u32; 8];
                table[0] = a0;
                table[1] = a1;
                if a0 > a1 {
                    for k in 1..7 {
                        table[k + 1] = (a0 * (7 - k as u32) + a1 * k as u32) / 7;
                    }
                } else {
                    for k in 1..5 {
                        table[k + 1] = (a0 * (5 - k as u32) + a1 * k as u32) / 5;
                    }
                    table[6] = 0;
                    table[7] = 255;
                }
                let bits = u64::from_le_bytes([a[2], a[3], a[4], a[5], a[6], a[7], 0, 0]);
                for (i, x) in alphas.iter_mut().enumerate() {
                    *x = table[((bits >> (3 * i)) & 7) as usize] as u8;
                }
            }
        }
        for i in 0..16 {
            let (x, y) = (bx + i % 4, by + i / 4);
            if x >= w || y >= h {
                continue;
            }
            let mut c = palette[((indices >> (2 * i)) & 3) as usize];
            if block_size == 16 {
                c[3] = alphas[i];
            }
            out[(y * w + x) * 4..][..4].copy_from_slice(&c);
        }
    }
    out
}
