//! A scene's FX post-processing, from its scene agent's properties: the filmic tone curve and
//! vignette (tt_grade.wgsl) and distance fog ("Fog Near / Far Plane", "Fog Color").

use crate::data::{GameData, Props};
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;
use ttmeta::PropertySet;

#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default, Debug)]
pub struct SceneGrade {
    pub toe: Vec4,
    pub shoulder: Vec4,
    pub pivot: Vec4,
    pub sign: Vec4,
    pub tonemap: Vec4,
    pub vignette: Vec4,
    pub vignette_tint: Vec4,
}

impl FullscreenMaterial for SceneGrade {
    fn fragment_shader() -> ShaderRef {
        "embedded://mcsm/tt_grade.wgsl".into()
    }
}

pub struct GradePlugin;

impl Plugin for GradePlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "tt_grade.wgsl");
        app.add_plugins(FullscreenMaterialPlugin::<SceneGrade>::default());
    }
}

fn vec3(props: &mut Props, data: &GameData, set: &PropertySet, key: &str, default: f32) -> Vec3 {
    props.vec3(data, set, key).map_or(Vec3::splat(default), Vec3::from_array)
}

/// The scene's grade and fog, if "After Effects Enabled" / "Fog Enabled".
pub fn scene_effects(props: &mut Props, data: &GameData, scene_agent: &PropertySet) -> (Option<SceneGrade>, Option<DistanceFog>) {
    let after_effects = props.bool(data, scene_agent, "After Effects Enabled").unwrap_or(false);
    let mut grade = None;
    if after_effects {
        let tonemap = props.bool(data, scene_agent, "FX Tonemap Enabled").unwrap_or(false) && props.bool(data, scene_agent, "FX Tonemap RGB Enabled").unwrap_or(false);
        let vignette = props.bool(data, scene_agent, "FX Vignette Tint Enabled").unwrap_or(false);
        if tonemap || vignette {
            let tint = props.color(data, scene_agent, "FX Vignette Tint").unwrap_or([0.0, 0.0, 0.0, 1.0]);
            grade = Some(SceneGrade {
                toe: vec3(props, data, scene_agent, "FX Tonemap RGB Toe Intensities", 0.0).extend(0.0),
                shoulder: vec3(props, data, scene_agent, "FX Tonemap RGB Shoulder Intensities", 0.0).extend(0.0),
                pivot: vec3(props, data, scene_agent, "FX Tonemap RGB Pivots", 0.5).extend(0.0),
                sign: vec3(props, data, scene_agent, "FX Tonemap RGB Signs", 1.0).extend(0.0),
                // The RGB curve applies in full: blending it in at "FX Tonemap Intensity" (0.3 in
                // the key art) leaves the image far from the game's (its blue-violet shadows and
                // warm highlights come from the full curve).
                tonemap: Vec4::new(if tonemap { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0),
                vignette: Vec4::new(
                    props.float(data, scene_agent, "FX Vignette Corners").unwrap_or(0.0),
                    props.float(data, scene_agent, "FX Vignette Falloff").unwrap_or(1.0),
                    vignette as u8 as f32,
                    0.0,
                ),
                vignette_tint: Vec4::from_array(tint),
            });
        }
    }
    let fog = props.bool(data, scene_agent, "Fog Enabled").unwrap_or(false).then(|| {
        let c = props.color(data, scene_agent, "Fog Color").unwrap_or([1.0; 4]);
        DistanceFog {
            color: Color::srgba(c[0], c[1], c[2], 1.0),
            falloff: FogFalloff::Linear {
                start: props.float(data, scene_agent, "Fog Near Plane").unwrap_or(10.0),
                end: props.float(data, scene_agent, "Fog Far Plane").unwrap_or(1000.0),
            },
            ..default()
        }
    });
    (grade, fog)
}
