//! Skinned characters: a skeleton of joint entities, the agent's visible meshes skinned to it,
//! and skeletal animation (the prop's idle, or whatever a chore plays on the agent).
//!
//! A character agent's props give "Skeleton File", "D3D Mesh List" (every mesh variant: hair,
//! hats, masks, mouth shapes, ...) and per mesh a "Mesh <name> - Visible" bool choosing the
//! variants shown. "Walk Animation - Idle" names the idle (an animation, or a chore whose
//! animation resource is the idle).

use crate::data::GameData;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use ttmeta::anim::SkeletonKeys;
use ttmeta::props::Value;
use ttmeta::{Animation, Skeleton, Symbol};

/// Joints of a spawned skeleton, for skinning.
pub struct Skin {
    pub joints: HashMap<Symbol, Entity>,
    /// Bind pose of each joint in the agent's space.
    pub bind: HashMap<Symbol, Mat4>,
    pub root: Entity,
}

#[derive(Clone, Copy)]
struct Joint {
    entity: Entity,
    rest: Transform,
    /// Length of the rest offset from the parent: animation positions are directions scaled
    /// by it (see `ttmeta::anim::SkeletonKeys`).
    length: f32,
}

enum Idle {
    Anim(Arc<Animation>),
    /// A looping chore of layered animations (base idle, weight shifts, gestures).
    Chore(crate::chore::ChorePlayer),
}

impl Idle {
    fn length(&self) -> f32 {
        match self {
            Idle::Anim(a) => a.length,
            Idle::Chore(c) => c.chore.length,
        }
    }
}

/// The prop's "Walk Animation - Forward", played whenever the agent moves (the engine's walk
/// animator): a looping cycle advanced by distance travelled.
struct Walk {
    anim: Arc<Animation>,
    /// Ground speed of the cycle, from its `relativeNode` (root motion) track.
    speed: f32,
    phase: f32,
    weight: f32,
    last: Option<Vec3>,
}

#[derive(Component)]
pub struct Rig {
    joints: HashMap<Symbol, Joint>,
    /// The skeleton's root bone: layers that animate it pose the body (face layers do not).
    root: Symbol,
    /// The idle the character's own prop names, when the scene blanked it: a base for
    /// additive layers only.
    base_idle: Option<Arc<Animation>>,
    idle: Option<Idle>,
    walk: Option<Walk>,
    time: f32,
}

/// An idle loop a finished chore left the agent in (replaces the prop's idle).
#[derive(Component)]
pub struct IdleOverride(pub Arc<Animation>);

/// The final pose of a finished chore, held by characters that have no idle.
#[derive(Component)]
pub struct HeldPose(pub Vec<(Arc<Animation>, f32, f32, bool)>);

/// Skeletal animations a chore plays on this agent this frame: (animation, time, weight,
/// additive). Replaces the idle while non-empty; consumed every frame.
#[derive(Component, Default)]
pub struct SkeletalOverride(pub Vec<(Arc<Animation>, f32, f32, bool)>);

/// Chores and props switch the meshes of a "D3D Mesh List" agent on and off by name
/// (`Mesh <mesh> - Visible`): mouth shapes, outfit variants, the blocks of a block-building
/// structure. Each mesh gets its own container entity, keyed by that property's symbol.
#[derive(Component, Default)]
pub struct MeshToggles(pub HashMap<Symbol, Entity>);

/// One container entity per listed mesh (hidden when its `Mesh <name> - Visible` is false),
/// registered in the agent's [`MeshToggles`]. Returns (container, mesh) pairs.
pub fn mesh_list(ctx: &mut crate::stage::Ctx, entity: Entity, props: &ttmeta::PropertySet, meshes: &[Symbol]) -> Vec<(Entity, Symbol)> {
    let mut toggles = HashMap::new();
    let mut out = Vec::new();
    for &m in meshes {
        let Some(stem) = mesh_stem(ctx.data, m) else { continue };
        let key = format!("Mesh {stem} - Visible");
        let visible = ctx.props.bool(ctx.data, props, &key) != Some(false);
        let container = ctx
            .commands
            .spawn((Transform::IDENTITY, if visible { Visibility::Inherited } else { Visibility::Hidden }, ChildOf(entity)))
            .id();
        toggles.insert(Symbol::of(&key), container);
        out.push((container, m));
    }
    ctx.commands.entity(entity).insert(MeshToggles(toggles));
    out
}

