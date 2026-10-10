//! Spawning a Telltale scene into the Bevy world.
//!
//! Each scene gets its own render layer and is drawn by its own camera (the scene's camera
//! agent), the way the engine composites scenes: the menu's 3D key art first, UI scenes over it.

use crate::convert;
use crate::data::{AssetCache, GameData, Props};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::ClearColorConfig;
use bevy::pbr::Lightmap;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use ttmeta::props::Value;
use ttmeta::mesh::Blend;
use ttmeta::{PropertySet, Scene};

/// How a scene's surfaces are shaded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shading {
    /// 3D world: diffuse x baked lightmap, alpha-tested.
    World,
    /// Flat UI: unlit, alpha-blended, ordered by "Render Layer".
    Ui,
}

#[derive(Component)]
pub struct TtAgent {
    pub name: String,
    pub props: Arc<PropertySet>,
}

/// Agents of one spawned scene, by name.
#[derive(Default)]
pub struct Spawned {
    pub agents: HashMap<String, Entity>,
    pub camera: Option<Entity>,
}

pub struct Ctx<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub data: &'a GameData,
    pub props: &'a mut Props,
    pub cache: &'a mut AssetCache,
    pub images: &'a mut Assets<Image>,
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub lit_materials: &'a mut Assets<crate::lit::LitMaterial>,
    pub bindposes: &'a mut Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
}

/// A camera agent's "Field of View": the horizontal angle of a 4:3 frame. The vertical angle
/// is therefore fixed (wider screens see more at the sides, not less at the top and bottom).
///
/// Evidence: the menu legend is created at y = -4.2 under the UI camera (z = 41, 20 degrees);
/// read as horizontal at 16:9 the visible half-height is 4.07 and the legend would be off-screen,
/// while the 4:3 reading gives 5.42. The game also ships separate widescreen ("aspectRatio")
/// chores, consistent with 4:3-authored framing.
#[derive(Component)]
pub struct HorizontalFov(pub f32);

pub fn vertical_fov(horizontal: f32) -> f32 {
    2.0 * ((horizontal * 0.5).tan() * 3.0 / 4.0).atan()
}

pub fn apply_fov(mut cams: Query<(&HorizontalFov, &mut Projection)>) {
    for (h, mut proj) in &mut cams {
        if let Projection::Perspective(p) = proj.as_mut() {
            let v = vertical_fov(h.0);
            if (p.fov - v).abs() > 1e-5 {
                p.fov = v;
            }
        }
    }
}

/// A light agent, reduced to what surfaces without baked lighting need.
///
/// Unbaked surfaces (sky dome, clouds, some props) are lit only by the lights of their lighting
/// group (the surface's "Render Lighting Groups" maps its texture to a group; a light lists its
/// groups in "Light Groups"). Approximation of the engine's model, which is not decoded yet:
/// ambient lights (type 2, e.g. "light_env_cloudsAmb", which has no distance range) add a flat
/// color x intensity; directional lights (type 1, the default; e.g. the sun-like Key light)
/// add color x intensity x N.L along their forward axis; other lights add color x intensity x
/// wrapped N.L x a linear fade between their min and max distance. The sum is clamped to 1 so surfaces never blow out.
#[derive(Clone, Copy, PartialEq)]
/// "Light Type", per the engine's names for its LightType values (in the exe, beside
/// `mLightType`): 0 Directional, 1 Point Light (module_light.prop's default), 2 Local Ambient
/// Light, 3 Directional shadows, 4 Point shadows, 5 Gobo Shadows, 6 Gobo Color.
enum LightKind {
    /// 2: a uniform term.
    Ambient,
    /// 0: a direction, no distance falloff.
    Directional,
    /// 1: a position with min/max distance falloff.
    Point,
    /// 3 "Directional shadows": a directional light that also casts shadows (lit like
    /// Directional; no shadow maps here yet).
    DirectionalShadows,
    /// 4 and up: point-shadow and gobo lights (left out).
    Shadow,
}

