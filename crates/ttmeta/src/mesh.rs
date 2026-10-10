//! D3DMesh, version 18 (this game's static and UI meshes).
//!
//! Field order follows the `ParseV18` path of HeitorSpectre's Telltale-D3DMesh-Editor
//! (GPL-3.0, see CREDITS.md). Index values are absolute into the vertex buffers.
//!
//! Skinning: the mesh lists bone palettes (per bone: symbol, 48 bytes of bounds); each submesh
//! names one palette, and a vertex's four bone bytes are palette index x 3 (offsets of 3-row
//! matrices), with four i16 weights summing to 32767. Vertices are in the skeleton's bind pose.

use crate::stream::{MetaStream, Reader};
use crate::symbol::Symbol;
use crate::Error;

/// Texture slots, in the order the v18 header and texture groups list them.
pub const SLOTS: [&str; 14] = [
    "diffuse", "bake", "bump", "environment", "detail_diffuse", "detail_bump", "specular", "tex8", "gradient", "tex10",
    "shadow", "emissive", "alternate_bump", "occlusion",
];
pub const DIFFUSE: usize = 0;
pub const BAKE: usize = 1;

#[derive(Debug, Clone)]
pub enum TextureRef {
    Name(String),
    Symbol(Symbol),
}

#[derive(Debug, Clone)]
pub struct Submesh {
    /// Range of triangle indices (into `Mesh::indices`).
    pub first_index: u32,
    pub index_count: u32,
    /// Per slot, an index into `Mesh::textures[slot]`, if the slot is used.
    pub textures: [Option<u32>; 14],
    pub tint: [f32; 4],
    pub blend: Blend,
    /// Index into `Mesh::palettes` for skinned submeshes.
    pub palette: Option<u32>,
}

/// Submesh blend mode (u32 at offset 0x6c of the submesh's material block). Meanings inferred
/// from which textures use each value across the menu archives: 0 on opaque props and
/// buildings, 1 on glass, clouds and matte paintings, 3 on foliage/flowers/bottles (cut-outs),
/// 5 only on the night-sky stars layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    Alpha,
    AlphaTest,
    Add,
    Other(u32),
}

