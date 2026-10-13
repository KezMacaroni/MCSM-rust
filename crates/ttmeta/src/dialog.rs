//! Partial runtime index of Season 1 dialogue graphs.
//!
//! Nodes are located by their serialized class symbols, then validated against the common
//! block-wrapped node structure. This is not a complete Dlg deserializer: only starts,
//! scripts, exchanges and chores are executable. Missing/unsupported links must halt the
//! runner, not silently invent a story branch. Member layout learned from reference-only
//! TelltaleToolLib declarations and verified against the user's files (see CREDITS.md).
use crate::{Error, PropertySet, Symbol};
use crate::stream::{MetaStream, Reader};
use crate::symbol::crc64;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum NodeKind {
    Start,
    Unsupported(&'static str),
    UnsupportedDetail(String),
    CancelChoices,
    /// Notes and markers: no runtime effect, execution continues with `next`.
    PassThrough,
    /// Property assignments, each with the scope it names: a logic set (`logic_game`,
    /// `logic_inventory`) or a scene agent (`Reuben`, `obj_logicalTrapDoor`).
    Assign(Vec<(String, PropertySet)>),
    Conditional(Vec<Choice>),
    Jump { target: Symbol, name: Symbol, target_class: u32, behavior: u32 },
    Idle(Symbol),
    Choices { choices: Vec<Choice>, before: Vec<Symbol>, after: Vec<Symbol> },
    Script(String),
    Exchange(Symbol),
    Chore { resource: Symbol, looping: bool },
}

#[derive(Debug, Clone)]
pub struct Choice {
    pub target: Symbol,
    pub props: PropertySet,
    /// Equality tests: (scope, key, expected value, negated).
    pub condition: Vec<(String, Symbol, crate::Value, bool)>,
    /// A Lua expression that must also hold (e.g. `PlayerChoice_IsFemale()`); empty if none.
    pub script: String,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: Symbol,
    pub name: Symbol,
    pub next: Symbol,
    pub props: PropertySet,
    pub kind: NodeKind,
}

#[derive(Debug, Default)]
pub struct Dialog {
    pub nodes: HashMap<Symbol, Node>,
}

impl Dialog {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let ms = MetaStream::parse(bytes)?;
        let mut nodes = HashMap::new();
        let mut failures = Vec::new();
        let types = [crc64("DlgNodeStart"), crc64("DlgNodeScript"), crc64("DlgNodeExchange"), crc64("DlgNodeChore"), crc64("DlgNodeIdle"), crc64("DlgNodeChoices"), crc64("DlgNodeCancelChoices"), crc64("DlgNodeWait"), crc64("DlgNodeConditional"), crc64("DlgNodeLogic"), crc64("DlgNodeJump"), crc64("DlgNodeSequence"), crc64("DlgNodeParallel"), crc64("DlgNodeNotes"), crc64("DlgNodeMarker"), crc64("DlgNodeText")];
        for at in 0..ms.data.len().saturating_sub(8) {
            let class = u64::from_le_bytes(ms.data[at..at + 8].try_into().unwrap());
            let Some(kind) = types.iter().position(|t| *t == class) else { continue };
            match read_node(&mut Reader::at(ms.data, at + 8), kind) {
                Ok(node) => {
                    if nodes.insert(node.id, node).is_some() {
                        return Err(Error::Format("duplicate dialogue node ID".into()));
                    }
                }
                Err(e) => failures.push(format!("{at:#x}: {e}")),
            }
        }
        if nodes.is_empty() { return Err(Error::Unsupported(format!("no supported dialogue nodes: {}", failures.join("; ")))); }
        if !failures.is_empty() { return Err(Error::Format(format!("dialogue node decoding failed: {}", failures.join("; ")))); }
        Ok(Self { nodes })
    }

    pub fn named(&self, name: &str) -> Option<&Node> {
        let symbol = Symbol::of(name);
        self.nodes.values().find(|n| n.name == symbol && matches!(n.kind, NodeKind::Start))
    }
}

