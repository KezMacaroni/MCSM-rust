//! Chore playback: drives scene agents from a chore's timeline.
//!
//! Supported per resource:
//! - property tracks keyed in the chore's control animation: `absoluteNode` (the agent's
//!   transform), `Runtime: Visible`, `Render Global Scale`, `Render Constant Alpha`,
//!   `Text Color`, the UI texture-state offset ([`UV_STATE`]), a scene agent's `Fog Color`,
//!   and a light's `Light Color` / `Light Max Distance`;
//! - `.anm` resources whose Transform tracks are additive (camera drift), placed on the timeline
//!   by their blocks and the control tracks `time` (normalized animation time) and
//!   `contribution` (weight);
//! - nested `.chore` resources, played inside their blocks.
//!
//! - skeletal `.anm` resources on character agents, handed to `character::animate`.
//!
//! Not yet: depth of field, sound, procedural objects.

use crate::convert;
use crate::data::GameData;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use ttmeta::anim::{bracket, sample_bool, sample_color, sample_float, Key, Track};
use ttmeta::chore::Chore;
use ttmeta::props::Transform as TtTransform;
use ttmeta::{Animation, Symbol};

/// An agent's transform before any chore touched it.
#[derive(Component, Clone, Copy)]
pub struct BaseTransform(pub Transform);

/// UI chores key this float to pick a row of a button's texture sheet (0, 0.25, 0.5, 0.75:
/// normal, selected, locked, locked-selected). Its name is not recovered; the meaning is
/// evident from the texture and the select/deselect chores (0.25 / 0).
pub const UV_STATE: Symbol = Symbol(0x84e6_4379_8f5f_cdda);

/// Other widgets' texture-sheet state floats: popup buttons (vertical, 0 / 0.25, like list
/// buttons) and the slider handle (horizontal, 0 / 0.5: its sheet has two columns).
const UV_STATES: [Symbol; 2] = [UV_STATE, Symbol(0xbb8d_d7e2_904d_b435)];
const UV_STATE_U: Symbol = Symbol(0x402e_f46e_98bf_e6bd);

/// Materials owned by an agent, so property tracks can drive them.
#[derive(Component, Default)]
pub struct AgentMaterials {
    /// Mesh materials and their alpha before "Render Constant Alpha" is applied.
    pub meshes: Vec<(Handle<StandardMaterial>, f32)>,
    /// Text materials (main pass only, not shadows).
    pub text: Vec<Handle<StandardMaterial>>,
    /// The last "Text Color" a chore set, for text that is rebuilt later (changed labels).
    pub text_color: Option<[f32; 4]>,
    /// Mesh materials and the diffuse texture their mesh names, for runtime texture swaps
    /// ("<texture> - Texture" set while the game runs, e.g. Jesse's appearance).
    pub diffuse: Vec<(Handle<StandardMaterial>, ttmeta::Symbol)>,
    /// Group-lit materials (lit.rs): handle, alpha before "Render Constant Alpha", diffuse.
    pub lit: Vec<(Handle<crate::lit::LitMaterial>, f32, Option<ttmeta::Symbol>)>,
}

#[derive(Component)]
pub struct ChorePlayer {
    pub chore: Arc<Chore>,
    pub time: f32,
    pub looping: bool,
    /// Playback rate; 0 holds the current time (the game pauses some chores at a computed time).
    pub speed: f32,
    /// Agent name (lowercase) -> entity, for the scene the chore drives. Chores do not always
    /// match the scene's capitalization of agent names.
    pub agents: Arc<HashMap<String, Entity>>,
    /// Loaded `.anm` / nested `.chore` resources, by resource index.
    anims: HashMap<usize, Arc<Animation>>,
    nested: HashMap<usize, Arc<Chore>>,
    /// Idles were handed to the agents when the chore finished.
    handed_off: bool,
    /// Each agent's last non-empty skeletal layers (an animation holds its last frame once
    /// its block ends; agents without an idle keep that pose after the chore).
    last_skeletal: HashMap<String, Vec<(Arc<Animation>, f32, f32, bool)>>,
}

