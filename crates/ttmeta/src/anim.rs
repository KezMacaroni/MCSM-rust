//! Animations (`.anm`, and the animations embedded in chores), keyframed values only.
//!
//! Layout (versions 4/5): u32 version, u32 flags, symbol name, f32 length, f32 additive mask,
//! bool tool props, then a block { u32 value count, u32 data-buffer size, u32 type count, per
//! type { symbol type, u16 count, u16 version }, the values grouped by type, per value u32 flags,
//! u16 "names follow" (0 = yes), then per value its name symbol }. Member order per the
//! TelltaleToolLib reading of the engine (reference only, see CREDITS.md); byte layout checked
//! against this game's files.
//!
//! A `KeyframedValue<T>` is: block { block { name symbol, u32 flags } }, T min, T max,
//! block { u32 count, per sample { f32 time, bool interpolate-to-next, u32 tangent mode, T } }.
//! Keyframed values, `CompressedSkeletonPoseKeys` and `SkeletonPoseValue` (both as
//! [`SkeletonKeys`]) are decoded; other
//! value types make the animation report `complete = false`, and the enclosing block lets
//! callers skip it.

use crate::stream::Reader;
use crate::symbol::{crc64, Symbol};
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key<T> {
    pub time: f32,
    pub interpolate: bool,
    pub tangent: u32,
    pub value: T,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Track {
    Float(Vec<Key<f32>>),
    Bool(Vec<Key<bool>>),
    Transform(Vec<Key<crate::props::Transform>>),
    Color(Vec<Key<[f32; 4]>>),
    Vec3(Vec<Key<[f32; 3]>>),
    /// Per-bone skeletal animation.
    Skeleton(SkeletonKeys),
    /// A texture handle (texture swaps, keyed by "<texture> - Texture").
    Texture(Vec<Key<Symbol>>),
    /// A keyframed value of a type not decoded here (its keys were skipped).
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub name: Symbol,
    pub flags: u32,
    pub track: Track,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Animation {
    pub name: Symbol,
    pub length: f32,
    pub values: Vec<Value>,
    /// False if a non-keyframed value type was present (it was skipped, not decoded).
    pub complete: bool,
}

const KF_FLOAT: u64 = crc64("KeyframedValue<float>");
const KF_BOOL: u64 = crc64("KeyframedValue<bool>");
const KF_TRANSFORM: u64 = crc64("KeyframedValue<Transform>");
const KF_COLOR: u64 = crc64("KeyframedValue<Color>");
const KF_VEC3: u64 = crc64("KeyframedValue<Vector3>");
const CSPK: u64 = crc64("CompressedSkeletonPoseKeys");
const KF_TEXTURE: u64 = crc64("KeyframedValue<Handle<T3Texture>>");
const SKELETON_POSE: u64 = crc64("SkeletonPoseValue");

/// Raw byte size of intrinsic value types that appear in keyframed values (not block-wrapped).
fn raw_size(type_hash: u64) -> Option<usize> {
    const TABLE: &[(&str, usize)] = &[
        ("KeyframedValue<int>", 4),
        ("KeyframedValue<Color>", 16),
        ("KeyframedValue<Vector2>", 8),
        ("KeyframedValue<Vector3>", 12),
        ("KeyframedValue<Vector4>", 16),
        ("KeyframedValue<Quaternion>", 16),
        ("KeyframedValue<Symbol>", 8),
    ];
    TABLE.iter().find(|(n, _)| crc64(n) == type_hash).map(|(_, s)| *s)
}

fn keys<T>(r: &mut Reader, mut read: impl FnMut(&mut Reader) -> Result<T, Error>) -> Result<Vec<Key<T>>, Error> {
    let end = r.block()?;
    let n = r.u32()?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let time = r.f32()?;
        let interpolate = r.bool()?;
        let tangent = r.u32()?;
        out.push(Key { time, interpolate, tangent, value: read(r)? });
    }
    r.seek(end)?;
    Ok(out)
}

fn transform(r: &mut Reader) -> Result<crate::props::Transform, Error> {
    let end = r.block()?;
    let t = crate::props::Transform { rot: r.vec::<4>()?, pos: r.vec::<3>()? };
    r.seek(end)?;
    Ok(t)
}

impl Animation {
    pub fn read(r: &mut Reader) -> Result<Self, Error> {
        let version = r.u32()?;
        if !(4..=5).contains(&version) {
            return Err(Error::Format(format!("animation version {version}")));
        }
        let _flags = r.u32()?;
        let name = r.symbol()?;
        let length = r.f32()?;
        let _additive_mask = r.f32()?;
        let _tool_props = r.bool()?;
        let end = r.block()?;
        let mut anim = Animation { name, length, values: Vec::new(), complete: true };
        let count = r.u32()?;
        let _buffer = r.u32()?;
        let types = (0..r.u32()?).map(|_| Ok((r.u64()?, r.u16()?, r.u16()?))).collect::<Result<Vec<_>, Error>>()?;
        'types: for (t, n, _version) in types {
            for _ in 0..n {
                if t == CSPK {
                    let size = r.u32()? as usize;
                    let keys = SkeletonKeys::decode(r.bytes(size)?)?;
                    anim.values.push(Value { name: Symbol::NONE, flags: 0, track: Track::Skeleton(keys) });
                    continue;
                }
                if t == SKELETON_POSE {
                    let keys = SkeletonKeys::read_pose_value(r)?;
                    anim.values.push(Value { name: Symbol::NONE, flags: 0, track: Track::Skeleton(keys) });
                    continue;
                }
                if t != KF_FLOAT && t != KF_BOOL && t != KF_TRANSFORM && t != KF_COLOR && t != KF_VEC3 && t != KF_TEXTURE && raw_size(t).is_none() {
                    anim.complete = false;
                    break 'types;
                }
                let base_end = r.block()?;
                let inner_end = r.block()?;
                let vname = r.symbol()?;
                let vflags = r.u32()?;
                r.seek(inner_end)?;
                r.seek(base_end)?;
                let track = match t {
                    KF_FLOAT => {
                        r.skip(8)?;
                        Track::Float(keys(r, |r| r.f32())?)
                    }
                    KF_BOOL => {
                        r.skip(2)?;
                        Track::Bool(keys(r, |r| r.bool())?)
                    }
                    KF_TRANSFORM => {
                        transform(r)?;
                        transform(r)?;
                        Track::Transform(keys(r, transform)?)
                    }
                    KF_VEC3 => {
                        r.skip(24)?;
                        Track::Vec3(keys(r, |r| r.vec::<3>())?)
                    }
                    // Handles: min, max and each key's value are block-wrapped symbols.
                    KF_TEXTURE => {
                        r.skip_block()?;
                        r.skip_block()?;
                        Track::Texture(keys(r, |r| {
                            let end = r.block()?;
                            let s = r.symbol()?;
                            r.seek(end)?;
                            Ok(s)
                        })?)
                    }
                    KF_COLOR => {
                        r.skip(32)?;
                        Track::Color(keys(r, |r| r.vec::<4>())?)
                    }
                    _ => {
                        let size = raw_size(t).unwrap();
                        r.skip(size * 2)?;
                        keys(r, |r| r.skip(size))?;
                        Track::Other
                    }
                };
                anim.values.push(Value { name: vname, flags: vflags, track });
            }
        }
        if anim.complete && anim.values.len() as u32 != count {
            anim.complete = false;
        }
        r.seek(end)?;
        Ok(anim)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let ms = crate::stream::MetaStream::parse(bytes)?;
        Animation::read(&mut ms.reader())
    }

    /// The skeletal track, if the animation has one.
    pub fn skeleton(&self) -> Option<&SkeletonKeys> {
        self.values.iter().find_map(|v| match &v.track {
            Track::Skeleton(k) => Some(k),
            _ => None,
        })
    }

    pub fn value(&self, name: &str) -> Option<&Track> {
        let s = Symbol::of(name);
        self.values.iter().find(|v| v.name == s).map(|v| &v.track)
    }
}

