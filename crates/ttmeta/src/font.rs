//! Telltale bitmap fonts (`.font`).
//!
//! Layout: block-string name, bool, f32 size, f32 base (ascent), f32, f32, block { u32 glyph
//! count, per glyph { u32 char, u32 page, u32 channel mask, f32 u0, u1, v0, v1, f32 width,
//! height, f32 x offset, y offset, f32 advance } }, block { u32 page count, per page an embedded
//! T3Texture }. Page pixels are consecutive in the async section. Field names per TTG Tools'
//! FontClass (GPL-3.0, see CREDITS.md); sizes confirmed against this game's files.

use crate::stream::MetaStream;
use crate::texture::Texture;
use crate::Error;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub page: u32,
    /// Texture rectangle in normalized coordinates: u0, u1, v0, v1.
    pub uv: [f32; 4],
    /// Size in pixels at the font's native size.
    pub size: [f32; 2],
    pub offset: [f32; 2],
    pub advance: f32,
}

#[derive(Debug, Clone)]
pub struct Font {
    pub name: String,
    /// Native pixel size (line height).
    pub size: f32,
    pub base: f32,
    pub glyphs: HashMap<u32, Glyph>,
    pub pages: Vec<Texture>,
}

impl Font {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let ms = MetaStream::parse(bytes)?;
        let mut r = ms.reader();
        let name = r.block_string()?;
        let _flag = r.bool()?;
        let size = r.f32()?;
        let base = r.f32()?;
        r.skip(8)?;
        let glyph_end = r.block()?;
        let count = r.u32()?;
        let mut glyphs = HashMap::with_capacity(count as usize);
        for _ in 0..count {
            let ch = r.u32()?;
            let page = r.u32()?;
            let _channels = r.u32()?;
            let uv = r.vec::<4>()?;
            let size = r.vec::<2>()?;
            let offset = r.vec::<2>()?;
            let advance = r.f32()?;
            glyphs.insert(ch, Glyph { page, uv, size, offset, advance });
        }
        r.seek(glyph_end)?;
        let pages_end = r.block()?;
        let page_count = r.u32()?;
        let mut pages = Vec::with_capacity(page_count as usize);
        let mut at = 0;
        for _ in 0..page_count {
            let (tex, used) = Texture::read_embedded(&mut r, &ms.async_data[at..])?;
            at += used;
            pages.push(tex);
        }
        r.seek(pages_end)?;
        Ok(Font { name, size, base, glyphs, pages })
    }
}
