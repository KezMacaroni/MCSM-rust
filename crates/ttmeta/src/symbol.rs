//! Telltale symbols: CRC-64/ECMA-182 (non-reflected, zero init, no final xor) of the lowercase name.
//! Type names, property keys and resource handles are all stored this way.

use std::collections::HashMap;
use std::fmt;

const POLY: u64 = 0x42F0_E1EB_A9EA_3693;

const TABLE: [u64; 256] = {
    let mut t = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u64) << 56;
        let mut k = 0;
        while k < 8 {
            c = if c >> 63 != 0 { (c << 1) ^ POLY } else { c << 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

/// Hash of `name`, lowercased (ASCII). Usable in `const` contexts.
pub const fn crc64(name: &str) -> u64 {
    let b = name.as_bytes();
    let mut c = 0u64;
    let mut i = 0;
    while i < b.len() {
        let x = b[i].to_ascii_lowercase();
        c = TABLE[(((c >> 56) as u8) ^ x) as usize] ^ (c << 8);
        i += 1;
    }
    c
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Symbol(pub u64);

impl Symbol {
    pub const NONE: Symbol = Symbol(0);

    pub const fn of(name: &str) -> Symbol {
        Symbol(crc64(name))
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:016x}", self.0)
    }
}

/// Reverse lookup for symbols whose names are known (file names, property keys used in code).
#[derive(Default, Clone)]
pub struct Names(HashMap<u64, String>);

impl Names {
    pub fn add(&mut self, name: &str) {
        self.0.entry(crc64(name)).or_insert_with(|| name.to_owned());
    }

    pub fn get(&self, s: Symbol) -> Option<&str> {
        self.0.get(&s.0).map(String::as_str)
    }

    pub fn show(&self, s: Symbol) -> String {
        self.get(s).map_or_else(|| format!("{s:?}"), str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_type_hashes() {
        // Values observed in MetaStream headers.
        assert_eq!(crc64("T3Texture"), 0x3e24_9e7e_6f38_cce2);
        assert_eq!(crc64("int32"), 0x7cac_eebc_d26d_075c);
        assert_eq!(crc64("bool"), 0x9004_c558_7575_d6c0);
    }
}
