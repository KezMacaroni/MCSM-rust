//! PropertySet: the typed key/value store behind `.prop` files and scene agents.
//!
//! Layout: u32 version, u32 flags, then a block holding { u32 parent count, parent `.prop`
//! symbols, u32 type count, per type { type symbol, u32 value count, per value { key symbol,
//! value } } }. Values are serialized by their type with no outer block, so an unknown type
//! cannot be skipped on its own; when one is met, the rest of the set is skipped via the block
//! end and `complete` is cleared.

use crate::stream::{MetaStream, Reader};
use crate::symbol::{crc64, Symbol};
use crate::Error;

pub type Vec3 = [f32; 3];
pub type Quat = [f32; 4];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub rot: Quat,
    pub pos: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Transform { rot: [0.0, 0.0, 0.0, 1.0], pos: [0.0; 3] }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Location {
    pub attached_agent: String,
    pub attached_node: Symbol,
    pub local: Transform,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Float(f32),
    Symbol(Symbol),
    String(String),
    Color([f32; 4]),
    Vec2([f32; 2]),
    Vec3(Vec3),
    Vec4([f32; 4]),
    Transform(Transform),
    Location(Location),
    Range([f32; 2]),
    AnimOrChore(Symbol, Symbol),
    Ints(Vec<i32>),
    Bools(Vec<bool>),
    SymbolInts(Vec<(Symbol, i32)>),
    Props(Box<PropertySet>),
    Symbols(Vec<Symbol>),
    Strings(Vec<String>),
    SymbolMap(Vec<(Symbol, Symbol)>),
    StringMap(Vec<(String, String)>),
}

pub mod ty {
    use super::crc64;
    pub const BOOL: u64 = crc64("bool");
    pub const INT32: u64 = crc64("int32");
    pub const INT: u64 = crc64("int");
    pub const LONG: u64 = crc64("long");
    pub const UINT32: u64 = crc64("uint32");
    pub const UINT: u64 = crc64("unsigned int");
    pub const FLOAT: u64 = crc64("float");
    pub const FLAGS: u64 = crc64("Flags");
    pub const SYMBOL: u64 = crc64("Symbol");
    pub const STRING: u64 = crc64("String");
    pub const COLOR: u64 = crc64("Color");
    pub const VECTOR2: u64 = crc64("Vector2");
    pub const VECTOR3: u64 = crc64("Vector3");
    pub const VECTOR4: u64 = crc64("Vector4");
    pub const QUATERNION: u64 = crc64("Quaternion");
    pub const TRANSFORM: u64 = crc64("Transform");
    pub const LOCATION_INFO: u64 = crc64("LocationInfo");
    pub const RANGE_FLOAT: u64 = crc64("TRange<float>");
    pub const ANIM_OR_CHORE: u64 = crc64("AnimOrChore");
    pub const LIGHT_TYPE: u64 = crc64("LightType");
    pub const DCARRAY_INT: u64 = crc64("DCArray<int>");
    pub const DCARRAY_SYMBOL: u64 = crc64("DCArray<Symbol>");
    pub const DCARRAY_STRING: u64 = crc64("DCArray<String>");
    pub const DCARRAY_BOOL: u64 = crc64("DCArray<bool>");
    pub const PROPERTY_SET: u64 = crc64("PropertySet");
    /// Hashes below were identified from their byte patterns; their exact template spellings
    /// do not reproduce with the obvious names.
    pub const SYMBOL_MAP: u64 = 0x9352_ba22_5560_5a10; // Map<Symbol,Symbol,less<Symbol>>
    pub const SYMBOL_SET: u64 = 0x885a_35d9_9a8d_1a4d; // count + symbols ("Light Groups")
    pub const STRING_MAP: u64 = 0x4f1b_64d8_1fc6_0325; // Map<String,String> ("Idle Agent Remap")
    pub const SYMBOL_INT_MAP: u64 = 0x33c5_6d9e_4438_fcd9; // count + (symbol, i32) ("Resource Active Sets")
    pub const MESH_LIST: u64 = 0x5401_81f1_5576_1548; // count + symbols ("D3D Mesh List")
}

fn read_transform(r: &mut Reader) -> Result<Transform, Error> {
    Ok(Transform { rot: r.vec::<4>()?, pos: r.vec::<3>()? })
}