/// Sample a track at `t`: step when the key does not interpolate, linear otherwise.
pub fn sample_float(keys: &[Key<f32>], t: f32) -> Option<f32> {
    let (a, b, f) = bracket(keys, t)?;
    Some(if a.interpolate { a.value + (b.value - a.value) * f } else { a.value })
}

pub fn sample_color(keys: &[Key<[f32; 4]>], t: f32) -> Option<[f32; 4]> {
    let (a, b, f) = bracket(keys, t)?;
    if !a.interpolate {
        return Some(a.value);
    }
    Some(std::array::from_fn(|i| a.value[i] + (b.value[i] - a.value[i]) * f))
}

pub fn sample_vec3(keys: &[Key<[f32; 3]>], t: f32) -> Option<[f32; 3]> {
    let (a, b, f) = bracket(keys, t)?;
    if !a.interpolate {
        return Some(a.value);
    }
    Some(std::array::from_fn(|i| a.value[i] + (b.value[i] - a.value[i]) * f))
}

pub fn sample_bool(keys: &[Key<bool>], t: f32) -> Option<bool> {
    let (a, _, _) = bracket(keys, t)?;
    Some(a.value)
}

/// The key at or before `t`, the key after it, and the blend factor between them.
pub fn bracket<T>(keys: &[Key<T>], t: f32) -> Option<(&Key<T>, &Key<T>, f32)> {
    let first = keys.first()?;
    if t <= first.time {
        return Some((first, first, 0.0));
    }
    let i = keys.iter().rposition(|k| k.time <= t)?;
    let a = &keys[i];
    let b = keys.get(i + 1).unwrap_or(a);
    let span = b.time - a.time;
    let f = if span > 0.0 { ((t - a.time) / span).clamp(0.0, 1.0) } else { 0.0 };
    Some((a, b, f))
}