fn id_owner(r: &mut Reader) -> Result<Symbol, Error> {
    let outer = r.block()?;
    let inner = r.block()?;
    let id = r.symbol()?;
    if r.pos() != inner || inner != outer { return Err(Error::Format("bad dialogue ID owner".into())); }
    Ok(id)
}

fn link(r: &mut Reader) -> Result<Symbol, Error> {
    let end = r.block()?;
    let id = id_owner(r)?;
    let _context = r.u32()?;
    if r.pos() != end { return Err(Error::Format("bad dialogue link".into())); }
    Ok(id)
}

fn read_node(r: &mut Reader, kind: usize) -> Result<Node, Error> {
    let mut resource = Symbol::NONE;
    let mut looping = false;
    if kind == 2 {
        r.f32()?;
        let end = r.block()?;
        resource = r.symbol()?;
        r.seek(end)?;
    } else if kind == 3 {
        let end = r.block()?;
        resource = r.symbol()?;
        r.seek(end)?;
        r.i32()?;
        looping = r.bool()?;
    }
    let end = r.block()?;
    let id = id_owner(r)?;
    r.skip_block()?; // visibility conditions
    let props_end = r.block()?;
    let object_end = r.block()?;
    let flags = r.u32()?;
    let mut props = PropertySet::default();
    for bit in [1, 2, 4] {
        if flags & bit != 0 {
            let set = PropertySet::read(r)?;
            if bit == 1 { props = set; }
        }
    }
    if r.pos() != object_end || object_end != props_end {
        return Err(Error::Format("bad dialogue properties".into()));
    }
    r.skip_block()?; // unique ID
    let _prev = link(r)?;
    let next = link(r)?;
    let name = r.symbol()?;
    r.u32()?; // node flags
    r.u32()?; // chain context
    if r.pos() != end || id == Symbol::NONE { return Err(Error::Format("bad dialogue base node".into())); }
    let kind = match kind {
        0 => NodeKind::Start,
        1 => NodeKind::Script(r.block_string()?),
        2 => NodeKind::Exchange(resource),
        6 => NodeKind::CancelChoices,
        7 => NodeKind::Unsupported("wait"),
        8 => match read_children(r, false) {
            Ok(cases) => NodeKind::Conditional(cases),
            Err(e) => NodeKind::UnsupportedDetail(format!("conditional: {e}")),
        }
        9 => match read_assignments(r) {
            Ok(props) => NodeKind::Assign(props),
            Err(e) => NodeKind::UnsupportedDetail(format!("logic: {e}")),
        }
        10 => NodeKind::Jump { target: link(r)?, name: r.symbol()?, target_class: r.u32()?, behavior: r.u32()? },
        11 => NodeKind::Unsupported("sequence"),
        12 => NodeKind::Unsupported("parallel"),
        // Writers' notes and markers do nothing when the graph runs.
        13 | 14 => NodeKind::PassThrough,
        15 => NodeKind::Unsupported("text"),
        // A choice set using unsupported conditions stops the runner if it is reached; it does
        // not keep the rest of the graph from indexing.
        5 => match (|| -> Result<NodeKind, Error> {
            let choices = read_children(r, true)?;
            let before = read_children(r, false)?.into_iter().map(|c| c.target).collect();
            let after = read_children(r, false)?.into_iter().map(|c| c.target).collect();
            Ok(NodeKind::Choices { choices, before, after })
        })() {
            Ok(kind) => kind,
            Err(e) => NodeKind::UnsupportedDetail(format!("choices: {e}")),
        },
        4 => {
            let end = r.block()?;
            let resource = r.symbol()?;
            r.seek(end)?;
            NodeKind::Idle(resource)
        }
        _ => NodeKind::Chore { resource, looping },
    };
    Ok(Node { id, name, next, props, kind })
}

