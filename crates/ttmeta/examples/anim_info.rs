//! Summarize animations from the install: `cargo run -p ttmeta --example anim_info -- <game-dir> <file.anm>...`.
//! Prints the value types present, whether everything decoded, and the skeletal track's size.
use ttmeta::anim::Track;

/// Animated value type names seen in Telltale games, to name type hashes.
const CANDIDATES: &[&str] = &[
    "CompressedSkeletonPoseKeys", "CompressedSkeletonPoseKeys2", "SkeletonPoseValue", "SkeletonPoseCompoundValue",
    "CompressedVector3Keys", "CompressedVector3Keys2", "CompressedQuaternionKeys", "CompressedQuaternionKeys2",
    "CompressedTransformKeys", "CompressedPathBlockingValue", "CompressedPhonemeKeys", "CompressedPathInfoKeys",
    "KeyframedValue<Transform>", "KeyframedValue<Quaternion>", "KeyframedValue<Vector3>", "KeyframedValue<float>",
    "KeyframedValue<bool>", "KeyframedValue<Color>", "KeyframedValue<Symbol>", "KeyframedValue<int>",
    "KeyframedValue<Handle<T3Texture>>", "KeyframedValue<PhonemeKey>", "KeyframedValue<Vector2>", "KeyframedValue<Vector4>",
    "KeyframedValue<String>", "KeyframedValue<Polar>", "KeyframedValue<SoundEventName<0>>", "KeyframedValue<ScriptEnum>",
    "AnimatedValueInterface<Transform>", "AnimatedValueInterface<Vector3>", "AnimatedValueInterface<Quaternion>",
    "AnimatedValueInterface<float>", "AnimatedValueInterface<SkeletonPose>", "CompressedKeys<Vector3>",
    "CompressedKeys<Quaternion>", "CompressedKeys<float>", "CompressedKeys<bool>", "CompressedKeys<Transform>",
    "CompressedKeys<Color>", "CompressedKeys<Symbol>", "CompressedKeys<int>", "CompressedKeys<Vector2>",
    "CompressedKeys<Handle<T3Texture>>", "CompressedKeys<String>", "CompressedKeys<PhonemeKey>",
    "SingleValue<Transform>", "SingleValue<Vector3>", "SingleValue<Quaternion>", "SingleValue<float>",
    "SingleValue<bool>", "SingleValue<Symbol>", "SingleValue<Color>", "SingleValue<Handle<T3Texture>>",
    "ProceduralEyes", "Procedural_LookAt", "Procedural_LookAt_Value", "LookAtValue", "AnimationMixerValueInfo",
    "Animation", "AnimOrChore", "T3VertexBufferSample<T3PositionSampleData,T3HeapAllocator>",
];
fn main() {
    let mut args = std::env::args().skip(1);
    let game = args.next().expect("game dir");
    let vfs = ttarch::Install::open(game.as_ref()).unwrap().mount().unwrap();
    for name in args {
        let Some(bytes) = vfs.read(&name) else {
            println!("{name}: not found");
            continue;
        };
        let bytes = bytes.unwrap();
        match ttmeta::anim::Animation::parse(&bytes) {
            Err(e) => println!("{name}: {} bytes, parse error: {e}", bytes.len()),
            Ok(a) => {
                if !a.complete {
                    // The value-type table, to name what was not decoded.
                    let ms = ttmeta::stream::MetaStream::parse(&bytes).unwrap();
                    let mut r = ms.reader();
                    let (_, _, _, _, _, _) = (r.u32(), r.u32(), r.symbol(), r.f32(), r.f32(), r.bool());
                    let _ = r.block();
                    let (_, _) = (r.u32(), r.u32());
                    for _ in 0..r.u32().unwrap_or(0) {
                        let (t, n, v) = (r.u64().unwrap(), r.u16().unwrap(), r.u16().unwrap());
                        let known = ms.types.iter().find(|(s, _)| s.0 == t).map(|(_, ver)| *ver);
                        let name = CANDIDATES.iter().find(|c| ttmeta::symbol::crc64(c) == t).copied().unwrap_or("?");
                        println!("  type {t:016x} {name} x{n} version {v} (stream type crc {known:?})");
                    }
                    println!("  stream types: {:?}", ms.types.iter().map(|(s, v)| format!("{:016x}/{v:08x}", s.0)).collect::<Vec<_>>());
                }
                let kinds: Vec<&str> = a
                    .values
                    .iter()
                    .map(|v| match v.track {
                        Track::Float(_) => "float",
                        Track::Bool(_) => "bool",
                        Track::Transform(_) => "transform",
                        Track::Color(_) => "color",
                        Track::Vec3(_) => "vec3",
                        Track::Skeleton(_) => "skeleton",
                        Track::Texture(_) => "texture",
                        Track::Other => "other",
                    })
                    .collect();
                println!(
                    "{name}: {} bytes, flags {:#x}, additive mask {}, length {}, complete {}, values {:?}, skeleton {:?}",
                    bytes.len(),
                    a.flags,
                    a.additive_mask,
                    a.length,
                    a.complete,
                    kinds,
                    a.skeleton().map(|k| format!("{k:?}").chars().take(160).collect::<String>())
                );
            }
        }
    }
}
