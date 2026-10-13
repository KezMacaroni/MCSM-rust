//! Chores (`.chore`): timelines that drive agents with animations and property tracks.
//!
//! Layout: block-string name, u32 flags, f32 length, u32 resource count, u32 agent count,
//! block editor props, block-string scene file, u32 render delay, block localize info, u32,
//! dependencies { bool present, when present: symbol type, u32 count, strings }, bool tool props,
//! block walk paths, then the resources, then the agents.
//!
//! Resource: u32 version, symbol resource name, f32 length, i32 priority, u32 flags, block-string
//! group, block handle, block control animation, block { u32 count, per block { f32 start,
//! f32 end, bool looping, f32 scale } }, 8 bools (no pose, embedded, enabled, agent resource,
//! 4 view flags), block properties, block group include, u32 auto-act status, then when
//! embedded { symbol type, symbol type, the object }.
//!
//! Agent: block-string name, u32 flags, block { u32 count, i32 resource indices }, block
//! attachment, block actor binding, block rule.
//!
//! Member order per the TelltaleToolLib reading of the engine (reference only, see CREDITS.md);
//! which members are block-wrapped was determined from this game's files.

use crate::anim::Animation;
use crate::stream::{MetaStream, Reader};
use crate::symbol::{crc64, Symbol};
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub start: f32,
    pub end: f32,
    pub looping: bool,
    pub scale: f32,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub name: Symbol,
    pub length: f32,
    pub priority: i32,
    pub flags: u32,
    pub blocks: Vec<Block>,
    pub enabled: bool,
    /// The resource's eight flag bools as stored (no pose, embedded, enabled, agent resource,
    /// four view flags).
    pub bools: [bool; 8],
    /// Property tracks keyed directly in the chore (absolute node transforms, visibility, ...).
    pub control: Animation,
    /// An animation stored inside the chore instead of in a separate `.anm`.
    pub embedded: Option<Animation>,
    /// A nested timeline stored inline (common in dialogue exchanges).
    pub embedded_chore: Option<Box<Chore>>,
}

#[derive(Debug, Clone)]
pub struct ChoreAgent {
    pub name: String,
    pub resources: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Chore {
    pub name: String,
    pub length: f32,
    pub scene: String,
    pub resources: Vec<Resource>,
    pub agents: Vec<ChoreAgent>,
}

const ANIMATION: u64 = crc64("Animation");

impl Chore {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let ms = MetaStream::parse(bytes)?;
        Self::read(&mut ms.reader())
    }

    pub fn read(r: &mut Reader) -> Result<Self, Error> {
        Self::read_depth(r, 0)
    }

    fn read_depth(r: &mut Reader, depth: usize) -> Result<Self, Error> {
        if depth > 32 { return Err(Error::Format("nested chore depth exceeded".into())); }
        let name = r.block_string()?;
        let _flags = r.u32()?;
        let length = r.f32()?;
        let n_res = r.u32()? as usize;
        let n_agents = r.u32()? as usize;
        r.skip_block()?;
        let scene = r.block_string()?;
        let _render_delay = r.u32()?;
        r.skip_block()?;
        let _ = r.u32()?;
        if r.bool()? {
            let _type = r.u64()?;
            for _ in 0..r.u32()? {
                r.string()?;
            }
        }
        let _tool_props = r.bool()?;
        r.skip_block()?;

        let mut resources = Vec::with_capacity(n_res);
        for _ in 0..n_res {
            resources.push(read_resource(r, depth)?);
        }
        let mut agents = Vec::with_capacity(n_agents);
        for _ in 0..n_agents {
            let name = r.block_string()?;
            let _flags = r.u32()?;
            let end = r.block()?;
            let n = r.u32()?;
            let ids = (0..n).map(|_| r.i32().map(|i| i as usize)).collect::<Result<Vec<_>, _>>()?;
            r.seek(end)?;
            r.skip_block()?;
            r.skip_block()?;
            r.skip_block()?;
            agents.push(ChoreAgent { name, resources: ids });
        }
        Ok(Chore { name, length, scene, resources, agents })
    }
}

fn read_resource(r: &mut Reader, depth: usize) -> Result<Resource, Error> {
    let _version = r.u32()?;
    let name = r.symbol()?;
    let length = r.f32()?;
    let priority = r.i32()?;
    let flags = r.u32()?;
    r.skip_block()?;
    r.skip_block()?;
    let control_end = r.block()?;
    let control = Animation::read(r)?;
    r.seek(control_end)?;
    let blocks_end = r.block()?;
    let n = r.u32()?;
    let mut blocks = Vec::with_capacity(n as usize);
    for _ in 0..n {
        blocks.push(Block { start: r.f32()?, end: r.f32()?, looping: r.bool()?, scale: r.f32()? });
    }
    r.seek(blocks_end)?;
    let mut b = [false; 8];
    for x in &mut b {
        *x = r.bool()?;
    }
    r.skip_block()?;
    r.skip_block()?;
    let _auto_act = r.u32()?;
    let mut embedded = None;
    let mut embedded_chore = None;
    if b[1] {
        let t = r.u64()?;
        let _ = r.u64()?;
        match t {
            ANIMATION => embedded = Some(Animation::read(r)?),
            t if t == crc64("Chore") => embedded_chore = Some(Box::new(Chore::read_depth(r, depth + 1)?)),
            _ => return Err(Error::Unsupported(format!("embedded chore object of type {t:#x}"))),
        }
    }
    Ok(Resource { name, length, priority, flags, blocks, enabled: b[2], bools: b, control, embedded, embedded_chore })
}
