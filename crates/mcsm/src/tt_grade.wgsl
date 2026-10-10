// A scene's "FX" post-processing (scene agent properties): the per-channel filmic tone curve
// ("FX Tonemap RGB ..." toe / shoulder / pivot / sign, blended in by "FX Tonemap Intensity")
// and the vignette ("FX Vignette Corners / Falloff / Tint").
//
// The curve, per channel in display (gamma) space with pivot p: below p a power toe, above p a
// power shoulder; "sign" +1 steepens (more contrast), -1 flattens (the key art's blue channel:
// lifted blue shadows, softened blue highlights).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

struct SceneGrade {
    toe: vec4<f32>,
    shoulder: vec4<f32>,
    pivot: vec4<f32>,
    sign: vec4<f32>,
    // x: tonemap intensity (0 = off)
    tonemap: vec4<f32>,
    // x: corners, y: falloff, z: enabled
    vignette: vec4<f32>,
    vignette_tint: vec4<f32>,
}

@group(0) @binding(2) var<uniform> grade: SceneGrade;

fn channel(x: f32, p: f32, toe: f32, shoulder: f32, sign: f32) -> f32 {
    let x0 = clamp(x, 0.0, 1.0);
    let pp = clamp(p, 0.01, 0.99);
    if x0 < pp {
        return pp * pow(x0 / pp, max(1.0 + toe * sign, 0.05));
    }
    return 1.0 - (1.0 - pp) * pow((1.0 - x0) / (1.0 - pp), max(1.0 + shoulder * sign, 0.05));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let src = textureSample(screen_texture, texture_sampler, in.uv);
    var c = pow(max(src.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    if grade.tonemap.x > 0.0 {
        let curved = vec3<f32>(
            channel(c.r, grade.pivot.r, grade.toe.r, grade.shoulder.r, grade.sign.r),
            channel(c.g, grade.pivot.g, grade.toe.g, grade.shoulder.g, grade.sign.g),
            channel(c.b, grade.pivot.b, grade.toe.b, grade.shoulder.b, grade.sign.b),
        );
        c = mix(c, curved, grade.tonemap.x);
    }
    if grade.vignette.z > 0.0 {
        // 0 at the centre, 1 in the corners.
        let d = length((in.uv - vec2<f32>(0.5)) * 2.0) / sqrt(2.0);
        let v = clamp(1.0 - grade.vignette.x * pow(d, grade.vignette.y), 0.0, 1.0);
        c = mix(grade.vignette_tint.rgb, c, v);
    }
    return vec4<f32>(pow(c, vec3<f32>(2.2)), src.a);
}