fn read_assignments(r: &mut Reader) -> Result<Vec<(String, PropertySet)>, Error> {
    let end = r.block()?;
    r.block_string()?;
    r.block_string()?;
    r.u32()?;
    let conditions = read_group(r)?;
    if conditions.iter().any(|(_, p)| !p.values.is_empty()) { return Err(Error::Unsupported("conditional rule".into())); }
    let actions = read_group(r)?;
    let otherwise = read_group(r)?;
    if otherwise.iter().any(|(_, p)| !p.values.is_empty()) { return Err(Error::Unsupported("rule else actions".into())); }
    r.block_string()?;
    if r.pos() != end { return Err(Error::Format("bad rule boundary".into())); }
    Ok(actions)
}

/// A logic group: per scope (logic set or agent name), the properties it names. Negated tests
/// and non-default operations are rejected.
fn read_group(r: &mut Reader) -> Result<Vec<(String, PropertySet)>, Error> {
    let end = r.block()?;
    r.i32()?;
    let items_end = r.block()?;
    let count = r.u32()?;
    if count > 256 { return Err(Error::Format("too many logic items".into())); }
    let mut scoped = Vec::new();
    for _ in 0..count {
        let scope = r.string()?;
        let props_end = r.block()?;
        let props = PropertySet::read(r)?;
        r.seek(props_end)?;
        if !props.complete { return Err(Error::Unsupported("incomplete logic values".into())); }
        r.block_string()?;
        let negate_end = r.block()?;
        for _ in 0..r.u32()? { r.symbol()?; if r.bool()? { return Err(Error::Unsupported("negated logic".into())); } }
        r.seek(negate_end)?;
        for _ in 0..2 {
            let map_end = r.block()?;
            for _ in 0..r.u32()? { r.symbol()?; if r.i32()? != 0 { return Err(Error::Unsupported("non-default logic operation".into())); } }
            r.seek(map_end)?;
        }
        scoped.push((scope, props));
    }
    if r.pos() != items_end { return Err(Error::Format("bad logic item map".into())); }
    let groups_end = r.block()?;
    if r.u32()? != 0 { return Err(Error::Unsupported("nested logic groups".into())); }
    r.seek(groups_end)?;
    r.i32()?;
    r.i32()?;
    r.block_string()?;
    if r.pos() != end { return Err(Error::Format("bad logic group".into())); }
    Ok(scoped)
}

type Tests = Vec<(String, Symbol, crate::Value, bool)>;

/// A child's visibility condition: property equality tests and a Lua expression.
fn read_visibility(r: &mut Reader) -> Result<(Tests, String), Error> {
    let end = r.block()?;
    let inner = r.block()?;
    r.bool()?;
    let flags = r.u32()?;
    r.skip_block()?;
    let script = r.block_string()?.trim().to_owned();
    let mut tests = Vec::new();
    if flags & 1 != 0 {
        r.block_string()?;
        r.block_string()?;
        r.u32()?;
        let condition = read_group(r)?;
        let actions = read_group(r)?;
        let otherwise = read_group(r)?;
        let empty = |g: &[(String, PropertySet)]| g.iter().all(|(_, p)| p.values.is_empty());
        if !empty(&actions) || !empty(&otherwise) || condition.iter().map(|(_, p)| p.values.len()).sum::<usize>() > 1 {
            return Err(Error::Unsupported("complex visibility rule".into()));
        }
        tests = condition
            .into_iter()
            .flat_map(|(scope, p)| p.values.into_iter().map(move |(key, _, value)| (scope.clone(), key, value, false)))
            .collect();
        r.block_string()?;
    }
    if r.pos() != inner || inner != end { return Err(Error::Format("bad visibility condition boundary".into())); }
    Ok((tests, script))
}

