//! Telltale-encrypted Lua scripts.
//!
//! `\x1bLEn`: the 4-byte tag is replaced by the standard `\x1bLua` signature and the rest is
//! Blowfish7-decrypted (whole 8-byte blocks; the tail is plain). `\x1bLEo` is the same cipher
//! over everything after the tag, with no signature restored. Layout per TTG Tools
//! (`Methods.decryptLua`, GPL-3.0, see CREDITS.md). Plain `\x1bLua` passes through.

use crate::Blowfish7;

pub const LUA_SIGNATURE: &[u8; 4] = b"\x1bLua";

/// Decrypt a script to standard Lua 5.2 bytecode (or source, if it was never compiled).
pub fn decrypt(cipher: &Blowfish7, bytes: &[u8]) -> Vec<u8> {
    match bytes.get(..4) {
        Some(b"\x1bLEn") => {
            let mut out = bytes.to_vec();
            out[..4].copy_from_slice(LUA_SIGNATURE);
            cipher.decrypt(&mut out[4..]);
            out
        }
        Some(b"\x1bLEo") => {
            let mut out = bytes[4..].to_vec();
            cipher.decrypt(&mut out);
            out
        }
        _ => bytes.to_vec(),
    }
}

/// True when `bytes` is a Lua 5.2 binary chunk header.
pub fn is_lua52_bytecode(bytes: &[u8]) -> bool {
    bytes.len() >= 5 && &bytes[..4] == LUA_SIGNATURE && bytes[4] == 0x52
}