pub struct SceneLight {
    kind: LightKind,
    /// Light's forward axis in world space (directional lights).
    direction: Vec3,
    position: Vec3,
    color: Vec3,
    min: f32,
    max: f32,
    /// "Light Wrap Around": light reaching past the terminator.
    wrap: f32,
    name: String,
    groups: Vec<ttmeta::Symbol>,
    /// "Light Intensity", kept apart from `color` so chores can re-key the color.
    intensity: f32,
    /// The lit materials (and their light slot) this light was packed into, so chores that
    /// key the light can update them (see [`LightAgent`]).
    bindings: std::sync::Arc<std::sync::Mutex<Vec<(Handle<crate::lit::LitMaterial>, usize)>>>,
}

/// A light agent and the lit material slots it feeds. Chores key its "Light Color" and
/// "Light Max Distance" (each diorama episode recolors the sky); [`sync_lights`] writes the
/// changes into the materials.
#[derive(Component)]
pub struct LightAgent {
    pub color: Vec3,
    pub intensity: f32,
    pub max: f32,
    ambient: bool,
    bindings: Vec<(Handle<crate::lit::LitMaterial>, usize)>,
}

/// Push changed light colors and ranges into the lit materials they feed. Only real changes
/// are written (each write re-uploads every material the light feeds, thousands for the key
/// art's lights). Positions stay where the scene put them: lights that move with the key art's
/// montage would otherwise rewrite those materials every frame.
pub fn sync_lights(
    lights: Query<(Entity, &LightAgent), Changed<LightAgent>>,
    mut materials: ResMut<Assets<crate::lit::LitMaterial>>,
    mut written: Local<HashMap<Entity, (Vec3, f32)>>,
) {
    for (e, light) in &lights {
        // Ambient at 2x, as when packed (dec016).
        let color = light.color * light.intensity * if light.ambient { 2.0 } else { 1.0 };
        let Some(last) = written.insert(e, (color, light.max)) else {
            // First sight: the materials were packed from these same values.
            continue;
        };
        if last == (color, light.max) {
            continue;
        }
        for (handle, slot) in &light.bindings {
            let Some(mut m) = materials.get_mut(handle) else { continue };
            let l = &mut m.extension.uniform.lights[*slot];
            l.color = color.extend(1.0);
            l.params.w = light.max.min(1.0e6);
        }
    }
}

/// World transforms of every agent, following "attached agent" links (attachment nodes,
/// i.e. bones, are ignored).
fn world_transforms(ctx: &mut Ctx, scene: &Scene) -> HashMap<String, Transform> {
    let mut local = HashMap::new();
    for a in &scene.agents {
        if let Some(Value::Location(l)) = ctx.props.get(ctx.data, &a.props, "Scene: Location Info") {
            local.insert(a.name.clone(), (convert::transform(&l.local), l.attached_agent));
        }
    }
    let mut out = HashMap::new();
    for name in local.keys() {
        let mut t = Transform::IDENTITY;
        let mut cur = name.clone();
        for _ in 0..16 {
            let Some((tf, parent)) = local.get(&cur) else { break };
            t = *tf * t;
            if parent.is_empty() {
                break;
            }
            cur = parent.clone();
        }
        out.insert(name.clone(), t);
    }
    out
}