fn mesh_stem(data: &GameData, s: Symbol) -> Option<String> {
    let name = data.name_of(s)?;
    Some(name.strip_suffix(".d3dmesh").unwrap_or(name).to_owned())
}

/// The idle named by "Walk Animation - Idle".
fn idle_animation(ctx: &mut crate::stage::Ctx, props: &ttmeta::PropertySet) -> Option<Idle> {
    let Some(Value::AnimOrChore(anim, chore)) = ctx.props.get(ctx.data, props, "Walk Animation - Idle") else { return None };
    if anim != Symbol::NONE {
        let a = ctx.data.read_symbol(anim).and_then(|b| Animation::parse(&b).ok()).filter(|a| a.skeleton().is_some())?;
        return Some(Idle::Anim(Arc::new(a)));
    }
    let chore = ttmeta::Chore::parse(&ctx.data.read_symbol(chore)?).ok()?;
    Some(Idle::Chore(crate::chore::ChorePlayer::new(ctx.data, chore, Arc::new(HashMap::new()), true)))
}

/// A new "Walk Animation - Idle" set while the game runs (a dialogue logic node): the
/// animation or chore symbol it names, or neither to clear the idle.
#[derive(Component)]
pub struct SetIdle(pub Symbol, pub Symbol);

/// Swap in idles requested by [`SetIdle`].
pub fn apply_set_idle(mut commands: Commands, data: Option<Res<GameData>>, mut rigs: Query<(Entity, &mut Rig, &SetIdle)>) {
    let Some(data) = data else { return };
    for (e, mut rig, set) in &mut rigs {
        commands.entity(e).remove::<(SetIdle, IdleOverride, HeldPose)>();
        rig.idle = if set.0 != Symbol::NONE {
            crate::chore::load_animation(&data, set.0).filter(|a| a.skeleton().is_some()).map(Idle::Anim)
        } else if set.1 != Symbol::NONE {
            crate::chore::chore_by_symbol(&data, set.1).map(|c| Idle::Chore(crate::chore::ChorePlayer::new(&data, c, Arc::new(HashMap::new()), true)))
        } else {
            None
        };
        if rig.idle.is_none() && (set.0 != Symbol::NONE || set.1 != Symbol::NONE) {
            warn!("idle {:?} / {:?} did not load", data.name_of(set.0), data.name_of(set.1));
        }
    }
}

/// The first full-body idle animation named by the agent's parent props (its character prop).
fn parent_idle(ctx: &mut crate::stage::Ctx, props: &ttmeta::PropertySet) -> Option<Arc<Animation>> {
    props.parents.iter().find_map(|&p| {
        let parent = ctx.props.file(ctx.data, p)?;
        match idle_animation(ctx, &parent)? {
            Idle::Anim(a) => Some(a),
            Idle::Chore(c) => c.skeletal_layers(ctx.data, 0.0).into_iter().find(|l| !l.3).map(|l| l.0),
        }
    })
}

fn walk_animation(ctx: &mut crate::stage::Ctx, props: &ttmeta::PropertySet) -> Option<Walk> {
    let Some(Value::AnimOrChore(anim, _)) = ctx.props.get(ctx.data, props, "Walk Animation - Forward") else { return None };
    let anim = ctx.data.read_symbol(anim).and_then(|b| Animation::parse(&b).ok()).filter(|a| a.skeleton().is_some())?;
    let stride = match anim.value("relativeNode") {
        Some(ttmeta::anim::Track::Transform(k)) if k.len() >= 2 => {
            let (a, b) = (k.first().unwrap().value.pos, k.last().unwrap().value.pos);
            Vec2::new(b[0] - a[0], b[2] - a[2]).length()
        }
        _ => 0.0,
    };
    let speed = if stride > 0.01 && anim.length > 0.0 { stride / anim.length } else { 1.2 };
    Some(Walk { anim: Arc::new(anim), speed, phase: 0.0, weight: 0.0, last: None })
}