/// Decoded `CompressedSkeletonPoseKeys` (version 1): per bone, a local rotation and a local
/// translation at every sample.
///
/// The layout was read from the game's own decoder (the function at 0x00843910 in the PC exe);
/// this is an independent implementation of it:
///
/// - header (0x80 bytes): Vector4 min / range for position deltas, rotation deltas and absolute
///   positions (6 x 16 bytes; absolute rotations need none); u32 bone count, sample count,
///   keyframe count, block count; 16 bytes of padding;
/// - one u64 bone symbol per bone;
/// - per keyframe, per group of 4 bones, 64 bytes: 4 x u32 low and 4 x u32 high words for the
///   positions, then the same for rotations. Each field is 10 low + 10 high bits (x) or 11 + 11
///   bits (y, z) mapped linearly onto its range; rotations store x, y, z of a unit quaternion
///   in [-1, 1] and w is rebuilt as non-negative;
/// - per remaining sample, per group of 4 bones, 32 bytes: 4 x u32 position deltas and 4 x u32
///   rotation deltas (10/11/11 bits). Positions add; rotations compose as `prev * delta`;
/// - f32 time per sample, then a u32 per block giving how many delta samples follow each
///   keyframe (a sample is a keyframe whenever the running count reaches zero).
///
/// Positions decode to roughly unit length: they are directions scaled by the bone's rest
/// length (the length of its skeleton local position) at playback.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SkeletonKeys {
    pub bones: Vec<Symbol>,
    pub times: Vec<f32>,
    /// `[bone][sample]`.
    pub positions: Vec<Vec<[f32; 3]>>,
    pub rotations: Vec<Vec<[f32; 4]>>,
}

