//! The mouse cursor: the game's own (`ui_cursor_default.d3dtx`, `ui_cursor_highlight.d3dtx`).
//!
//! `cursor.prop` sizes it to 32 pixels with offset (-16, 16), i.e. the image is centred on the
//! pointer; `Cursor_Highlight` (Cursor.lua) switches to the highlight image over buttons.

use crate::data::{GameData, Props};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow};

#[derive(Resource)]
pub struct GameCursor {
    default: Handle<Image>,
    highlight: Handle<Image>,
    hotspot: (u16, u16),
    /// Whether the pointer is over something clickable (set by the menu each frame).
    pub highlighted: bool,
    shown: Option<bool>,
}

/// Decode a cursor texture to RGBA8 at `size` pixels (box-filtered down from the texture).
fn cursor_image(data: &GameData, name: &str, size: u32) -> Option<Image> {
    let tex = ttmeta::Texture::parse(&data.read(name)?).ok()?;
    let rgba = tex.level0_rgba()?;
    let (w, h) = (tex.width, tex.height);
    let size = size.min(w).max(1);
    let step = (w / size).max(1);
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0u32; 4];
            let mut n = 0;
            for dy in 0..step {
                for dx in 0..step {
                    let (sx, sy) = (x * step + dx, y * step + dy);
                    if sx < w && sy < h {
                        let i = ((sy * w + sx) * 4) as usize;
                        for c in 0..4 {
                            acc[c] += rgba[i + c] as u32;
                        }
                        n += 1;
                    }
                }
            }
            let o = ((y * size + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    Some(Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        out,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}

pub fn load(mut commands: Commands, data: Option<Res<GameData>>, mut props: ResMut<Props>, mut images: ResMut<Assets<Image>>) {
    let Some(data) = data else { return };
    let cursor_props = props.file(&data, ttmeta::Symbol::of("cursor.prop"));
    let size = cursor_props.as_deref().and_then(|p| props.float(&data, p, "Cursor Size")).unwrap_or(32.0).max(1.0);
    let offset_x = cursor_props.as_deref().and_then(|p| props.float(&data, p, "Cursor Offset X")).unwrap_or(-16.0);
    let offset_y = cursor_props.as_deref().and_then(|p| props.float(&data, p, "Cursor Offset Y")).unwrap_or(16.0);
    let (Some(default), Some(highlight)) =
        (cursor_image(&data, "ui_cursor_default.d3dtx", size as u32), cursor_image(&data, "ui_cursor_highlight.d3dtx", size as u32))
    else {
        warn!("cursor textures missing; keeping the system cursor");
        return;
    };
    // The image is drawn at the pointer plus (offset x, -offset y): the hotspot is the inverse.
    let hotspot = ((-offset_x).clamp(0.0, size - 1.0) as u16, offset_y.clamp(0.0, size - 1.0) as u16);
    commands.insert_resource(GameCursor { default: images.add(default), highlight: images.add(highlight), hotspot, highlighted: false, shown: None });
}

pub fn apply(mut commands: Commands, cursor: Option<ResMut<GameCursor>>, windows: Query<Entity, With<PrimaryWindow>>) {
    let Some(mut cursor) = cursor else { return };
    if cursor.shown == Some(cursor.highlighted) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let handle = if cursor.highlighted { cursor.highlight.clone() } else { cursor.default.clone() };
    let icon: CursorIcon = CustomCursor::Image(CustomCursorImage { handle, hotspot: cursor.hotspot, ..default() }).into();
    commands.entity(window).insert(icon);
    cursor.shown = Some(cursor.highlighted);
}