/// Parsed animations and chores, shared by every player (reading and decoding them from the
/// archives again for each chore that plays them made menus hitch).
static ANIMATIONS: std::sync::Mutex<Option<HashMap<Symbol, Option<Arc<Animation>>>>> = std::sync::Mutex::new(None);
static CHORES: std::sync::Mutex<Option<HashMap<Symbol, Option<Arc<Chore>>>>> = std::sync::Mutex::new(None);

pub fn load_animation(data: &GameData, s: Symbol) -> Option<Arc<Animation>> {
    let mut guard = ANIMATIONS.lock().unwrap();
    guard
        .get_or_insert_with(HashMap::new)
        .entry(s)
        .or_insert_with(|| data.read_symbol(s).and_then(|b| Animation::parse(&b).ok()).map(Arc::new))
        .clone()
}

fn load_chore(data: &GameData, s: Symbol) -> Option<Arc<Chore>> {
    let mut guard = CHORES.lock().unwrap();
    guard
        .get_or_insert_with(HashMap::new)
        .entry(s)
        .or_insert_with(|| {
            let bytes = data.read_symbol(s)?;
            Chore::parse(&bytes).map_err(|e| warn!("{}: {e}", data.name_of(s).unwrap_or("?"))).ok().map(Arc::new)
        })
        .clone()
}

/// A chore by symbol, parsed once.
pub fn chore_by_symbol(data: &GameData, s: Symbol) -> Option<Arc<Chore>> {
    load_chore(data, s)
}

/// A chore by file name, parsed once.
pub fn chore_by_name(data: &GameData, name: &str) -> Option<Arc<Chore>> {
    load_chore(data, Symbol::of(name))
}

impl ChorePlayer {
    pub fn new(data: &GameData, chore: impl Into<Arc<Chore>>, agents: Arc<HashMap<String, Entity>>, looping: bool) -> Self {
        let chore: Arc<Chore> = chore.into();
        let mut anims = HashMap::new();
        let mut nested = HashMap::new();
        for (i, res) in chore.resources.iter().enumerate() {
            let name = data.name_of(res.name).unwrap_or("").to_ascii_lowercase();
            if let Some(c) = &res.embedded_chore {
                nested.insert(i, Arc::new((**c).clone()));
            } else if name.ends_with(".anm") {
                if let Some(a) = load_animation(data, res.name) {
                    anims.insert(i, a);
                }
            } else if name.ends_with(".chore") {
                if let Some(c) = load_chore(data, res.name) {
                    nested.insert(i, c);
                }
            } else if let Some(a) = &res.embedded {
                anims.insert(i, Arc::new(a.clone()));
            }
        }
        let agents = Arc::new(agents.iter().map(|(k, v)| (k.to_ascii_lowercase(), *v)).collect());
        ChorePlayer { chore, time: 0.0, looping, speed: 1.0, agents, anims, nested, handed_off: false, last_skeletal: HashMap::new() }
    }
}

impl ChorePlayer {
    /// Skeletal animation layers of every agent in the chore at `t`, for idle chores (whose
    /// agent names are generic and stand for whichever character plays them).
    pub fn skeletal_layers(&self, data: &GameData, t: f32) -> Vec<(Arc<Animation>, f32, f32, bool)> {
        let mut poses = HashMap::new();
        evaluate(&self.chore, &self.anims, &self.nested, t, &mut poses, data, 0, None);
        poses.into_values().flat_map(|p| p.skeletal).collect()
    }
}