fn scene_lights(ctx: &mut Ctx, scene: &Scene, world: &HashMap<String, Transform>) -> Vec<SceneLight> {
    let mut lights = Vec::new();
    for a in &scene.agents {
        if !ctx.props.inherits(ctx.data, &a.props, "module_light.prop") {
            continue;
        }
        let c = ctx.props.color(ctx.data, &a.props, "Light Color").unwrap_or([1.0; 4]);
        let i = ctx.props.float(ctx.data, &a.props, "Light Intensity").unwrap_or(1.0);
        let groups = match ctx.props.get(ctx.data, &a.props, "Light Groups") {
            Some(Value::Symbols(g)) => g,
            _ => Vec::new(),
        };
        let tf = world.get(&a.name).copied().unwrap_or_default();
        lights.push(SceneLight {
            kind: match ctx.props.int(ctx.data, &a.props, "Light Type") {
                Some(0) => LightKind::Directional,
                Some(1) | None => LightKind::Point,
                Some(2) => LightKind::Ambient,
                Some(3) => LightKind::DirectionalShadows,
                _ => LightKind::Shadow,
            },
            direction: (tf.rotation * Vec3::Z).normalize_or_zero(),
            position: tf.translation,
            color: Vec3::new(c[0], c[1], c[2]) * i,
            intensity: i,
            bindings: Default::default(),
            min: ctx.props.float(ctx.data, &a.props, "Light Min Distance").unwrap_or(0.0),
            max: ctx.props.float(ctx.data, &a.props, "Light Max Distance").unwrap_or(f32::MAX),
            wrap: ctx.props.float(ctx.data, &a.props, "Light Wrap Around").unwrap_or(0.0).max(0.0),
            name: a.name.clone(),
            groups,
        });
    }
    lights
}

pub fn spawn_scene(ctx: &mut Ctx, scene: &Scene, layer: usize, order: isize, shading: Shading, skip: impl Fn(&str) -> bool) -> Spawned {
    spawn_scene_with_camera(ctx, scene, layer, order, shading, skip, None)
}

