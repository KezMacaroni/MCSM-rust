//! Pull the archive key and Blowfish tables out of the user's MinecraftStoryMode.exe.
//!
//! Layout observed in the executable's read-only data: the four 256-entry S-boxes,
//! immediately followed by the 18-entry P-array, immediately followed by the game key
//! as a NUL-terminated byte string. The P-array and S-boxes start with the well-known
//! Blowfish constants (digits of pi), which is how they are found; nothing build-specific
//! is hardcoded. A second, unrelated Blowfish (statically linked OpenSSL) stores P before S,
//! so it never matches this pattern.

use crate::blowfish::Tables;
use crate::Error;
use std::path::Path;

const P0: [u8; 4] = 0x243F_6A88u32.to_le_bytes();
const S0_0: [u8; 4] = 0xD131_0BA6u32.to_le_bytes();
const S_LEN: usize = 4 * 256 * 4;
const P_LEN: usize = 18 * 4;
const MAX_KEY: usize = 56;

pub fn tables_from_exe(path: &Path) -> Result<Tables, Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    tables_from_bytes(&bytes).ok_or_else(|| Error::KeyNotFound(path.to_owned()))
}

pub fn tables_from_bytes(bytes: &[u8]) -> Option<Tables> {
    let mut at = 0;
    while let Some(rel) = find(&bytes[at..], &S0_0) {
        let s_off = at + rel;
        at = s_off + 1;
        let p_off = s_off + S_LEN;
        if bytes.get(p_off..p_off + 4) != Some(&P0[..]) {
            continue;
        }
        let key_off = p_off + P_LEN;
        let tail = bytes.get(key_off..(key_off + MAX_KEY + 1).min(bytes.len()))?;
        let key_len = tail.iter().position(|&b| b == 0)?;
        if key_len == 0 {
            continue;
        }
        let word = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let s = std::array::from_fn(|b| std::array::from_fn(|i| word(s_off + (b * 256 + i) * 4)));
        let p = std::array::from_fn(|i| word(p_off + i * 4));
        return Some(Tables { p, s, key: tail[..key_len].to_vec() });
    }
    None
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