/// Per-frame result for one agent.
#[derive(Default)]
struct Pose {
    absolute: Option<Transform>,
    additive: Vec<(Transform, f32)>,
    visible: Option<bool>,
    scale: Option<f32>,
    constant_alpha: Option<f32>,
    text_color: Option<[f32; 4]>,
    /// "Fog Color" keyed on a scene agent.
    fog_color: Option<[f32; 4]>,
    /// "Light Color" / "Light Max Distance" keyed on a light agent.
    light_color: Option<[f32; 4]>,
    light_max: Option<f32>,
    field_of_view: Option<f32>,
    uv_state: Option<f32>,
    uv_state_u: Option<f32>,
    skeletal: Vec<(Arc<Animation>, f32, f32, bool)>,
    /// The last full-body idle loop (`*_idle_*.anm`) playing on the agent, which it keeps
    /// looping once the chore is over.
    idle: Option<Arc<Animation>>,
    /// `Mesh <name> - Visible` (and other bool) tracks: per-mesh switches.
    toggles: Vec<(Symbol, bool)>,
    /// "<texture> - Texture" tracks: texture swaps (key symbol, new texture).
    textures: Vec<(Symbol, Symbol)>,
    /// Agent whose space `absolute` is in (block-building chores), instead of the scene's.
    frame: Option<String>,
}

const RUNTIME_VISIBLE: Symbol = Symbol(0x9c3c_5c9d_cb9e_790e);

/// In a block-building chore (`<blocks agent>_buildOrder.chore`, generated by the game's
/// BlockBuilding.lua), the builders are attached to the blocks agent for the whole chore, so
/// their `absoluteNode` keys are positions in that agent's space.
fn build_frame(chore: &Chore) -> Option<String> {
    let lower = chore.name.to_ascii_lowercase();
    let stem = lower.strip_suffix("_buildorder.chore")?;
    chore
        .agents
        .iter()
        .find(|a| a.name.to_ascii_lowercase() == stem)
        .or_else(|| chore.agents.iter().find(|a| a.name.to_ascii_lowercase().ends_with("blocks")))
        .map(|a| a.name.clone())
}

fn sample_transform(keys: &[Key<TtTransform>], t: f32) -> Option<Transform> {
    let (a, b, f) = bracket(keys, t)?;
    let ta = convert::transform(&a.value);
    if !a.interpolate || f <= 0.0 {
        return Some(ta);
    }
    let tb = convert::transform(&b.value);
    // Tangent modes: 1 stepped, 2 knot (linear), 3 smooth, 4 flat (ease in/out).
    let f = match a.tangent {
        1 => 0.0,
        3 | 4 => f * f * (3.0 - 2.0 * f),
        _ => f,
    };
    Some(Transform {
        translation: ta.translation.lerp(tb.translation, f),
        rotation: ta.rotation.slerp(tb.rotation, f),
        scale: ta.scale.lerp(tb.scale, f),
    })
}

fn track<'a>(a: &'a Animation, name: &str) -> Option<&'a Track> {
    a.value(name)
}