/// Like [`spawn_scene`], rendering through the camera agent named `camera` (default: the first).
pub fn spawn_scene_with_camera(
    ctx: &mut Ctx,
    scene: &Scene,
    layer: usize,
    order: isize,
    shading: Shading,
    skip: impl Fn(&str) -> bool,
    camera: Option<&str>,
) -> Spawned {
    let mut out = Spawned::default();
    let layers = RenderLayers::layer(layer);
    let mut attachments = Vec::new();
    let mut bones: HashMap<String, HashMap<ttmeta::Symbol, Entity>> = HashMap::new();
    let world = world_transforms(ctx, scene);
    let lights = if shading == Shading::World { scene_lights(ctx, scene, &world) } else { Vec::new() };
    // The scene's own FX (tone curve, vignette, fog), carried by its scene agent.
    let scene_fx = if shading == Shading::World {
        scene.agents.iter().find(|a| a.name == scene.name).map(|a| crate::grade::scene_effects(ctx.props, ctx.data, &a.props))
    } else {
        None
    };

    // MCSM_HIDE=a,b: leave out agents whose names contain any of these (debugging).
    let hide: Vec<String> = std::env::var("MCSM_HIDE").map(|v| v.split(',').map(|s| s.to_ascii_lowercase()).collect()).unwrap_or_default();
    for agent in &scene.agents {
        if agent.name == scene.name || skip(&agent.name) {
            continue;
        }
        if hide.iter().any(|h| !h.is_empty() && agent.name.to_ascii_lowercase().contains(h.as_str())) {
            continue;
        }
        let props = Arc::new(agent.props.clone());
        let location = match ctx.props.get(ctx.data, &props, "Scene: Location Info") {
            Some(Value::Location(l)) => l,
            _ => Default::default(),
        };
        let visible = ctx.props.bool(ctx.data, &props, "Scene: Start Visibility").unwrap_or(true);
        let mut tf = convert::transform(&location.local);
        if let Some(s) = ctx.props.vec3(ctx.data, &props, "Render Axis Scale") {
            tf.scale = Vec3::new(s[0], s[1], s[2]);
        }
        if let Some(g) = ctx.props.float(ctx.data, &props, "Render Global Scale").filter(|g| *g > 0.0) {
            tf.scale *= g;
        }
        // UI is ordered by "Render Layer", not depth: flatten agents (cameras keep their distance).
        if shading == Shading::Ui && !ctx.props.inherits(ctx.data, &props, "module_camera.prop") {
            tf.translation.z = 0.0;
        }
        let entity = ctx
            .commands
            .spawn((
                Name::new(agent.name.clone()),
                tf,
                if visible { Visibility::Inherited } else { Visibility::Hidden },
                layers.clone(),
                TtAgent { name: agent.name.clone(), props: props.clone() },
                crate::chore::BaseTransform(tf),
            ))
            .id();
        out.agents.insert(agent.name.clone(), entity);
        if !location.attached_agent.is_empty() {
            attachments.push((entity, location.attached_agent.clone(), location.attached_node));
        }

        if out.camera.is_none() && camera.is_none_or(|c| c == agent.name) && ctx.props.inherits(ctx.data, &props, "module_camera.prop") {
            let fov = ctx.props.float(ctx.data, &props, "Field of View").unwrap_or(60.0);
            let near = ctx.props.float(ctx.data, &props, "Clip Plane - Near").unwrap_or(0.25);
            let far = ctx.props.float(ctx.data, &props, "Clip Plane - Far").unwrap_or(600.0);
            // The camera looks down the agent's local +Z; Bevy cameras look down -Z.
            let lens = ctx
                .commands
                .spawn((Transform::from_rotation(convert::CAMERA_FIX), layers.clone(), ChildOf(entity)))
                .id();
            ctx.commands.entity(lens).insert((
                Camera3d::default(),
                Camera {
                    order,
                    clear_color: if order == 0 { ClearColorConfig::Custom(Color::BLACK) } else { ClearColorConfig::None },
                    ..default()
                },
                Projection::Perspective(PerspectiveProjection { fov: fov.to_radians(), near, far, ..default() }),
                HorizontalFov(fov.to_radians()),
                Msaa::Off,
                // Baked lighting and UI colors are plain 0..1 multipliers: no exposure, no tonemapping.
                bevy::camera::Exposure { ev100: (1.0f32 / 1.2).log2() },
                bevy::core_pipeline::tonemapping::Tonemapping::None,
            ));
            if let Some((grade, fog)) = &scene_fx {
                if let Some(g) = grade {
                    ctx.commands.entity(lens).insert(*g);
                }
                if let Some(f) = fog {
                    ctx.commands.entity(lens).insert(f.clone());
                }
            }
            out.camera = Some(lens);
            continue;
        }

        let agent_world = world.get(&agent.name).copied().unwrap_or_default();
        let mut owned = crate::chore::AgentMaterials::default();
        let character_bones = crate::character::spawn(ctx, entity, &agent.name, &props, |ctx, container, mesh, skin| {
            spawn_mesh(ctx, container, &props, mesh, &layers, shading, &lights, agent_world, Some(skin), &mut owned)
        });
        let is_character = character_bones.is_some();
        if let Some(b) = character_bones {
            bones.insert(agent.name.clone(), b);
        }
        if !is_character {
            if let Some(list) = match ctx.props.get(ctx.data, &props, "D3D Mesh List") {
                // Every renderable inherits an empty list (module_renderable.prop).
                Some(Value::Symbols(l)) if !l.is_empty() => Some(l),
                _ => None,
            } {
                // Multi-mesh props (e.g. block-building structures): every mesh, individually toggleable.
                for (container, mesh) in crate::character::mesh_list(ctx, entity, &props, &list) {
                    spawn_mesh(ctx, container, &props, mesh, &layers, shading, &lights, agent_world, None, &mut owned);
                }
            } else if let Some(mesh) = ctx.props.symbol(ctx.data, &props, "D3D Mesh") {
                spawn_mesh(ctx, entity, &props, mesh, &layers, shading, &lights, agent_world, None, &mut owned);
            }
        }
        ctx.commands.entity(entity).insert(owned);
    }

    for (child, parent, node) in attachments {
        // Attached to a bone of a character (held items): parent to that bone.
        let bone = bones.get(&parent).and_then(|b| b.get(&node)).copied();
        if let Some(p) = bone.or_else(|| out.agents.get(&parent).copied()) {
            ctx.commands.entity(child).insert(ChildOf(p));
        }
    }
    // Light agents keep the material slots they feed, for chores that re-key them.
    for l in &lights {
        let Some(&e) = out.agents.get(&l.name) else { continue };
        let bindings = std::mem::take(&mut *l.bindings.lock().unwrap());
        if bindings.is_empty() {
            continue;
        }
        let c = l.color / l.intensity.max(1.0e-6);
        ctx.commands.entity(e).insert(LightAgent {
            color: if l.intensity > 0.0 { c } else { Vec3::ZERO },
            intensity: l.intensity,
            max: l.max,
            ambient: matches!(l.kind, LightKind::Ambient),
            bindings,
        });
    }
    // The scene agent itself, so chores can key its properties (each diorama episode sets
    // its own "Fog Color").
    if let Some(camera) = out.camera {
        let e = ctx.commands.spawn((Name::new(scene.name.clone()), SceneAgent { camera })).id();
        out.agents.insert(scene.name.clone(), e);
    }
    out
}

