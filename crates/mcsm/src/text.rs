//! Text drawn with the game's own bitmap fonts, as world-space quads (the engine's text agents
//! live in the scene like any other renderable).

use crate::data::{GameData, Props};
use crate::stage::Ctx;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use ttmeta::PropertySet;

#[derive(Clone)]
pub struct TextStyle {
    pub font: ttmeta::Symbol,
    pub color: [f32; 4],
    pub shadow_color: [f32; 4],
    /// Shadow offset in font pixels.
    pub shadow: f32,
    /// World units per font pixel.
    pub unit: f32,
    pub offset: Vec3,
    /// 1 = left, 2 = center, 3 = right (the engine's EnumHTextAlignmentType values seen in data).
    pub align: i32,
    /// "Text Render Layer": UI draws by layer; text is nudged toward the camera by it.
    pub layer: i32,
    /// "Text Alignment Vertical": 1 top, 2 middle, 3 bottom (where the agent sits on the line).
    pub valign: i32,
}

impl TextStyle {
    pub fn from_props(props: &mut Props, data: &GameData, set: &PropertySet) -> Option<Self> {
        let font = props.symbol(data, set, "Text Font")?;
        let ppu = props.float(data, set, "Text Pixels Per World Unit").unwrap_or(100.0);
        let scale = props.float(data, set, "Text Scale").unwrap_or(1.0) * props.float(data, set, "Text Glyph Scale").unwrap_or(1.0);
        let off = props.vec3(data, set, "Text Offset").unwrap_or([0.0; 3]);
        Some(TextStyle {
            font,
            color: props.color(data, set, "Text Color").unwrap_or([1.0; 4]),
            shadow_color: props.color(data, set, "Text Shadow Color").unwrap_or([0.0, 0.0, 0.0, 1.0]),
            shadow: props.float(data, set, "Text Shadow Height").unwrap_or(0.0),
            unit: scale / ppu,
            offset: Vec3::from_array(off),
            align: props.int(data, set, "Text Alignment Horizontal").unwrap_or(1),
            valign: props.int(data, set, "Text Alignment Vertical").unwrap_or(2),
            layer: props.int(data, set, "Text Render Layer").or_else(|| props.int(data, set, "Render Layer")).unwrap_or(0),
        })
    }
}

/// Spawn `text` as children of `parent`. Returns the text's width in world units and the
/// materials of its main (non-shadow) pass, so chores can recolor it.
pub fn spawn_text(ctx: &mut Ctx, parent: Entity, text: &str, style: &TextStyle, layers: &RenderLayers) -> (f32, Vec<Handle<StandardMaterial>>) {
    spawn_line(ctx, parent, text, style, layers, 0.0)
}

/// Spawn `text` word-wrapped to `wrap` world units ("Text Width"), lines stacked downward
/// from the parent (or upward from it with `bottom`, the engine's "Bottom" vertical alignment).
/// Explicit newlines are honoured. Returns the block's (width, height) and main-pass materials.
pub fn spawn_text_block(
    ctx: &mut Ctx,
    parent: Entity,
    text: &str,
    style: &TextStyle,
    layers: &RenderLayers,
    wrap: f32,
    bottom: bool,
) -> (Vec2, Vec<Handle<StandardMaterial>>) {
    let Some(font) = ctx.cache.font(ctx.data, ctx.images, style.font) else { return (Vec2::ZERO, Vec::new()) };
    let measure = |s: &str| -> f32 {
        s.chars().filter_map(|c| font.0.glyphs.get(&(c as u32)).or_else(|| font.0.glyphs.get(&('?' as u32)))).map(|g| g.advance).sum::<f32>() * style.unit
    };
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if !line.is_empty() && measure(&candidate) > wrap {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    let line_height = font.0.size * style.unit;
    let n = lines.len();
    let mut width = 0.0f32;
    let mut mats = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        // Line i of n: top-down, or the last line sitting on the anchor for bottom alignment.
        let y = if bottom { (n - 1 - i) as f32 * line_height } else { -(i as f32) * line_height };
        let (w, m) = spawn_line(ctx, parent, line, style, layers, y);
        width = width.max(w);
        mats.extend(m);
    }
    (Vec2::new(width, n as f32 * line_height), mats)
}

fn spawn_line(ctx: &mut Ctx, parent: Entity, text: &str, style: &TextStyle, layers: &RenderLayers, y: f32) -> (f32, Vec<Handle<StandardMaterial>>) {
    let mut main_pass = Vec::new();
    let Some(font) = ctx.cache.font(ctx.data, ctx.images, style.font) else {
        warn!("font {:?} not found", style.font);
        return (0.0, main_pass);
    };
    let (font, pages) = (&font.0, &font.1);
    let u = style.unit;

    // Lay out one line in font pixels, y up, origin at the line's top-left.
    let mut width = 0.0f32;
    let mut quads: Vec<(u32, [f32; 4], [f32; 4])> = Vec::new(); // page, (x0, y0, x1, y1), uv
    for ch in text.chars() {
        let Some(g) = font.glyphs.get(&(ch as u32)).or_else(|| font.glyphs.get(&('?' as u32))) else { continue };
        let x0 = width + g.offset[0];
        let y0 = -g.offset[1];
        quads.push((g.page, [x0, y0, x0 + g.size[0], y0 - g.size[1]], g.uv));
        width += g.advance;
    }
    let x_shift = match style.align {
        2 => -width * 0.5,
        3 => -width,
        _ => 0.0,
    };
    // Place the line vertically on the agent (centered by default); the line height is `size`.
    let y_shift = match style.valign {
        1 => 0.0,
        3 => font.size,
        _ => font.size * 0.5,
    };

    for (pass, (color, dx, dz)) in [(style.shadow_color, style.shadow, 0.0005), (style.color, 0.0, 0.0)].into_iter().enumerate() {
        if pass == 0 && style.shadow <= 0.0 {
            continue;
        }
        for (page, handle) in pages.iter().enumerate() {
            let mut pos = Vec::new();
            let mut uv = Vec::new();
            let mut idx = Vec::new();
            for (p, r, t) in quads.iter().filter(|q| q.0 as usize == page) {
                let _ = p;
                let base = pos.len() as u32;
                let (x0, y0, x1, y1) = ((r[0] + x_shift + dx) * u, (r[1] + y_shift - dx) * u, (r[2] + x_shift + dx) * u, (r[3] + y_shift - dx) * u);
                pos.extend([[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]]);
                uv.extend([[t[0], t[2]], [t[1], t[2]], [t[1], t[3]], [t[0], t[3]]]);
                idx.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
            }
            if pos.is_empty() {
                continue;
            }
            let n = pos.len();
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            mesh.insert_indices(Indices::U32(idx));
            let material = StandardMaterial {
                base_color: Color::srgba(color[0], color[1], color[2], color[3]),
                base_color_texture: Some(handle.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            };
            let material = ctx.materials.add(material);
            if pass == 1 {
                main_pass.push(material.clone());
            }
            ctx.commands.spawn((
                Mesh3d(ctx.meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::from_translation(style.offset + Vec3::new(0.0, y, 0.0005 + 0.001 * style.layer as f32 - dz)),
                layers.clone(),
                ChildOf(parent),
            ));
        }
    }
    (width * u, main_pass)
}