fn evaluate(
    chore: &Chore,
    anims: &HashMap<usize, Arc<Animation>>,
    nested: &HashMap<usize, Arc<Chore>>,
    t: f32,
    out: &mut HashMap<String, Pose>,
    data: &GameData,
    depth: u32,
    owner: Option<&str>,
) {
    let frame = build_frame(chore);
    for agent in &chore.agents {
        // Reusable action chores name their performer "default": that is the agent playing them.
        let name = match owner {
            Some(o) if agent.name.eq_ignore_ascii_case("default") => o.to_owned(),
            _ => agent.name.clone(),
        };
        for &ri in &agent.resources {
            let Some(res) = chore.resources.get(ri) else { continue };
            if !res.enabled {
                continue;
            }
            let ctrl = &res.control;
            let pose = out.entry(name.clone()).or_default();
            if let Some(Track::Transform(k)) = track(ctrl, "absoluteNode") {
                if let Some(x) = sample_transform(k, t) {
                    pose.absolute = Some(x);
                    pose.frame = frame.clone().filter(|f| *f != agent.name);
                }
            }
            for v in &ctrl.values {
                if let Track::Texture(k) = &v.track {
                    if let Some((a, _, _)) = bracket(k, t) {
                        pose.textures.push((v.name, a.value));
                    }
                }
                if let Track::Bool(k) = &v.track {
                    if v.name != RUNTIME_VISIBLE {
                        if let Some(b) = sample_bool(k, t) {
                            pose.toggles.push((v.name, b));
                        }
                    }
                }
            }
            if let Some(Track::Bool(k)) = track(ctrl, "Runtime: Visible") {
                if let Some(v) = sample_bool(k, t) {
                    pose.visible = Some(v);
                }
            }
            if let Some(Track::Float(k)) = track(ctrl, "Render Global Scale") {
                if let Some(v) = sample_float(k, t) {
                    pose.scale = Some(v);
                }
            }
            if let Some(Track::Float(k)) = track(ctrl, "Render Constant Alpha") {
                pose.constant_alpha = sample_float(k, t).or(pose.constant_alpha);
            }
            if let Some(Track::Color(k)) = track(ctrl, "Text Color") {
                pose.text_color = sample_color(k, t).or(pose.text_color);
            }
            if let Some(Track::Color(k)) = track(ctrl, "Fog Color") {
                pose.fog_color = sample_color(k, t).or(pose.fog_color);
            }
            if let Some(Track::Color(k)) = track(ctrl, "Light Color") {
                pose.light_color = sample_color(k, t).or(pose.light_color);
            }
            if let Some(Track::Float(k)) = track(ctrl, "Light Max Distance") {
                pose.light_max = sample_float(k, t).or(pose.light_max);
            }
            if let Some(Track::Float(k)) = track(ctrl, "Field of View") {
                pose.field_of_view = sample_float(k, t).or(pose.field_of_view);
            }
            if let Some(v) = ctrl.values.iter().find(|v| UV_STATES.contains(&v.name)) {
                if let Track::Float(k) = &v.track {
                    pose.uv_state = sample_float(k, t).or(pose.uv_state);
                }
            }
            if let Some(v) = ctrl.values.iter().find(|v| v.name == UV_STATE_U) {
                if let Track::Float(k) = &v.track {
                    pose.uv_state_u = sample_float(k, t).or(pose.uv_state_u);
                }
            }

            // Timeline-placed resources: active only inside one of their blocks.
            let Some(block) = res.blocks.iter().find(|b| t >= b.start && t <= b.end) else { continue };
            let local = (t - block.start) * block.scale.max(0.0001);
            let norm = match track(ctrl, "time") {
                Some(Track::Float(k)) => sample_float(k, t),
                _ => None,
            };
            let weight = match track(ctrl, "contribution") {
                Some(Track::Float(k)) if k.len() > 1 || k.first().is_some_and(|x| x.time >= 0.0) => sample_float(k, t).unwrap_or(1.0),
                _ => 1.0,
            };
            if let Some(anim) = anims.get(&ri) {
                let at = norm.map_or(local, |n| n * anim.length);
                if anim.skeleton().is_some() {
                    let name = data.name_of(res.name).unwrap_or("").to_ascii_lowercase();
                    let additive = name.ends_with("_add.anm");
                    pose.skeletal.push((anim.clone(), at, weight, additive));
                    if !additive && name.contains("_idle") {
                        pose.idle = Some(anim.clone());
                    }
                    // Their Transform tracks are root motion, not agent placement.
                    continue;
                }
                for v in &anim.values {
                    if let Track::Transform(k) = &v.track {
                        if let Some(x) = sample_transform(k, at) {
                            pose.additive.push((x, weight));
                        }
                    }
                }
            } else if let Some(sub) = nested.get(&ri) {
                if depth < 4 {
                    let at = norm.map_or(local, |n| n * sub.length);
                    // Nested chores reference their own resources; load them lazily by name.
                    let player = nested_cache(data, sub);
                    evaluate(sub, &player.0, &player.1, at, out, data, depth + 1, Some(&name));
                }
            }
        }
    }
}

type NestedRes = (HashMap<usize, Arc<Animation>>, HashMap<usize, Arc<Chore>>);

fn nested_cache(data: &GameData, chore: &Chore) -> Arc<NestedRes> {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<String, Arc<NestedRes>>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    map.entry(chore.name.clone())
        .or_insert_with(|| {
            let p = ChorePlayer::new(data, Arc::new(chore.clone()), Arc::new(HashMap::new()), false);
            Arc::new((p.anims, p.nested))
        })
        .clone()
}

