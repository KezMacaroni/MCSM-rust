//! The material for unbaked world surfaces: a `StandardMaterial` (texture, alpha, skinning)
//! extended with the Telltale lighting of the surface's lighting group (see `tt_lit.wgsl`).

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

pub const MAX_LIGHTS: usize = 8;

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct TtLight {
    pub color: Vec4,
    pub vector: Vec4,
    /// kind (0 ambient, 1 directional, 2 point), wrap around, min distance, max distance.
    pub params: Vec4,
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct TtLightingUniform {
    pub lights: [TtLight; MAX_LIGHTS],
    pub count: u32,
    pub translucent: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct TtLighting {
    #[uniform(100)]
    pub uniform: TtLightingUniform,
}

impl MaterialExtension for TtLighting {
    fn fragment_shader() -> ShaderRef {
        "embedded://mcsm/tt_lit.wgsl".into()
    }
}

pub type LitMaterial = ExtendedMaterial<StandardMaterial, TtLighting>;

pub struct LitPlugin;

impl Plugin for LitPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "tt_lit.wgsl");
        app.add_plugins(MaterialPlugin::<LitMaterial>::default());
    }
}
