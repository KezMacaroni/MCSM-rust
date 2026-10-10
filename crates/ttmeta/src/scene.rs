//! Scenes: a name plus a list of agents, each an agent name and a PropertySet.
//!
//! Layout (default section): bool hidden, block string name, block (referenced scenes, skipped),
//! block { u32 agent count, per agent { block string name, block PropertySet } }.

use crate::props::PropertySet;
use crate::stream::MetaStream;
use crate::Error;

#[derive(Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub props: PropertySet,
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub name: String,
    pub agents: Vec<Agent>,
}

impl Scene {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let ms = MetaStream::parse(bytes)?;
        let mut r = ms.reader();
        let _hidden = r.bool()?;
        let name = r.block_string()?;
        r.skip_block()?;
        let list_end = r.block()?;
        let count = r.u32()?;
        let mut agents = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let name = r.block_string()?;
            let end = r.block()?;
            let props = PropertySet::read(&mut r)?;
            r.seek(end)?;
            agents.push(Agent { name, props });
        }
        r.seek(list_end)?;
        Ok(Scene { name, agents })
    }
}