fn qmul(p: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    let [px, py, pz, pw] = p;
    let [dx, dy, dz, dw] = d;
    [
        pw * dx + px * dw + py * dz - pz * dy,
        pw * dy - px * dz + py * dw + pz * dx,
        pw * dz + px * dy - py * dx + pz * dw,
        pw * dw - px * dx - py * dy - pz * dz,
    ]
}

fn with_w(x: f32, y: f32, z: f32) -> [f32; 4] {
    [x, y, z, (1.0 - x * x - y * y - z * z).max(0.0).sqrt()]
}

impl SkeletonKeys {
    pub fn decode(buf: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(buf);
        let mut range = [[0f32; 4]; 6];
        for v in &mut range {
            *v = r.vec::<4>()?;
        }
        let [min_dv, rng_dv, min_dq, rng_dq, min_v, rng_v] = range;
        let bones = r.u32()? as usize;
        let samples = r.u32()? as usize;
        let keyframes = r.u32()? as usize;
        let blocks = r.u32()? as usize;
        if bones > 4096 || samples > 1 << 20 || keyframes > samples {
            return Err(Error::Format(format!("skeleton keys: {bones} bones, {samples} samples, {keyframes} keyframes")));
        }
        r.skip(16)?;
        let names = (0..bones).map(|_| r.symbol()).collect::<Result<Vec<_>, _>>()?;
        let groups = bones.div_ceil(4);
        let abs = r.bytes(groups * 64 * keyframes)?;
        let delta = r.bytes(groups * 32 * (samples - keyframes))?;
        let times = (0..samples).map(|_| r.f32()).collect::<Result<Vec<_>, _>>()?;
        let table = (0..blocks).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
        if r.remaining() != 0 {
            return Err(Error::Format(format!("skeleton keys: {} trailing bytes", r.remaining())));
        }
        let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        let mut positions = vec![Vec::with_capacity(samples); bones];
        let mut rotations = vec![Vec::with_capacity(samples); bones];
        let (mut ai, mut di, mut bi) = (0, 0, 0);
        let mut count = table.first().copied().unwrap_or(0);
        for s in 0..samples {
            if count == 0 {
                if ai >= keyframes {
                    return Err(Error::Format("skeleton keys: block table overruns keyframes".into()));
                }
                for g in 0..groups {
                    let e = &abs[(ai * groups + g) * 64..][..64];
                    for k in 0..4 {
                        let b = g * 4 + k;
                        if b >= bones {
                            break;
                        }
                        let (vl, vh, ql, qh) = (word(e, k), word(e, 4 + k), word(e, 8 + k), word(e, 12 + k));
                        let x = ((vl & 0x3ff) | (vh & 0x3ff) << 10) as f32;
                        let y = ((vl >> 10 & 0x7ff) | (vh >> 10 & 0x7ff) << 11) as f32;
                        let z = ((vl >> 21) | (vh >> 21) << 11) as f32;
                        positions[b].push([
                            x * rng_v[0] * 9.536_752e-7 + min_v[0],
                            y * rng_v[1] * 2.384_186_4e-7 + min_v[1],
                            z * rng_v[2] * 2.384_186_4e-7 + min_v[2],
                        ]);
                        let x = ((ql & 0x3ff) | (qh & 0x3ff) << 10) as f32;
                        let y = ((ql >> 10 & 0x7ff) | (qh >> 10 & 0x7ff) << 11) as f32;
                        let z = ((ql >> 21) | (qh >> 21) << 11) as f32;
                        rotations[b].push(with_w(x * 1.907_350_5e-6 - 1.0, y * 4.768_372_7e-7 - 1.0, z * 4.768_372_7e-7 - 1.0));
                    }
                }
                ai += 1;
                bi += 1;
                count = table.get(bi).copied().unwrap_or(0);
            } else {
                if di >= samples - keyframes {
                    return Err(Error::Format("skeleton keys: more deltas than stored".into()));
                }
                for g in 0..groups {
                    let e = &delta[(di * groups + g) * 32..][..32];
                    for k in 0..4 {
                        let b = g * 4 + k;
                        if b >= bones {
                            break;
                        }
                        let (v, q) = (word(e, k), word(e, 4 + k));
                        // Additive animations may start with deltas: from zero / identity.
                        let p = if s == 0 { [0.0; 3] } else { positions[b][s - 1] };
                        positions[b].push([
                            p[0] + (v & 0x3ff) as f32 * rng_dv[0] * 9.775_171e-4 + min_dv[0],
                            p[1] + (v >> 10 & 0x7ff) as f32 * rng_dv[1] * 4.885_198e-4 + min_dv[1],
                            p[2] + (v >> 21) as f32 * rng_dv[2] * 4.885_198e-4 + min_dv[2],
                        ]);
                        let d = with_w(
                            (q & 0x3ff) as f32 * rng_dq[0] * 9.775_171e-4 + min_dq[0],
                            (q >> 10 & 0x7ff) as f32 * rng_dq[1] * 4.885_198e-4 + min_dq[1],
                            (q >> 21) as f32 * rng_dq[2] * 4.885_198e-4 + min_dq[2],
                        );
                        let prev = if s == 0 { [0.0, 0.0, 0.0, 1.0] } else { rotations[b][s - 1] };
                        rotations[b].push(qmul(prev, d));
                    }
                }
                di += 1;
                count -= 1;
            }
        }
        Ok(SkeletonKeys { bones: names, times, positions, rotations })
    }