impl Blend {
    fn from_code(c: u32) -> Blend {
        match c {
            0 => Blend::Opaque,
            1 => Blend::Alpha,
            2..=4 => Blend::AlphaTest,
            5 => Blend::Add,
            x => Blend::Other(x),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub name: String,
    pub bounds: ([f32; 3], [f32; 3]),
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// UV channels 1..=4; empty when absent. V is in the file's convention (not flipped).
    pub uvs: [Vec<[f32; 2]>; 4],
    pub indices: Vec<u16>,
    /// Per vertex, four palette indices and weights (skinned meshes only).
    pub joints: Vec<[u8; 4]>,
    pub weights: Vec<[f32; 4]>,
    /// Bone palettes: the skeleton bones a submesh's vertex joint indices refer to.
    pub palettes: Vec<Vec<Symbol>>,
    pub submeshes: Vec<Submesh>,
    /// Per slot, the texture names referenced by submeshes.
    pub textures: Vec<Vec<TextureRef>>,
}

#[derive(Clone, Copy, Default)]
struct Attr {
    offset: u32,
    format: u32,
}

const ATTR_ORDER: [&str; 13] =
    ["position", "uv1", "normals", "weights", "bones", "colors", "unknown1", "binormals", "tangents", "uv2", "uv3", "uv4", "uv5"];

fn opt_index(r: &mut Reader) -> Result<Option<u32>, Error> {
    let v = r.u32()?;
    Ok((v != u32::MAX).then_some(v))
}

impl Mesh {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let ms = MetaStream::parse(bytes)?;
        let mut r = ms.reader();
        let name = r.block_string()?;
        let version = r.u32()?;
        if version != 18 {
            return Err(Error::Format(format!("{name}: D3DMesh version {version} (only 18 is supported)")));
        }
        r.skip(4)?;
        let bounds = (r.vec::<3>()?, r.vec::<3>()?);
        r.skip_block()?;

        let _ = r.u32()?;
        let sub_count = r.u32()?;
        if sub_count > 4096 {
            return Err(Error::Format(format!("{name}: {sub_count} submeshes")));
        }
        let mut raw = Vec::with_capacity(sub_count as usize);
        for _ in 0..sub_count {
            r.u32()?;
            let palette = opt_index(&mut r)?;
            r.u32()?;
            r.u32()?;
            let _vmin = r.u32()?;
            let _vmax = r.u32()?;
            let first = r.u32()?;
            let tris = r.u32()?;
            r.skip(12 + 12 + 4 + 12 + 4)?;
            let header_end = r.block()?;
            let mut textures = [None; 14];
            for t in &mut textures {
                *t = opt_index(&mut r)?;
            }
            r.seek(header_end)?;
            let material = r.bytes(0x88)?;
            let blend = Blend::from_code(u32::from_le_bytes(material[0x6c..0x70].try_into().unwrap()));
            let tint = r.vec::<4>()?;
            raw.push(Submesh { first_index: first, index_count: tris * 3, textures, tint, blend, palette });
        }

        r.skip_block()?;
        let _ = r.u32()?;
        let mut palettes = Vec::new();
        for _ in 0..r.u32()? {
            let bones = r.u32()? as usize;
            let mut p = Vec::with_capacity(bones);
            for _ in 0..bones {
                p.push(r.symbol()?);
                r.skip(48)?;
            }
            palettes.push(p);
        }
        r.skip_block()?;
        r.skip_block()?;
        r.skip_block()?;
        r.skip(8)?;

        let _ = r.u32()?;
        let mut textures = Vec::with_capacity(SLOTS.len());
        for _ in SLOTS {
            let n = r.u32()?;
            let mut names = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let end = r.block()?;
                let content = end - r.pos;
                names.push(if content == 8 { TextureRef::Symbol(r.symbol()?) } else { TextureRef::Name(r.string()?) });
                r.seek(end)?;
                r.u32()?;
                r.skip(24)?;
                if u32::from_le_bytes(r.buf[r.pos..r.pos + 4].try_into().unwrap()) != 0x14 {
                    r.skip(8)?;
                }
                r.u32()?;
                r.skip(24)?;
            }
            textures.push(names);
        }

        let uv_scale = r.vec::<10>()?;
        r.skip_block()?;
        r.skip_block()?;
        r.skip(1 + 4)?;

        let index_count = r.u32()? as usize;
        let index_format = r.u32()?;
        r.skip(4)?;
        if index_format != 0 {
            return Err(Error::Format(format!("{name}: index format {index_format}")));
        }
        let indices = (0..index_count).map(|_| r.u16()).collect::<Result<Vec<_>, _>>()?;

        let mut mesh = Mesh { name, bounds, indices, submeshes: raw, textures, palettes, ..Default::default() };
        let scales = [[uv_scale[0], uv_scale[1]], [uv_scale[4], uv_scale[5]], [uv_scale[6], uv_scale[7]], [uv_scale[8], uv_scale[9]]];
        for _ in 0..2 {
            let count = r.u32()? as usize;
            let stride = r.u32()? as usize;
            r.skip(8)?;
            r.u32()?;
            let mut attrs = [Attr::default(); 13];
            for a in &mut attrs {
                *a = Attr { offset: r.u32()?, format: { r.u32()?; r.u32()? } };
            }
            let start = r.pos;
            r.skip(count * stride)?;
            let at = |i: usize, a: Attr| Reader::at(r.buf, start + i * stride + a.offset as usize);
            for (slot, attr) in ATTR_ORDER.iter().zip(attrs) {
                if attr.format == 0 {
                    continue;
                }
                match *slot {
                    "position" => {
                        mesh.positions = (0..count).map(|i| at(i, attr).vec::<3>()).collect::<Result<_, _>>()?;
                    }
                    "normals" => {
                        mesh.normals = (0..count).map(|i| read_normal(&mut at(i, attr), attr.format)).collect::<Result<_, _>>()?;
                    }
                    "uv1" | "uv2" | "uv3" | "uv4" => {
                        let ch = (slot.as_bytes()[2] - b'1') as usize;
                        mesh.uvs[ch] =
                            (0..count).map(|i| read_uv(&mut at(i, attr), attr.format, scales[ch])).collect::<Result<_, _>>()?;
                    }
                    "bones" => {
                        if attr.format != 8 {
                            return Err(Error::Format(format!("bone index format {}", attr.format)));
                        }
                        mesh.joints = (0..count)
                            .map(|i| Ok(at(i, attr).bytes(4)?.try_into().unwrap()))
                            .collect::<Result<_, Error>>()?;
                    }
                    "weights" => {
                        mesh.weights = (0..count).map(|i| read_weights(&mut at(i, attr), attr.format)).collect::<Result<_, _>>()?;
                    }
                    _ => {}
                }
            }
        }
        // Some meshes are deliberately empty placeholders (no triangles, no vertices).
        if mesh.positions.is_empty() && !mesh.indices.is_empty() {
            return Err(Error::Format(format!("{}: {} indices but no positions", mesh.name, mesh.indices.len())));
        }
        Ok(mesh)
    }
}

fn read_normal(r: &mut Reader, format: u32) -> Result<[f32; 3], Error> {
    match format {
        2 => {
            let b = r.bytes(3)?;
            Ok([b[0] as i8 as f32 / 127.0, b[1] as i8 as f32 / 127.0, b[2] as i8 as f32 / 127.0])
        }
        4 => Ok([r.u16()? as i16, r.u16()? as i16, r.u16()? as i16].map(|x| x as f32 / 32767.0)),
        1 => r.vec::<3>(),
        f => Err(Error::Format(format!("normal format {f}"))),
    }
}

fn read_weights(r: &mut Reader, format: u32) -> Result<[f32; 4], Error> {
    match format {
        4 => Ok([r.u16()?, r.u16()?, r.u16()?, r.u16()?].map(|x| x as i16 as f32 / 32767.0)),
        1 => r.vec::<4>(),
        f => Err(Error::Format(format!("weight format {f}"))),
    }
}

fn read_uv(r: &mut Reader, format: u32, scale: [f32; 2]) -> Result<[f32; 2], Error> {
    Ok(match format {
        1 => r.vec::<2>()?,
        4 => [r.u16()? as i16 as f32 / 32767.0 * scale[0], r.u16()? as i16 as f32 / 32767.0 * scale[1]],
        5 => [r.u16()? as f32 / 65535.0 * scale[0], r.u16()? as f32 / 65535.0 * scale[1]],
        11 => [half::f16::from_bits(r.u16()?).to_f32(), half::f16::from_bits(r.u16()?).to_f32()],
        f => return Err(Error::Format(format!("uv format {f}"))),
    })
}