/// Spawn the skeleton and meshes of a character agent. Returns its bone entities by bone
/// name (for agents attached to a bone, e.g. a held torch), or None if `props` do not
/// describe a character.
pub fn spawn(
    ctx: &mut crate::stage::Ctx,
    entity: Entity,
    agent_name: &str,
    props: &ttmeta::PropertySet,
    mut spawn_mesh: impl FnMut(&mut crate::stage::Ctx, Entity, Symbol, &Skin),
) -> Option<HashMap<Symbol, Entity>> {
    let Some(Value::Symbols(meshes)) = ctx.props.get(ctx.data, props, "D3D Mesh List") else { return None };
    let Some(skl) = ctx.props.symbol(ctx.data, props, "Skeleton File") else { return None };
    let skeleton = match ctx.data.read_symbol(skl).map(|b| Skeleton::parse(&b)) {
        Some(Ok(s)) => s,
        Some(Err(e)) => {
            warn!("{agent_name}: skeleton: {e}");
            return None;
        }
        None => {
            warn!("{agent_name}: skeleton {} missing", ctx.data.name_of(skl).unwrap_or("?"));
            return None;
        }
    };

    let mut entities: Vec<Entity> = Vec::with_capacity(skeleton.bones.len());
    let mut global: Vec<Mat4> = Vec::with_capacity(skeleton.bones.len());
    let mut joints = HashMap::new();
    let mut bind = HashMap::new();
    for bone in &skeleton.bones {
        let rest = Transform {
            translation: crate::convert::position(bone.local_pos),
            rotation: crate::convert::rotation(bone.local_rot),
            scale: Vec3::ONE,
        };
        let parent = bone.parent.map_or(entity, |p| entities[p]);
        let e = ctx.commands.spawn((rest, Visibility::Inherited, ChildOf(parent))).id();
        let g = bone.parent.map_or(Mat4::IDENTITY, |p| global[p]) * rest.to_matrix();
        entities.push(e);
        global.push(g);
        joints.insert(bone.name, Joint { entity: e, rest, length: Vec3::from_array(bone.local_pos).length() });
        bind.insert(bone.name, g);
    }
    let Some(&root) = entities.first() else { return None };
    let skin = Skin { joints: joints.iter().map(|(s, j)| (*s, j.entity)).collect(), bind, root };

    for (container, m) in mesh_list(ctx, entity, props, &meshes) {
        spawn_mesh(ctx, container, m, &skin);
    }

    let idle = idle_animation(ctx, props);
    // A blank idle ((none, none)) is deliberate: the agent holds chore poses instead.
    let blank = matches!(ctx.props.get(ctx.data, props, "Walk Animation - Idle"), Some(Value::AnimOrChore(a, c)) if a == Symbol::NONE && c == Symbol::NONE);
    if idle.is_none() && !blank && ctx.props.get(ctx.data, props, "Walk Animation - Idle").is_some() {
        warn!("{agent_name}: idle animation did not load");
    }
    // Desynchronize idles of identical characters.
    let phase = (Symbol::of(agent_name).0 % 997) as f32 / 997.0 * idle.as_ref().map_or(0.0, Idle::length);
    let walk = walk_animation(ctx, props);
    let root_bone = skeleton.bones[0].name;
    let base_idle = if idle.is_none() { parent_idle(ctx, props) } else { None };
    let bones = joints.iter().map(|(s, j)| (*s, j.entity)).collect();
    ctx.commands.entity(entity).insert((Rig { joints, root: root_bone, base_idle, idle, walk, time: phase }, SkeletalOverride::default()));
    Some(bones)
}

fn sample(k: &SkeletonKeys, bone: usize, t: f32) -> Option<(Vec3, Quat)> {
    let (i, j, f) = k.bracket(t)?;
    let p = Vec3::from_array(k.positions[bone][i]).lerp(Vec3::from_array(k.positions[bone][j]), f);
    let a = crate::convert::rotation(k.rotations[bone][i]);
    let b = crate::convert::rotation(k.rotations[bone][j]);
    Some((p, a.slerp(b, f).normalize()))
}

