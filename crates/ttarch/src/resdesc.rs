//! Resource set descriptions (`Archives/_resdesc_50_*.lua`).
//!
//! Each is an encrypted Lua source file that fills a `set` table and calls
//! `RegisterSetDescription(set)`. The fields used here: `name`, `priority` (higher overrides
//! lower), `descriptionPriority` (picks between several descriptions of the same set),
//! `enableMode` (`bootable` / `constant` are always on; `localization` sets are language packs),
//! and `gameDataArchives` (the `.ttarch2` files the set mounts).
//!
//! The files are simple enough to read line by line instead of running a Lua VM at boot.

use crate::{script, Blowfish7, Error};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ResourceSet {
    pub name: String,
    pub priority: i32,
    pub description_priority: i32,
    pub enable_mode: String,
    /// Archive file names, relative to the Archives folder.
    pub archives: Vec<String>,
}

impl ResourceSet {
    pub fn is_localization(&self) -> bool {
        self.enable_mode == "localization"
    }
}

pub fn parse(source: &str) -> Option<ResourceSet> {
    let field = |key: &str| {
        source.lines().find_map(|l| {
            let rest = l.trim().strip_prefix("set.")?.strip_prefix(key)?.trim_start();
            Some(rest.strip_prefix('=')?.trim().to_owned())
        })
    };
    let unquote = |s: String| s.trim_matches('"').to_owned();
    let start = source.find("set.gameDataArchives")?;
    let block = &source[start..];
    let block = &block[..block.find('}').unwrap_or(block.len())];
    let archives = block
        .split('"')
        .filter(|s| s.to_ascii_lowercase().ends_with(".ttarch2"))
        .map(str::to_owned)
        .collect();
    Some(ResourceSet {
        name: unquote(field("name")?),
        priority: field("priority")?.parse().ok()?,
        description_priority: field("descriptionPriority").and_then(|v| v.parse().ok()).unwrap_or(0),
        enable_mode: unquote(field("enableMode").unwrap_or_default()),
        archives,
    })
}

/// Read every description in `archives_dir`, keep the winning description per set, drop
/// language packs, and return the sets ordered highest priority first.
pub fn load(archives_dir: &Path, cipher: &Blowfish7) -> Result<Vec<ResourceSet>, Error> {
    let mut by_name: HashMap<String, ResourceSet> = HashMap::new();
    let entries = std::fs::read_dir(archives_dir).map_err(|e| Error::Io(archives_dir.to_owned(), e))?;
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        let file = path.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        if !(file.starts_with("_resdesc_") && file.ends_with(".lua")) {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| Error::Io(path.clone(), e))?;
        let text = String::from_utf8_lossy(&script::decrypt(cipher, &bytes)).into_owned();
        let Some(set) = parse(&text) else { continue };
        let keep = by_name
            .get(&set.name)
            .is_none_or(|old| set.description_priority > old.description_priority);
        if keep {
            by_name.insert(set.name.clone(), set);
        }
    }
    let mut sets: Vec<ResourceSet> = by_name.into_values().filter(|s| !s.is_localization()).collect();
    sets.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.name.cmp(&b.name)));
    Ok(sets)
}

/// Archive paths in lookup order (highest priority first), each listed once.
pub fn archive_order(archives_dir: &Path, sets: &[ResourceSet]) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    sets.iter()
        .flat_map(|s| s.archives.iter())
        .filter(|a| seen.insert(a.to_ascii_lowercase()))
        .map(|a| archives_dir.join(a))
        .filter(|p| p.is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_set_description() {
        let src = "local set = {}\nset.name = \"PC_1_Menu\"\nset.priority = 720\nset.enableMode = \"constant\"\n\
                   set.descriptionPriority = 0\nset.gameDataArchives = \n{\n    _currentDirectory .. \"a_menu.ttarch2\"\n}\n\
                   RegisterSetDescription(set)\n";
        let s = parse(src).unwrap();
        assert_eq!((s.name.as_str(), s.priority, s.enable_mode.as_str()), ("PC_1_Menu", 720, "constant"));
        assert_eq!(s.archives, vec!["a_menu.ttarch2"]);
    }
}