/// How far animation can carry a skinned body from its unposed mesh (the diorama's leaping
/// Jesse is placed 2.6 units below the camera's view and lifted into it by the jump).
const SKINNED_REACH: f32 = 5.0;

/// Culling uses the unposed mesh's bounds; grow skinned meshes' bounds by [`SKINNED_REACH`]
/// whenever the engine (re)computes them, so animated bodies are not culled while in view.
pub fn grow_skinned_bounds(
    mut bounds: Query<(&mut bevy::camera::primitives::Aabb, Option<&GrownBounds>, Entity), (With<bevy::mesh::skinning::SkinnedMesh>, Changed<bevy::camera::primitives::Aabb>)>,
    mut commands: Commands,
) {
    for (mut aabb, grown, e) in &mut bounds {
        if grown.is_some_and(|g| g.0 == aabb.half_extents) {
            continue;
        }
        aabb.half_extents += bevy::math::Vec3A::splat(SKINNED_REACH);
        commands.entity(e).insert(GrownBounds(aabb.half_extents));
    }
}

/// The half extents [`grow_skinned_bounds`] last set.
#[derive(Component)]
pub struct GrownBounds(bevy::math::Vec3A);

/// A scene's own agent (named after the scene file): its camera, whose fog and grade belong
/// to the scene.
#[derive(Component)]
pub struct SceneAgent {
    pub camera: Entity,
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_mesh(
    ctx: &mut Ctx,
    entity: Entity,
    props: &PropertySet,
    mesh: ttmeta::Symbol,
    layers: &RenderLayers,
    shading: Shading,
    lights: &[SceneLight],
    agent_world: Transform,
    skin: Option<&crate::character::Skin>,
    owned: &mut crate::chore::AgentMaterials,
) {
    let Some(loaded) = ctx.cache.mesh(ctx.data, ctx.meshes, mesh) else {
        warn!("mesh {:?} ({}) did not load", mesh, ctx.data.name_of(mesh).unwrap_or("?"));
        return;
    };
    let diffuse_color = ctx.props.color(ctx.data, props, "Render Diffuse Color").unwrap_or([1.0; 4]);
    // A negative constant alpha means "not set": use the texture's alpha as is.
    let constant_alpha = ctx.props.float(ctx.data, props, "Render Constant Alpha").filter(|a| *a >= 0.0).unwrap_or(1.0);
    let alpha_multiply = ctx.props.float(ctx.data, props, "Render Constant Alpha Multiply").unwrap_or(1.0);
    let alpha = constant_alpha * alpha_multiply;
    let render_layer = ctx.props.int(ctx.data, props, "Render Layer").unwrap_or(0);
    // "Fog Override settings" with "Fog Mesh Enabled" off: excluded from the scene's fog (the
    // sky dome and clouds).
    let fogged = !(ctx.props.bool(ctx.data, props, "Fog Override settings") == Some(true)
        && ctx.props.bool(ctx.data, props, "Fog Mesh Enabled") != Some(true));
    let lighting_groups = match ctx.props.get(ctx.data, props, "Render Lighting Groups") {
        Some(Value::SymbolMap(m)) => m,
        _ => Vec::new(),
    };
    for (part_index, part) in loaded.parts.iter().enumerate() {
        // Glass opacity applies to 3D surfaces (UI meshes keep other data at that offset).
        let opacity = if shading == Shading::World { part.opacity } else { None };
        let pixel_art = shading == Shading::World;
        // "<texture> - Texture" swaps a texture on the agent's meshes (outfits, e.g. Magnus's
        // armor; the episode menu's appearanceSwap uses "skM1_jesse - Texture").
        let diffuse = part.diffuse.map(|d| {
            let swap = ctx.data.name_of(d).map(|n| format!("{} - Texture", n.strip_suffix(".d3dtx").unwrap_or(n)));
            swap.and_then(|key| ctx.props.symbol(ctx.data, props, &key)).filter(|s| *s != ttmeta::Symbol::NONE).unwrap_or(d)
        });
        let tex = diffuse.and_then(|s| ctx.cache.image(ctx.data, ctx.images, s, pixel_art));
        let bake = part.bake.and_then(|s| ctx.cache.image(ctx.data, ctx.images, s, false));
        let c = [
            diffuse_color[0] * part.tint[0],
            diffuse_color[1] * part.tint[1],
            diffuse_color[2] * part.tint[2],
            diffuse_color[3] * part.tint[3] * alpha * opacity.unwrap_or(1.0),
        ];
        let material = match shading {
            Shading::Ui => StandardMaterial {
                base_color: Color::srgba(c[0], c[1], c[2], c[3]),
                base_color_texture: tex,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            },
            // "Render Constant Alpha" below 1 makes a world surface translucent (e.g. the
            // character-select floor shadow at 0.2).
            Shading::World => StandardMaterial {
                // Constant alpha below 1 or a glass material's opacity make it translucent.
                base_color: Color::srgba(c[0], c[1], c[2], if alpha < 0.999 || opacity.is_some() { c[3] } else { 1.0 }),
                base_color_texture: tex,
                unlit: bake.is_none(),
                alpha_mode: match part.blend {
                    _ if alpha < 0.999 => AlphaMode::Blend,
                    Blend::Opaque => AlphaMode::Opaque,
                    Blend::Alpha => AlphaMode::Blend,
                    Blend::AlphaTest | Blend::Other(_) => AlphaMode::Mask(0.5),
                    Blend::Add => AlphaMode::Add,
                },
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                fog_enabled: fogged,
                cull_mode: None,
                ..default()
            },
        };
        // UI quads sit on one plane facing the UI camera (which is on +Z); nudge each toward the
        // camera by its render layer so blending order follows the engine's layer order.
        // Later submeshes draw over earlier ones (the engine draws them in order).
        let nudge = if shading == Shading::Ui { Vec3::new(0.0, 0.0, 0.001 * render_layer as f32 + 0.0001 * part_index as f32) } else { Vec3::ZERO };
        // Unbaked world surfaces lit by their lighting group: per-vertex light color.
        let mesh_handle = part.mesh.clone();
        // Unbaked world surfaces are lit per pixel by their lighting group (lit.rs).
        let mut lit: Option<crate::lit::TtLightingUniform> = None;
        let mut slots: Vec<(&SceneLight, usize)> = Vec::new();
        if shading == Shading::World && bake.is_none() {
            // "Render Lighting Groups" names textures as the agent shows them, i.e. after a
            // "<texture> - Texture" swap (the diorama's skydome); fall back to the mesh's own.
            // A map naming one group whose texture keys are stale (the skydome names a texture
            // its mesh no longer uses) still puts the whole agent in that group.
            let only_group = lighting_groups.first().map(|(_, g)| *g).filter(|g| lighting_groups.iter().all(|(_, x)| x == g));
            let group = [diffuse, part.diffuse]
                .into_iter()
                .flatten()
                .find_map(|d| lighting_groups.iter().find(|(t, _)| *t == d).map(|(_, g)| *g))
                .or(only_group);
            let group_lights: Vec<&SceneLight> = group.map_or(Vec::new(), |g| lights.iter().filter(|l| l.groups.contains(&g)).collect());
            // An additive surface whose group has no lights receives no light and so adds
            // nothing (the night-stars layer in a daytime scene).
            if part.blend == Blend::Add && group.is_some() && group_lights.is_empty() {
                continue;
            }
            if let Ok(filter) = std::env::var("MCSM_LIGHT_LOG") {
                let name = ctx.data.name_of(mesh).unwrap_or("?");
                if name.to_ascii_lowercase().contains(&filter.to_ascii_lowercase()) {
                    info!(
                        "{name}: diffuse {:?} / {:?}, groups {:?}, group {:?}, at {:?}, lights {:?}",
                        part.diffuse.and_then(|d| ctx.data.name_of(d)),
                        diffuse.and_then(|d| ctx.data.name_of(d)),
                        lighting_groups.iter().map(|(t, _)| ctx.data.name_of(*t).unwrap_or("?")).collect::<Vec<_>>(),
                        group,
                        agent_world.translation,
                        group_lights.iter().map(|l| (l.name.clone(), l.position, l.min, l.max, l.color)).collect::<Vec<_>>()
                    );
                }
            }
            let debug_skip = std::env::var("MCSM_LIT_SKIP").ok().is_some_and(|f| ctx.data.name_of(mesh).unwrap_or("").contains(&f));
            if !group_lights.is_empty() && !debug_skip {
                let mut u = crate::lit::TtLightingUniform { translucent: (part.blend == Blend::Alpha) as u32, ..default() };
                for l in group_lights.iter().filter(|l| !matches!(l.kind, LightKind::Shadow)).take(crate::lit::MAX_LIGHTS) {
                    let (kind, vector, color) = match l.kind {
                        // Ambient at 2x (decisions.tsv dec016): the key art's clouds, lit only by a
                        // 0.5 ambient, are white in the game.
                        LightKind::Ambient => (0.0, Vec3::ZERO, l.color * 2.0),
                        LightKind::Directional | LightKind::DirectionalShadows => (1.0, l.direction, l.color),
                        LightKind::Point | LightKind::Shadow => (2.0, l.position, l.color),
                    };
                    u.lights[u.count as usize] = crate::lit::TtLight {
                        color: color.extend(1.0),
                        vector: vector.extend(0.0),
                        params: Vec4::new(kind, l.wrap, l.min, l.max.min(1.0e6)),
                    };
                    slots.push((*l, u.count as usize));
                    u.count += 1;
                }
                lit = Some(u);
            }
        }
        let base_alpha = diffuse_color[3] * part.tint[3] * alpha_multiply * opacity.unwrap_or(1.0);
        let material_entity = if let Some(uniform) = lit {
            let mut base = material;
            base.unlit = false;
            let handle = ctx.lit_materials.add(crate::lit::LitMaterial { base, extension: crate::lit::TtLighting { uniform } });
            for (light, slot) in &slots {
                light.bindings.lock().unwrap().push((handle.clone(), *slot));
            }
            owned.lit.push((handle.clone(), base_alpha, part.diffuse));
            ctx.commands.spawn(MeshMaterial3d(handle)).id()
        } else {
            let material = ctx.materials.add(material);
            owned.meshes.push((material.clone(), base_alpha));
            if let Some(d) = part.diffuse {
                owned.diffuse.push((material.clone(), d));
            }
            ctx.commands.spawn(MeshMaterial3d(material)).id()
        };
        let mut part_entity = ctx.commands.entity(material_entity);
        part_entity.insert((
            Mesh3d(mesh_handle),
            Transform::from_translation(nudge),
            layers.clone(),
            ChildOf(entity),
        ));
        if let Some(image) = bake {
            part_entity.insert(Lightmap { image, uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0), bicubic_sampling: false });
        }
        if let (Some(skin), Some(palette)) = (skin, &part.palette) {
            let joints = palette.iter().map(|b| skin.joints.get(b).copied().unwrap_or(skin.root)).collect();
            let inverse: Vec<Mat4> = palette.iter().map(|b| skin.bind.get(b).map_or(Mat4::IDENTITY, |m| m.inverse())).collect();
            part_entity.insert(bevy::mesh::skinning::SkinnedMesh { inverse_bindposes: ctx.bindposes.add(inverse), joints });
        }
    }
}
