//! Telltale -> Bevy conversions.
//!
//! Both are right-handed with +Y up, so positions, rotations and normals carry over unchanged.
//! The difference is the camera: a Telltale camera looks down its local +Z, a Bevy camera down
//! its local -Z, so camera agents get an extra half turn about their local Y ([`CAMERA_FIX`]).
//! Evidence: the main-menu UI camera sits at z = +41 with a half-turn about Y, and the title
//! quad's U coordinate grows toward +X, so +X must be screen-right for the logo to read.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ttmeta::mesh::{TextureRef, BAKE, DIFFUSE};
use ttmeta::props::Transform as TtTransform;
use ttmeta::texture::Format;
use ttmeta::Symbol;

pub fn position(p: [f32; 3]) -> Vec3 {
    Vec3::from_array(p)
}

pub fn rotation(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize()
}

/// Applied in a camera agent's local frame (see the module docs).
pub const CAMERA_FIX: Quat = Quat::from_xyzw(0.0, 1.0, 0.0, 0.0);

pub fn transform(t: &TtTransform) -> Transform {
    Transform { translation: position(t.pos), rotation: rotation(t.rot), scale: Vec3::ONE }
}

pub fn image(tex: &ttmeta::Texture, pixel_art: bool) -> Option<Image> {
    let (format, mips): (TextureFormat, Vec<&[u8]>) = match tex.format {
        Format::Bc1 => (TextureFormat::Bc1RgbaUnormSrgb, tex.mips.iter().map(Vec::as_slice).collect()),
        Format::Bc2 => (TextureFormat::Bc2RgbaUnormSrgb, tex.mips.iter().map(Vec::as_slice).collect()),
        Format::Bc3 => (TextureFormat::Bc3RgbaUnormSrgb, tex.mips.iter().map(Vec::as_slice).collect()),
        Format::Bc4 => (TextureFormat::Bc4RUnorm, tex.mips.iter().map(Vec::as_slice).collect()),
        Format::Bc5 => (TextureFormat::Bc5RgUnorm, tex.mips.iter().map(Vec::as_slice).collect()),
        Format::Bgra8 => (TextureFormat::Bgra8UnormSrgb, tex.mips.iter().map(Vec::as_slice).collect()),
        Format::A8 | Format::L8 | Format::Argb4 => {
            let rgba = tex.level0_rgba()?;
            return Some(finish(
                Image::new(
                    Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    rgba,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::RENDER_WORLD,
                ),
                pixel_art,
            ));
        }
    };
    if tex.format.is_block() && (tex.width % 4 != 0 || tex.height % 4 != 0) {
        warn!("{}: {}x{} block-compressed texture is not 4-aligned", tex.name, tex.width, tex.height);
        return None;
    }
    let mut img = Image::new_uninit(
        Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = mips.len() as u32;
    img.data = Some(mips.concat());
    Some(finish(img, pixel_art))
}

fn finish(mut img: Image, pixel_art: bool) -> Image {
    img.sampler = if pixel_art {
        // Blocky up close, filtered with mipmaps at a distance so far-off blocks do not shimmer.
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: bevy::image::ImageAddressMode::Repeat,
            address_mode_v: bevy::image::ImageAddressMode::Repeat,
            mag_filter: bevy::image::ImageFilterMode::Nearest,
            min_filter: bevy::image::ImageFilterMode::Linear,
            mipmap_filter: bevy::image::ImageFilterMode::Linear,
            ..ImageSamplerDescriptor::nearest()
        })
    } else {
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: bevy::image::ImageAddressMode::Repeat,
            address_mode_v: bevy::image::ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        })
    };
    img
}

pub struct MeshPart {
    pub mesh: Handle<Mesh>,
    pub blend: ttmeta::mesh::Blend,
    pub diffuse: Option<Symbol>,
    pub bake: Option<Symbol>,
    pub tint: [f32; 4],
    /// For skinned parts: the bones the mesh's joint indices refer to, in order.
    pub palette: Option<Vec<Symbol>>,
}

