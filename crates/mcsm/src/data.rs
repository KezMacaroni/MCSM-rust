//! Access to the mounted game data: files by name or symbol, property inheritance, and cached
//! conversions of Telltale assets into Bevy assets.

use crate::convert;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use ttarch::install::Vfs;
use ttmeta::{props::Value, Font, PropertySet, Symbol};

#[derive(Resource, Clone)]
pub struct GameData {
    pub vfs: Arc<Vfs>,
    /// Symbol of each archived file name -> (archive index, entry index), highest priority first.
    by_symbol: Arc<HashMap<u64, (usize, usize)>>,
}

impl GameData {
    pub fn new(vfs: Arc<Vfs>) -> Self {
        let mut by_symbol = HashMap::new();
        for (ai, a) in vfs.archives.iter().enumerate() {
            for (ei, e) in a.entries().iter().enumerate() {
                by_symbol.entry(ttmeta::crc64(&e.name)).or_insert((ai, ei));
            }
        }
        GameData { vfs, by_symbol: Arc::new(by_symbol) }
    }

    pub fn name_of(&self, s: Symbol) -> Option<&str> {
        let &(a, e) = self.by_symbol.get(&s.0)?;
        Some(&self.vfs.archives[a].entries()[e].name)
    }

    pub fn read_symbol(&self, s: Symbol) -> Option<Vec<u8>> {
        let &(a, e) = self.by_symbol.get(&s.0)?;
        let archive = &self.vfs.archives[a];
        archive.read(&archive.entries()[e]).map_err(|err| warn!("read failed: {err}")).ok()
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.read_symbol(Symbol::of(name))
    }
}

/// Parsed `.prop` files, and lookups that follow the parent chain like the engine does
/// (the agent's own values first, then each parent depth-first).
#[derive(Resource, Default)]
pub struct Props {
    cache: HashMap<u64, Option<Arc<PropertySet>>>,
}

impl Props {
    pub fn file(&mut self, data: &GameData, s: Symbol) -> Option<Arc<PropertySet>> {
        self.cache
            .entry(s.0)
            .or_insert_with(|| {
                let bytes = data.read_symbol(s)?;
                PropertySet::from_file(&bytes)
                    .map_err(|e| warn!("{}: {e}", data.name_of(s).unwrap_or("?")))
                    .ok()
                    .map(Arc::new)
            })
            .clone()
    }

    pub fn get(&mut self, data: &GameData, set: &PropertySet, key: &str) -> Option<Value> {
        let mut seen = HashSet::new();
        self.get_in(data, set, Symbol::of(key), &mut seen)
    }

    fn get_in(&mut self, data: &GameData, set: &PropertySet, key: Symbol, seen: &mut HashSet<u64>) -> Option<Value> {
        if let Some(v) = set.get(key) {
            return Some(v.clone());
        }
        for &p in &set.parents {
            if !seen.insert(p.0) {
                continue;
            }
            if let Some(parent) = self.file(data, p) {
                if let Some(v) = self.get_in(data, &parent, key, seen) {
                    return Some(v);
                }
            }
        }
        None
    }

    /// True if `ancestor` appears anywhere in the parent chain.
    pub fn inherits(&mut self, data: &GameData, set: &PropertySet, ancestor: &str) -> bool {
        let target = Symbol::of(ancestor);
        let mut stack: Vec<Symbol> = set.parents.clone();
        let mut seen = HashSet::new();
        while let Some(p) = stack.pop() {
            if p == target {
                return true;
            }
            if seen.insert(p.0) {
                if let Some(parent) = self.file(data, p) {
                    stack.extend(parent.parents.iter().copied());
                }
            }
        }
        false
    }

    pub fn float(&mut self, data: &GameData, set: &PropertySet, key: &str) -> Option<f32> {
        match self.get(data, set, key)? {
            Value::Float(f) => Some(f),
            Value::Int(i) => Some(i as f32),
            _ => None,
        }
    }

    pub fn int(&mut self, data: &GameData, set: &PropertySet, key: &str) -> Option<i32> {
        match self.get(data, set, key)? {
            Value::Int(i) => Some(i),
            _ => None,
        }
    }

    pub fn bool(&mut self, data: &GameData, set: &PropertySet, key: &str) -> Option<bool> {
        match self.get(data, set, key)? {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn symbol(&mut self, data: &GameData, set: &PropertySet, key: &str) -> Option<Symbol> {
        match self.get(data, set, key)? {
            Value::Symbol(s) if !s.is_none() => Some(s),
            _ => None,
        }
    }

    pub fn vec3(&mut self, data: &GameData, set: &PropertySet, key: &str) -> Option<[f32; 3]> {
        match self.get(data, set, key)? {
            Value::Vec3(v) => Some(v),
            _ => None,
        }
    }

    pub fn color(&mut self, data: &GameData, set: &PropertySet, key: &str) -> Option<[f32; 4]> {
        match self.get(data, set, key)? {
            Value::Color(c) => Some(c),
            _ => None,
        }
    }

    pub fn string(&mut self, data: &GameData, set: &PropertySet, key: &str) -> Option<String> {
        match self.get(data, set, key)? {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

/// Converted assets, keyed by source symbol.
#[derive(Resource, Default)]
pub struct AssetCache {
    images: HashMap<(u64, bool), Option<Handle<Image>>>,
    meshes: HashMap<u64, Option<Arc<convert::LoadedMesh>>>,
    fonts: HashMap<u64, Option<Arc<(Font, Vec<Handle<Image>>)>>>,
}

impl AssetCache {
    /// `pixel_art`: nearest-neighbour sampling, for the game's block textures.
    pub fn image(&mut self, data: &GameData, images: &mut Assets<Image>, s: Symbol, pixel_art: bool) -> Option<Handle<Image>> {
        self.images
            .entry((s.0, pixel_art))
            .or_insert_with(|| {
                let bytes = data.read_symbol(s)?;
                let tex = ttmeta::Texture::parse(&bytes).map_err(|e| warn!("texture: {e}")).ok()?;
                convert::image(&tex, pixel_art).map(|img| images.add(img))
            })
            .clone()
    }

    pub fn mesh(&mut self, data: &GameData, meshes: &mut Assets<Mesh>, s: Symbol) -> Option<Arc<convert::LoadedMesh>> {
        self.meshes
            .entry(s.0)
            .or_insert_with(|| {
                let bytes = data.read_symbol(s)?;
                let m = ttmeta::Mesh::parse(&bytes).map_err(|e| warn!("mesh: {e}")).ok()?;
                Some(Arc::new(convert::mesh(&m, meshes)))
            })
            .clone()
    }

    pub fn font(&mut self, data: &GameData, images: &mut Assets<Image>, s: Symbol) -> Option<Arc<(Font, Vec<Handle<Image>>)>> {
        self.fonts
            .entry(s.0)
            .or_insert_with(|| {
                let bytes = data.read_symbol(s)?;
                let font = Font::parse(&bytes).map_err(|e| warn!("font: {e}")).ok()?;
                let pages = font.pages.iter().filter_map(|p| convert::image(p, false)).map(|i| images.add(i)).collect();
                Some(Arc::new((font, pages)))
            })
            .clone()
    }
}