/// Decode a value of type `t`, or `None` if the type is not understood.
fn read_value(r: &mut Reader, t: u64) -> Result<Option<Value>, Error> {
    use ty::*;
    let v = match t {
        BOOL => Value::Bool(r.bool()?),
        INT32 | INT | LONG | UINT32 | UINT | FLAGS | LIGHT_TYPE => Value::Int(r.i32()?),
        FLOAT => Value::Float(r.f32()?),
        SYMBOL => Value::Symbol(r.symbol()?),
        STRING => Value::String(r.string()?),
        COLOR => Value::Color(r.vec::<4>()?),
        VECTOR2 => Value::Vec2(r.vec::<2>()?),
        VECTOR3 => Value::Vec3(r.vec::<3>()?),
        VECTOR4 | QUATERNION => Value::Vec4(r.vec::<4>()?),
        TRANSFORM => Value::Transform(read_transform(r)?),
        LOCATION_INFO => {
            let attached_agent = r.block_string()?;
            let attached_node = r.symbol()?;
            let end = r.block()?;
            let local = read_transform(r)?;
            r.seek(end)?;
            Value::Location(Location { attached_agent, attached_node, local })
        }
        RANGE_FLOAT => Value::Range(r.vec::<2>()?),
        ANIM_OR_CHORE => {
            let mut h = [Symbol::NONE; 2];
            for x in &mut h {
                let end = r.block()?;
                *x = r.symbol()?;
                r.seek(end)?;
            }
            Value::AnimOrChore(h[0], h[1])
        }
        DCARRAY_INT => {
            let n = r.u32()?;
            Value::Ints((0..n).map(|_| r.i32()).collect::<Result<_, _>>()?)
        }
        DCARRAY_SYMBOL | SYMBOL_SET | MESH_LIST => {
            let n = r.u32()?;
            Value::Symbols((0..n).map(|_| r.symbol()).collect::<Result<_, _>>()?)
        }
        DCARRAY_BOOL => {
            let n = r.u32()?;
            Value::Bools((0..n).map(|_| r.bool()).collect::<Result<_, _>>()?)
        }
        DCARRAY_STRING => {
            let n = r.u32()?;
            Value::Strings((0..n).map(|_| r.block_string()).collect::<Result<_, _>>()?)
        }
        SYMBOL_MAP => {
            let n = r.u32()?;
            Value::SymbolMap((0..n).map(|_| Ok((r.symbol()?, r.symbol()?))).collect::<Result<_, Error>>()?)
        }
        SYMBOL_INT_MAP => {
            let n = r.u32()?;
            Value::SymbolInts((0..n).map(|_| Ok((r.symbol()?, r.i32()?))).collect::<Result<_, Error>>()?)
        }
        STRING_MAP => {
            let n = r.u32()?;
            Value::StringMap((0..n).map(|_| Ok((r.string()?, r.string()?))).collect::<Result<_, Error>>()?)
        }
        // A nested set (e.g. "Walk Animation - Auto Act Lookat"): serialized like a whole set,
        // block-wrapped, so even an incomplete one ends cleanly.
        PROPERTY_SET => Value::Props(Box::new(PropertySet::read(r)?)),
        _ if is_handle(t) => Value::Symbol(r.symbol()?),
        _ if is_enum(t) => Value::Int(r.i32()?),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// Engine enum wrappers (`Enum*` classes) serialize as a bare u32 at the top level.
fn is_enum(t: u64) -> bool {
    const ENUMS: &[u64] = &[
        crc64("EnumHTextAlignmentType"),
        crc64("EnumVTextAlignmentType"),
        crc64("EnumTextOrientationType"),
        crc64("EnumDepthOfFieldType"),
        crc64("EnumEmitterColorType"),
        crc64("EnumEmitterSpawnShape"),
        crc64("EnumMeshDebugRenderType"),
        crc64("EnumParticleAffectorType"),
        crc64("EnumParticleGeometryType"),
        crc64("EnumParticleSortMode"),
        crc64("EnumPlatformType"),
        crc64("EnumRenderMaskTest"),
        crc64("EnumRenderMaskWrite"),
        crc64("EnumTonemapType"),
        crc64("EnumeTangentModes"),
        crc64("ZTestFunction"),
        crc64("CameraFacingTypes"),
        crc64("SoundReverbPreset"),
        // "Light Spot Alpha Mode" values: name not recovered; decodes as a u32 like every other
        // engine enum (module_light.prop then parses through to its end).
        0x254e_dc51_7b59_bb47,
    ];
    ENUMS.contains(&t)
}

/// Handles (`Handle<T>`) and enum wrappers serialize as a single symbol / u32; they are
/// recognised from a small list of the template instances seen in this game's data.
fn is_handle(t: u64) -> bool {
    const HANDLES: &[u64] = &[
        crc64("Handle<PropertySet>"),
        crc64("Handle<D3DMesh>"),
        crc64("Handle<T3Texture>"),
        crc64("Handle<Scene>"),
        crc64("Handle<Chore>"),
        crc64("Handle<Font>"),
        crc64("Handle<Dlg>"),
        crc64("Handle<LanguageDB>"),
        crc64("Handle<Skeleton>"),
        crc64("Handle<Animation>"),
        crc64("Handle<StyleGuide>"),
        crc64("Handle<WalkBoxes>"),
    ];
    HANDLES.contains(&t)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropertySet {
    pub parents: Vec<Symbol>,
    pub values: Vec<(Symbol, u64, Value)>,
    /// False when an unknown value type cut decoding short.
    pub complete: bool,
    /// The key and type symbol where decoding stopped, when incomplete.
    pub stopped_at: Option<(Symbol, u64)>,
}

impl PropertySet {
    pub fn read(r: &mut Reader) -> Result<Self, Error> {
        let _version = r.u32()?;
        let _flags = r.u32()?;
        let end = r.block()?;
        let parents = (0..r.u32()?).map(|_| r.symbol()).collect::<Result<_, _>>()?;
        let mut set = PropertySet { parents, values: Vec::new(), complete: true, stopped_at: None };
        'types: for _ in 0..r.u32()? {
            let t = r.symbol()?.0;
            for _ in 0..r.u32()? {
                let key = r.symbol()?;
                match read_value(r, t) {
                    Ok(Some(v)) => set.values.push((key, t, v)),
                    // Unknown type, or a known type whose encoding differs here: keep what was
                    // read and skip the rest of this set.
                    Ok(None) | Err(_) => {
                        set.complete = false;
                        set.stopped_at = Some((key, t));
                        break 'types;
                    }
                }
            }
        }
        r.seek(end)?;
        Ok(set)
    }

    /// A standalone `.prop` file.
    pub fn from_file(bytes: &[u8]) -> Result<Self, Error> {
        let ms = MetaStream::parse(bytes)?;
        PropertySet::read(&mut ms.reader())
    }

    pub fn get(&self, key: Symbol) -> Option<&Value> {
        self.values.iter().find(|(k, _, _)| *k == key).map(|(_, _, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&Value> {
        self.get(Symbol::of(key))
    }
}
