// Telltale-style lighting for unbaked world surfaces (characters, props without lightmaps).
//
// Model, as read from the game's own compiled shaders (MCSM_pc_Shaders: per-pixel key light
// "N.L * a + b" with a = 1 / (1 + wrap), b = wrap / (1 + wrap), negative results dropped, plus
// ambient; the sum multiplies the diffuse texture and material colour and is only clamped at 0):
//   colour = diffuse * (sum of lights)
// Lights: kind 0 ambient, 1 directional (`vector`: the light's forward axis), 2 point (at `vector`,
// linear falloff from min to max distance).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}

struct TtLight {
    color: vec4<f32>,
    vector: vec4<f32>,
    // x: kind, y: wrap around, z: min distance, w: max distance
    params: vec4<f32>,
}

struct TtLighting {
    lights: array<TtLight, 8>,
    count: u32,
    // 1: translucent surfaces (glass) are lit from either side
    translucent: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> tt: TtLighting;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var n = normalize(in.world_normal);
    if !is_front {
        n = -n;
    }
    let p = in.world_position.xyz;
    var light = vec3<f32>(0.0);
    for (var i = 0u; i < tt.count; i = i + 1u) {
        let l = tt.lights[i];
        let kind = u32(l.params.x);
        let wrap = l.params.y;
        if kind == 0u {
            light += l.color.rgb;
            continue;
        }
        var dir: vec3<f32>;
        var atten = 1.0;
        if kind == 1u {
            // The light's forward axis points at the light (from the lit surface).
            dir = normalize(l.vector.xyz);
        } else {
            let to = l.vector.xyz - p;
            let d = length(to);
            dir = to / max(d, 1e-4);
            if d >= l.params.w {
                atten = 0.0;
            } else if d > l.params.z {
                atten = 1.0 - (d - l.params.z) / max(l.params.w - l.params.z, 1e-4);
            }
        }
        var ndl = dot(n, dir);
        if tt.translucent != 0u {
            ndl = abs(ndl);
        }
        let lambert = max((ndl + wrap) / (1.0 + wrap), 0.0);
        light += l.color.rgb * lambert * atten;
    }

    var out: FragmentOutput;
    let base = pbr_input.material.base_color;
    out.color = vec4<f32>(max(base.rgb * light, vec3<f32>(0.0)), base.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