pub struct LoadedMesh {
    pub parts: Vec<MeshPart>,
}

fn texture_symbol(r: &TextureRef) -> Symbol {
    match r {
        TextureRef::Symbol(s) => *s,
        TextureRef::Name(n) if n.contains('.') => Symbol::of(n),
        TextureRef::Name(n) => Symbol::of(&format!("{n}.d3dtx")),
    }
}

/// One Bevy mesh per submesh, holding only the vertices that submesh uses.
pub fn mesh(m: &ttmeta::Mesh, meshes: &mut Assets<Mesh>) -> LoadedMesh {
    let mut parts = Vec::new();
    let slot = |sm: &ttmeta::mesh::Submesh, s: usize| -> Option<Symbol> {
        let i = sm.textures[s]? as usize;
        m.textures.get(s)?.get(i).map(texture_symbol)
    };
    for sm in &m.submeshes {
        let range = sm.first_index as usize..(sm.first_index + sm.index_count) as usize;
        let Some(idx) = m.indices.get(range) else { continue };
        if idx.is_empty() {
            continue;
        }
        let mut remap = std::collections::HashMap::new();
        let mut order = Vec::new();
        let local: Vec<u32> = idx
            .iter()
            .map(|&g| {
                *remap.entry(g).or_insert_with(|| {
                    order.push(g as usize);
                    (order.len() - 1) as u32
                })
            })
            .collect();
        if order.iter().any(|&g| g >= m.positions.len()) {
            continue;
        }
        let pick = |v: &Vec<[f32; 2]>| -> Option<Vec<[f32; 2]>> {
            (!v.is_empty()).then(|| order.iter().map(|&g| v.get(g).copied().unwrap_or_default()).collect())
        };
        // Kept in the main world too: later scenes reuse cached meshes and read them for lighting.
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            order.iter().map(|&g| position(m.positions[g]).to_array()).collect::<Vec<_>>(),
        );
        if m.normals.len() == m.positions.len() {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_NORMAL,
                order.iter().map(|&g| position(m.normals[g]).normalize_or_zero().to_array()).collect::<Vec<_>>(),
            );
        }
        // Diffuse uses the first channel; the baked lightmap uses the second when present.
        if let Some(uv) = pick(&m.uvs[0]) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        }
        if let Some(uv) = pick(&m.uvs[1]) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv);
        }
        // Skinning: joint bytes are palette index x 3; weights are renormalized (they sum to
        // 32767/32767 only approximately). Rigid parts have no weights: one bone per vertex.
        let palette = sm.palette.and_then(|p| m.palettes.get(p as usize)).filter(|_| m.joints.len() == m.positions.len());
        if let Some(palette) = palette {
            let last = palette.len().saturating_sub(1) as u16;
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_INDEX,
                bevy::mesh::VertexAttributeValues::Uint16x4(order.iter().map(|&g| m.joints[g].map(|j| (j as u16 / 3).min(last))).collect()),
            );
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_WEIGHT,
                order
                    .iter()
                    .map(|&g| {
                        let w = m.weights.get(g).map_or([1.0, 0.0, 0.0, 0.0], |w| w.map(|w| w.max(0.0)));
                        let sum: f32 = w.iter().sum();
                        if sum > 0.0 { w.map(|x| x / sum) } else { [1.0, 0.0, 0.0, 0.0] }
                    })
                    .collect::<Vec<_>>(),
            );
        }
        mesh.insert_indices(Indices::U32(local));
        if !mesh.contains_attribute(Mesh::ATTRIBUTE_NORMAL) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; order.len()]);
        }
        parts.push(MeshPart { mesh: meshes.add(mesh), blend: sm.blend, diffuse: slot(sm, DIFFUSE), bake: slot(sm, BAKE), tint: sm.tint, palette: palette.cloned() });
    }
    LoadedMesh { parts }
}