pub fn animate(
    time: Res<Time>,
    data: Option<Res<GameData>>,
    mut rigs: Query<(&mut Rig, &mut SkeletalOverride, &GlobalTransform, Option<&IdleOverride>, Option<&HeldPose>, Option<&Name>)>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(data) = data else { return };
    let dt = time.delta_secs().min(1.0 / 30.0);
    let log = std::env::var("MCSM_RIG_LOG").ok();
    for (mut rig, mut over, global, idle_override, held, name) in &mut rigs {
        rig.time += dt;
        let rig = &mut *rig;
        let mut layers = std::mem::take(&mut over.0);
        let root = rig.root;
        let poses_body = |l: &(Arc<Animation>, f32, f32, bool)| !l.3 && l.0.skeleton().is_some_and(|k| k.bones.contains(&root));
        let chore_drives_body = layers.iter().any(poses_body);
        if !chore_drives_body {
            // No chore drives the whole body: the idle plays underneath.
            match &rig.idle {
                _ if idle_override.is_some() => {
                    let a = &idle_override.unwrap().0;
                    layers.push((a.clone(), rig.time % a.length.max(0.01), 1.0, false));
                }
                Some(Idle::Anim(a)) => layers.push((a.clone(), rig.time % a.length.max(0.01), 1.0, false)),
                Some(Idle::Chore(c)) => layers.extend(c.skeletal_layers(&data, rig.time % c.chore.length.max(0.01))),
                None => {
                    if let Some(h) = held {
                        layers.extend(h.0.iter().cloned());
                    }
                }
            }
        }
        // Additive body layers with nothing underneath would compose onto the T-pose: put the
        // character's own idle under them (the diorama's running Axel, whose scene entry blanks
        // the idle his character prop names).
        if !layers.iter().any(poses_body) && layers.iter().any(|l| l.3 && l.0.skeleton().is_some_and(|k| k.bones.contains(&root))) {
            if let Some(base) = &rig.base_idle {
                layers.push((base.clone(), rig.time % base.length.max(0.01), 1.0, false));
            }
        }
        // Moving agents walk: blend the walk cycle in by ground speed and advance it by distance,
        // so feet plant instead of sliding. Jumps (teleports) do not count as movement.
        if let Some(walk) = &mut rig.walk {
            let pos = global.translation();
            let moved = walk.last.map_or(0.0, |l| Vec2::new(pos.x - l.x, pos.z - l.z).length());
            walk.last = Some(pos);
            let speed = if dt > 0.0 { moved / dt } else { 0.0 };
            // Faster than a run is a jump (block-building time-lapse hops, parking), not a walk;
            // a chore playing a full-body animation also owns the legs.
            let speed = if speed > 5.0 || chore_drives_body { 0.0 } else { speed };
            let target = ((speed - 0.05) / 0.25).clamp(0.0, 1.0);
            walk.weight += (target - walk.weight) * (dt * 8.0).min(1.0);
            walk.phase = (walk.phase + dt * speed / walk.speed) % walk.anim.length.max(0.01);
            if walk.weight > 0.01 {
                layers.push((walk.anim.clone(), walk.phase, walk.weight, false));
            }
        }
        // MCSM_RIG_LOG=<agent>: the layers posing that agent this frame (debugging).
        if log.as_deref().is_some_and(|l| name.is_some_and(|n| n.as_str() == l)) {
            info!(
                "{l:?}: idle {} override {} held {} layers {:?}",
                rig.idle.is_some(),
                idle_override.is_some(),
                held.map_or(0, |h| h.0.len()),
                layers.iter().map(|(a, t, w, add)| (data.name_of(a.name).unwrap_or("?").to_owned(), *t, *w, *add)).collect::<Vec<_>>(),
                l = log
            );
        }
        // Base layers first, additive ones on top.
        layers.sort_by_key(|l| l.3);
        // Start from the rest pose; base layers blend over it by weight, additive ones compose.
        let mut pose: HashMap<Symbol, (Vec3, Quat)> = rig.joints.iter().map(|(s, j)| (*s, (j.rest.translation, j.rest.rotation))).collect();
        for (anim, t, w, additive) in &layers {
            let Some(k) = anim.skeleton() else { continue };
            let mut apply = |name: &Symbol, p: Vec3, rot: Quat| {
                let Some(cur) = pose.get_mut(name) else { return };
                if *additive {
                    cur.1 = (cur.1 * Quat::IDENTITY.slerp(rot, *w)).normalize();
                    cur.0 += p * *w;
                } else {
                    cur.0 = cur.0.lerp(p, *w);
                    cur.1 = cur.1.slerp(rot, *w);
                }
            };
            for (b, name) in k.bones.iter().enumerate() {
                let Some(joint) = rig.joints.get(name) else { continue };
                let Some((dir, rot)) = sample(k, b, *t) else { continue };
                apply(name, dir * joint.length, rot);
            }
        }
        if log.as_deref().is_some_and(|l| name.is_some_and(|n| n.as_str() == l)) {
            info!("root {:?}: {:?} (rest {:?})", root, pose.get(&root), rig.joints.get(&root).map(|j| j.rest.translation));
        }
        for (name, (p, r)) in pose {
            if let Ok(mut tf) = transforms.get_mut(rig.joints[&name].entity) {
                tf.translation = p;
                tf.rotation = r;
            }
        }
    }
}