fn read_children(r: &mut Reader, conditions: bool) -> Result<Vec<Choice>, Error> {
    let end = r.block()?;
    let base_end = r.block()?;
    let count = r.u32()?;
    if count > 256 { return Err(Error::Format("too many dialogue children".into())); }
    let mut children = Vec::new();
    for _ in 0..count {
        let class = r.u64()?;
        let expected = if conditions { crc64("DlgChoice") } else { class };
        if class != expected { return Err(Error::Unsupported("unknown choice child".into())); }
        let child_end = r.block()?;
        let head_end = r.block()?;
        id_owner(r)?;
        let target = link(r)?;
        if r.pos() != head_end { return Err(Error::Format("bad child chain head".into())); }
        r.symbol()?; // child name
        let (condition, script) = read_visibility(r)?;
        let props_end = r.block()?;
        let object_end = r.block()?;
        let flags = r.u32()?;
        let mut props = PropertySet::default();
        for bit in [1, 2, 4] {
            if flags & bit != 0 {
                let set = PropertySet::read(r)?;
                if bit == 1 { props = set; }
            }
        }
        if r.pos() != object_end || object_end != props_end { return Err(Error::Format("bad child props".into())); }
        link(r)?; // parent node
        if r.pos() != child_end { return Err(Error::Format("bad child end".into())); }
        if conditions { r.skip_block()?; } // input condition set; selection is explicit in the UI
        children.push(Choice { target, props, condition, script });
    }
    if r.pos() != base_end || base_end != end { return Err(Error::Format("bad child set".into())); }
    Ok(children)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truncated_or_untyped_data_is_rejected() {
        assert!(Dialog::parse(b"5VSM").is_err());
        let mut bytes = b"5VSM".to_vec();
        bytes.extend([0u8; 16]);
        assert!(Dialog::parse(&bytes).is_err());
    }
    fn block(bytes: Vec<u8>) -> Vec<u8> {
        [(bytes.len() as u32 + 4).to_le_bytes().to_vec(), bytes].concat()
    }
    fn owner(id: u64) -> Vec<u8> { block(block(id.to_le_bytes().to_vec())) }
    fn node_link(id: u64) -> Vec<u8> { block([owner(id), 1u32.to_le_bytes().to_vec()].concat()) }
    fn base(id: u64, next: u64, name: &str) -> Vec<u8> {
        let vis = block(block([vec![b'0'], 0u32.to_le_bytes().to_vec(), block(vec![0; 8]), block(vec![0; 4])].concat()));
        block([owner(id), vis, block(block(vec![0; 4])), block(vec![0; 4]),
            node_link(0), node_link(next), Symbol::of(name).0.to_le_bytes().to_vec(),
            0u32.to_le_bytes().to_vec(), 1u32.to_le_bytes().to_vec()].concat())
    }
    #[test]
    fn follows_a_synthetic_start_and_script_chain() {
        let mut data = crc64("DlgNodeStart").to_le_bytes().to_vec();
        data.extend(base(1, 2, "test_start"));
        data.extend(crc64("DlgNodeScript").to_le_bytes());
        data.extend(base(2, 0, "test_script"));
        let script = "local x = 1";
        data.extend(block([(script.len() as u32).to_le_bytes().to_vec(), script.as_bytes().to_vec()].concat()));
        let bytes = [b"5VSM".to_vec(), (data.len() as u32).to_le_bytes().to_vec(), vec![0; 12], data].concat();
        let dialog = Dialog::parse(&bytes).unwrap();
        let start = dialog.named("test_start").unwrap();
        assert_eq!(start.next, Symbol(2));
        assert!(matches!(&dialog.nodes[&start.next].kind, NodeKind::Script(s) if s == script));
    }
    #[test]
    fn id_owner_rejects_wrong_sizes() {
        let bytes = [16u32.to_le_bytes(), 8u32.to_le_bytes(), 1u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
        assert!(id_owner(&mut Reader::new(&bytes)).is_err());
    }
}