    /// Uncompressed `SkeletonPoseValue`, laid out (from this game's files) as:
    /// block { symbol name, u32 flags }; block { u32 bone count, per bone { symbol, u32 flags } };
    /// block { u32 sample count, per sample { f32 time, f32 (unknown), block { u32 count, per
    /// bone { Quaternion, Vector3 } }, block { u32 count, per bone u32 (unknown) } } }.
    /// Positions follow the same rest-length-direction convention as the compressed keys.
    pub fn read_pose_value(r: &mut Reader) -> Result<Self, Error> {
        r.skip_block()?;
        let end = r.block()?;
        let n = r.u32()? as usize;
        if n > 4096 {
            return Err(Error::Format(format!("skeleton pose with {n} bones")));
        }
        let mut bones = Vec::with_capacity(n);
        for _ in 0..n {
            bones.push(r.symbol()?);
            r.u32()?;
        }
        r.seek(end)?;
        let end = r.block()?;
        let samples = r.u32()? as usize;
        let mut keys = SkeletonKeys {
            bones,
            times: Vec::with_capacity(samples.min(1 << 16)),
            positions: vec![Vec::new(); n],
            rotations: vec![Vec::new(); n],
        };
        for _ in 0..samples {
            keys.times.push(r.f32()?);
            r.f32()?;
            let tf_end = r.block()?;
            if r.u32()? as usize != n {
                return Err(Error::Format("skeleton pose sample with a different bone count".into()));
            }
            for b in 0..n {
                keys.rotations[b].push(r.vec::<4>()?);
                keys.positions[b].push(r.vec::<3>()?);
            }
            r.seek(tf_end)?;
            r.skip_block()?;
        }
        r.seek(end)?;
        Ok(keys)
    }

    /// Sample index at or before `t`, the one after it, and the blend factor.
    pub fn bracket(&self, t: f32) -> Option<(usize, usize, f32)> {
        let first = *self.times.first()?;
        if t <= first {
            return Some((0, 0, 0.0));
        }
        let i = self.times.iter().rposition(|&x| x <= t)?;
        let j = (i + 1).min(self.times.len() - 1);
        let span = self.times[j] - self.times[i];
        Some((i, j, if span > 0.0 { ((t - self.times[i]) / span).clamp(0.0, 1.0) } else { 0.0 }))
    }
}
