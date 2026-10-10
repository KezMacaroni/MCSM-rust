//! Localized text: language databases (`.landb`) and dialog label lookup (`.dlog`).
//!
//! A LanguageDB record starts with { u32 id, u64 crc64("<id>.lang"), u32 id, u32 0 }, then two
//! 12-byte symbol blocks, a string block, u32 0, a u32 block size, a block-string speaker and a
//! block-string text (field order per TTG Tools' LandbClass, GPL-3.0). The id/hash pair is
//! self-validating, so records are located by scanning for it.
//!
//! Dialog lookup (`Menu_Text("label_play")` in the game's scripts) is only partly decoded: a
//! dialog folder entry is { block(DlgObjID u64), u32 1, u64 name symbol }; the same object id
//! reappears in the text node it leads to, followed by that node's language id. Labels behind a
//! conditional node are not resolved yet and return `None`.

use crate::symbol::crc64;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct LanguageDb {
    pub texts: HashMap<u32, String>,
}

impl LanguageDb {
    pub fn parse(bytes: &[u8]) -> Self {
        let mut texts = HashMap::new();
        let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        let u64_at = |o: usize| bytes.get(o..o + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
        let mut o = 0;
        while o + 12 <= bytes.len() {
            let id = u32_at(o).unwrap();
            if id >= 0x10000 && u64_at(o + 4) == Some(crc64(&format!("{id}.lang"))) {
                if let Some(text) = record_text(bytes, o) {
                    texts.insert(id, text);
                }
            }
            o += 1;
        }
        LanguageDb { texts }
    }

    pub fn get(&self, id: u32) -> Option<&str> {
        self.texts.get(&id).map(String::as_str)
    }
}

fn record_text(b: &[u8], start: usize) -> Option<String> {
    let u32_at = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize);
    let mut p = start + 12 + 4 + 4;
    for _ in 0..3 {
        p += u32_at(p)?;
    }
    p += 8;
    p += u32_at(p)?;
    let _block = u32_at(p)?;
    let len = u32_at(p + 4)?;
    let s = b.get(p + 8..p + 8 + len)?;
    Some(String::from_utf8_lossy(s).into_owned())
}

/// Resolve a dialog label to its language id (direct text nodes only).
pub fn dialog_label(dlog: &[u8], label: &str, db: &LanguageDb) -> Option<u32> {
    let name = crc64(label).to_le_bytes();
    let at = dlog.windows(8).position(|w| w == name)?;
    let obj = dlog.get(at.checked_sub(12)?..at - 4)?;
    let folder_entry = at - 12;
    let mut from = 0;
    while let Some(rel) = dlog[from..].windows(8).position(|w| w == obj) {
        let hit = from + rel;
        from = hit + 1;
        if hit == folder_entry {
            continue;
        }
        let window = &dlog[hit + 8..(hit + 0x180).min(dlog.len())];
        for k in 0..window.len().saturating_sub(4) {
            let id = u32::from_le_bytes(window[k..k + 4].try_into().unwrap());
            if db.texts.contains_key(&id) {
                return Some(id);
            }
        }
    }
    None
}
