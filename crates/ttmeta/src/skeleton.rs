//! Skeletons (`.skl`).
//!
//! Layout: block { u32 bone count, per bone an `Entry` }. Entry member order follows
//! TelltaleToolKit's `Skeleton.Entry` (MIT, see CREDITS.md); this game's files omit the bone
//! length / direction / rotation-adjust members, which was checked by reading every file to its
//! exact end. Per entry: symbol joint name, symbol parent name, i32 parent index, symbol mirror
//! bone, i32 mirror index, Vector3 local position, Quaternion local rotation, block { rest
//! Transform }, Vector3 global / local / animation translation scale, block { resource-group
//! map }, block { constraints }, u32 flags.

use crate::stream::{MetaStream, Reader};
use crate::symbol::Symbol;
use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: Symbol,
    pub parent: Option<usize>,
    /// Rest pose relative to the parent bone.
    pub local_pos: [f32; 3],
    pub local_rot: [f32; 4],
    pub anim_translation_scale: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
}

impl Skeleton {
    pub fn read(r: &mut Reader) -> Result<Self, Error> {
        let end = r.block()?;
        let n = r.u32()?;
        if n > 4096 {
            return Err(Error::Format(format!("skeleton with {n} bones")));
        }
        let mut bones = Vec::with_capacity(n as usize);
        for i in 0..n as usize {
            let name = r.symbol()?;
            let _parent_name = r.symbol()?;
            let parent = r.i32()?;
            let _mirror = r.symbol()?;
            let _mirror_index = r.i32()?;
            let local_pos = r.vec::<3>()?;
            let local_rot = r.vec::<4>()?;
            r.skip_block()?;
            let _global_ts = r.vec::<3>()?;
            let _local_ts = r.vec::<3>()?;
            let anim_translation_scale = r.vec::<3>()?;
            r.skip_block()?;
            r.skip_block()?;
            let _flags = r.u32()?;
            let parent = match usize::try_from(parent) {
                Ok(p) if p < i => Some(p),
                Ok(p) => return Err(Error::Format(format!("bone {i} has parent {p}"))),
                Err(_) => None,
            };
            bones.push(Bone { name, parent, local_pos, local_rot, anim_translation_scale });
        }
        r.seek(end)?;
        Ok(Skeleton { bones })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let ms = MetaStream::parse(bytes)?;
        let mut r = ms.reader();
        let s = Skeleton::read(&mut r)?;
        if r.remaining() != 0 {
            return Err(Error::Format(format!("{} bytes after the skeleton", r.remaining())));
        }
        Ok(s)
    }

    pub fn index_of(&self, name: Symbol) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }
}