/// A one-shot chore that is finished despawns its player entity.
#[derive(Component)]
pub struct DespawnWhenDone;

pub fn play(
    mut commands: Commands,
    time: Res<Time>,
    data: Option<Res<GameData>>,
    mut players: Query<(Entity, &mut ChorePlayer, Has<DespawnWhenDone>)>,
    mut agents: Query<(&BaseTransform, &mut Transform, &mut Visibility, Option<&mut AgentMaterials>, Option<&mut crate::character::SkeletalOverride>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    globals: Query<&GlobalTransform>,
    toggles: Query<&crate::character::MeshToggles>,
    mut cache: ResMut<crate::data::AssetCache>,
    mut images: ResMut<Assets<Image>>,
    mut lit_materials: ResMut<Assets<crate::lit::LitMaterial>>,
    scene_agents: Query<&crate::stage::SceneAgent>,
    mut fogs: Query<&mut bevy::pbr::DistanceFog>,
    mut lights: Query<&mut crate::stage::LightAgent>,
    mut cameras: Query<(&ChildOf, &mut crate::stage::HorizontalFov)>,
) {
    let Some(data) = data else { return };
    for (player_entity, mut player, one_shot) in &mut players {
        if one_shot && player.time >= player.chore.length {
            commands.entity(player_entity).despawn();
        }
        // Cap the step so a long frame (e.g. the one after loading) does not skip a transition.
        player.time += time.delta_secs().min(1.0 / 30.0) * player.speed;
        let len = player.chore.length.max(0.01);
        if player.time > len {
            player.time = if player.looping { player.time % len } else { len };
        }
        // A finished one-shot chore leaves its agents where it put them, but no longer animates
        // their skeletons: characters go back to their own idles (the held key-art shot).
        let finished = !player.looping && player.time >= len;
        let hand_off = finished && !player.handed_off;
        if hand_off {
            player.handed_off = true;
        }
        let mut poses: HashMap<String, Pose> = HashMap::new();
        evaluate(&player.chore, &player.anims, &player.nested, player.time, &mut poses, &data, 0, None);
        for (name, pose) in poses {
            let Some(&e) = player.agents.get(&name.to_ascii_lowercase()) else { continue };
            if let (Some(c), Ok(scene)) = (pose.fog_color, scene_agents.get(e)) {
                if let Ok(mut fog) = fogs.get_mut(scene.camera) {
                    fog.color = Color::srgba(c[0], c[1], c[2], 1.0);
                }
                continue;
            }
            if pose.light_color.is_some() || pose.light_max.is_some() {
                if let Ok(mut light) = lights.get_mut(e) {
                    if let Some(c) = pose.light_color {
                        let c = Vec3::new(c[0], c[1], c[2]);
                        if light.color != c {
                            light.color = c;
                        }
                    }
                    if let Some(m) = pose.light_max {
                        if light.max != m {
                            light.max = m;
                        }
                    }
                }
            }
            if let Some(fov) = pose.field_of_view.filter(|f| f.is_finite() && *f > 0.0 && *f < 180.0) {
                for (parent, mut angle) in &mut cameras {
                    if parent.parent() == e { angle.0 = fov.to_radians(); }
                }
            }
            let Ok((base, mut tf, mut vis, mats, skeletal)) = agents.get_mut(e) else { continue };
            // Full-body layers become the held pose; additive-only ones only when there is no
            // full-body pose (an "_add" body animation, played as a pose by `character::animate`).
            if pose.skeletal.iter().any(|l| !l.3) {
                player.last_skeletal.insert(name.clone(), pose.skeletal.clone());
            } else if !pose.skeletal.is_empty() && !player.last_skeletal.get(&name).is_some_and(|l| l.iter().any(|l| !l.3)) {
                player.last_skeletal.insert(name.clone(), pose.skeletal.clone());
            }
            if let (Some(mut s), false) = (skeletal, finished) {
                s.0.extend(pose.skeletal.iter().cloned());
            }
            if hand_off {
                if let Some(idle) = pose.idle {
                    commands.entity(e).insert(crate::character::IdleOverride(idle));
                }
                // Characters without an idle keep the pose the chore ended on.
                if let Some(held) = player.last_skeletal.get(&name) {
                    commands.entity(e).insert(crate::character::HeldPose(held.clone()));
                }
            }
            // Only chores that key the transform move the agent (UI chores must not undo layout).
            if pose.absolute.is_some() || !pose.additive.is_empty() || pose.scale.is_some() {
                let mut t = pose.absolute.unwrap_or(base.0);
                // Positions keyed in another agent's space (assumes the agent itself has no parent).
                if let Some(frame) = pose.frame.as_ref().and_then(|f| player.agents.get(&f.to_ascii_lowercase())) {
                    if let (Some(local), Ok(g)) = (pose.absolute, globals.get(*frame)) {
                        t = Transform::from_matrix(g.to_matrix() * local.to_matrix());
                    }
                }
                for (add, w) in &pose.additive {
                    let d = Transform {
                        translation: add.translation * *w,
                        rotation: Quat::IDENTITY.slerp(add.rotation, *w),
                        scale: Vec3::ONE,
                    };
                    t = t * d;
                }
                if let Some(s) = pose.scale {
                    t.scale = base.0.scale * s;
                }
                *tf = t;
            }
            if let Some(mut mats) = mats {
                if let Some(a) = pose.constant_alpha {
                    for (h, base_alpha) in &mats.meshes {
                        if let Some(mut m) = materials.get_mut(h) {
                            m.base_color.set_alpha(base_alpha * a.max(0.0));
                        }
                    }
                    for (h, base_alpha, _) in &mats.lit {
                        if let Some(mut m) = lit_materials.get_mut(h) {
                            m.base.base_color.set_alpha(base_alpha * a.max(0.0));
                        }
                    }
                }
                if pose.uv_state.is_some() || pose.uv_state_u.is_some() {
                    for (h, _) in &mats.meshes {
                        if let Some(mut m) = materials.get_mut(h) {
                            let mut t = m.uv_transform.translation;
                            t.x = pose.uv_state_u.unwrap_or(t.x);
                            t.y = pose.uv_state.unwrap_or(t.y);
                            m.uv_transform = bevy::math::Affine2::from_translation(t);
                        }
                    }
                }
                for (key, texture) in &pose.textures {
                    for (h, original, _) in mats.lit.iter().filter_map(|(h, a, d)| d.map(|d| (h, d, a))) {
                        let Some(name) = data.name_of(original) else { continue };
                        if Symbol::of(&format!("{} - Texture", name.strip_suffix(".d3dtx").unwrap_or(name))) == *key {
                            let image = cache.image(&data, &mut images, *texture, true);
                            if let Some(mut m) = lit_materials.get_mut(h) {
                                m.base.base_color_texture = image;
                            }
                        }
                    }
                    for (h, original) in &mats.diffuse {
                        let Some(name) = data.name_of(*original) else { continue };
                        if Symbol::of(&format!("{} - Texture", name.strip_suffix(".d3dtx").unwrap_or(name))) != *key {
                            continue;
                        }
                        let image = cache.image(&data, &mut images, *texture, false);
                        if let Some(m) = materials.get(h) {
                            if m.base_color_texture != image {
                                if let Some(mut m) = materials.get_mut(h) {
                                    m.base_color_texture = image;
                                }
                            }
                        }
                    }
                }
                if let Some(c) = pose.text_color {
                    mats.text_color = Some(c);
                    for h in &mats.text {
                        if let Some(mut m) = materials.get_mut(h) {
                            m.base_color = Color::srgba(c[0], c[1], c[2], c[3]);
                        }
                    }
                }
            }
            if let Ok(tg) = toggles.get(e) {
                for (key, on) in &pose.toggles {
                    if let Some(&c) = tg.0.get(key) {
                        commands.entity(c).insert(if *on { Visibility::Inherited } else { Visibility::Hidden });
                    }
                }
            }
            if let Some(v) = pose.visible {
                *vis = if v { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
}
