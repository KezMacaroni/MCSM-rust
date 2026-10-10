//! Telltale's modified Blowfish ("version 7").
//!
//! The key schedule is standard Blowfish. Decryption differs in two ways, both taken from
//! Luigi Auriemma's ttarchext (`blowfish_ttarch.c`, GPL-2.0-or-later, see CREDITS.md):
//! P[1]..P[4] are visited in a permuted order, and the final whitening uses P[3] instead of P[1].
//! The third difference, S-box 0 entry 118 being byte-swapped, is already baked into the tables
//! inside MinecraftStoryMode.exe, so the tables are used exactly as read from the exe.
//!
//! Words are little-endian; a trailing partial block (len % 8) is left untouched.

/// Raw Blowfish tables plus the game key, as read from the executable.
#[derive(Clone)]
pub struct Tables {
    pub p: [u32; 18],
    pub s: [[u32; 256]; 4],
    pub key: Vec<u8>,
}

/// A keyed cipher context.
#[derive(Clone)]
pub struct Blowfish7 {
    p: [u32; 18],
    s: [[u32; 256]; 4],
}

impl Blowfish7 {
    pub fn new(tables: &Tables) -> Self {
        let mut bf = Blowfish7 { p: tables.p, s: tables.s };
        let key = &tables.key;
        let mut j = 0;
        for i in 0..18 {
            let mut data = 0u32;
            for _ in 0..4 {
                data = (data << 8) | u32::from(key[j]);
                j = (j + 1) % key.len();
            }
            bf.p[i] ^= data;
        }
        let (mut l, mut r) = (0u32, 0u32);
        for i in (0..18).step_by(2) {
            (l, r) = bf.encipher(l, r);
            bf.p[i] = l;
            bf.p[i + 1] = r;
        }
        for b in 0..4 {
            for i in (0..256).step_by(2) {
                (l, r) = bf.encipher(l, r);
                bf.s[b][i] = l;
                bf.s[b][i + 1] = r;
            }
        }
        bf
    }

    #[inline]
    fn f(&self, x: u32) -> u32 {
        let [a, b, c, d] = x.to_be_bytes();
        (self.s[0][a as usize].wrapping_add(self.s[1][b as usize]) ^ self.s[2][c as usize])
            .wrapping_add(self.s[3][d as usize])
    }

    /// Standard encipher; only the key schedule uses it.
    fn encipher(&self, mut l: u32, mut r: u32) -> (u32, u32) {
        for i in 0..16 {
            l ^= self.p[i];
            r ^= self.f(l);
            (l, r) = (r, l);
        }
        // undo the last swap, then whiten
        (r ^ self.p[17], l ^ self.p[16])
    }

    fn decipher7(&self, mut l: u32, mut r: u32) -> (u32, u32) {
        for i in (2..=17).rev() {
            let k = match i {
                4 => self.p[2],
                3 => self.p[1],
                2 => self.p[4],
                _ => self.p[i],
            };
            l ^= k;
            r ^= self.f(l);
            (l, r) = (r, l);
        }
        (r ^ self.p[0], l ^ self.p[3])
    }

    fn encipher7(&self, mut l: u32, mut r: u32) -> (u32, u32) {
        for i in 0..16 {
            let k = match i {
                1 => self.p[3],
                2 => self.p[4],
                3 => self.p[1],
                4 => self.p[2],
                _ => self.p[i],
            };
            l ^= k;
            r ^= self.f(l);
            (l, r) = (r, l);
        }
        // undo the last swap, then whiten
        (r ^ self.p[17], l ^ self.p[16])
    }

    /// Decrypt whole 8-byte blocks in place.
    pub fn decrypt(&self, buf: &mut [u8]) {
        for block in buf.chunks_exact_mut(8) {
            let l = u32::from_le_bytes(block[0..4].try_into().unwrap());
            let r = u32::from_le_bytes(block[4..8].try_into().unwrap());
            let (l, r) = self.decipher7(l, r);
            block[0..4].copy_from_slice(&l.to_le_bytes());
            block[4..8].copy_from_slice(&r.to_le_bytes());
        }
    }

    /// Encrypt whole 8-byte blocks in place (inverse of [`Self::decrypt`]).
    pub fn encrypt(&self, buf: &mut [u8]) {
        for block in buf.chunks_exact_mut(8) {
            let l = u32::from_le_bytes(block[0..4].try_into().unwrap());
            let r = u32::from_le_bytes(block[4..8].try_into().unwrap());
            let (l, r) = self.encipher7(l, r);
            block[0..4].copy_from_slice(&l.to_le_bytes());
            block[4..8].copy_from_slice(&r.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_tables() -> Tables {
        // Deterministic non-zero tables; the real ones only ever come from the user's exe.
        let mut x = 0x1234_5678u32;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let p = std::array::from_fn(|_| next());
        let s = std::array::from_fn(|_| std::array::from_fn(|_| next()));
        Tables { p, s, key: b"not the game key".to_vec() }
    }

    #[test]
    fn roundtrip_leaves_tail_untouched() {
        let bf = Blowfish7::new(&dummy_tables());
        let original: Vec<u8> = (0..37u8).collect();
        let mut buf = original.clone();
        bf.encrypt(&mut buf);
        assert_ne!(buf[..32], original[..32]);
        assert_eq!(buf[32..], original[32..]);
        bf.decrypt(&mut buf);
        assert_eq!(buf, original);
    }
}
