//! MetaStream container (`5VSM`) and the primitive reader everything else is built on.
//!
//! Header: magic, u32 default-section size, u32 debug-section size, u32 async-section size,
//! u32 type count, then per type { u64 type symbol, u32 version crc }. The default section holds
//! the object; the async section holds bulk data (texture pixels). Serialization conventions:
//! bools are the bytes `'0'`/`'1'`; non-intrinsic members are wrapped in a block whose leading
//! u32 is the block's total size including itself.

use crate::symbol::Symbol;
use crate::Error;

pub struct MetaStream<'a> {
    pub types: Vec<(Symbol, u32)>,
    pub data: &'a [u8],
    pub async_data: &'a [u8],
}

impl<'a> MetaStream<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.get(..4) != Some(b"5VSM") {
            return Err(Error::Format(format!("expected 5VSM, found {:?}", bytes.get(..4))));
        }
        let mut r = Reader::new(bytes);
        r.skip(4)?;
        let (data_len, debug_len, async_len, count) = (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize, r.u32()?);
        let mut types = Vec::with_capacity(count as usize);
        for _ in 0..count {
            types.push((r.symbol()?, r.u32()?));
        }
        let start = r.pos;
        let data = bytes.get(start..start + data_len).ok_or_else(|| Error::Format("default section truncated".into()))?;
        let async_start = start + data_len + debug_len;
        let async_data =
            bytes.get(async_start..async_start + async_len).ok_or_else(|| Error::Format("async section truncated".into()))?;
        Ok(MetaStream { types, data, async_data })
    }

    pub fn has_type(&self, t: Symbol) -> bool {
        self.types.iter().any(|(s, _)| *s == t)
    }

    pub fn reader(&self) -> Reader<'a> {
        Reader::new(self.data)
    }
}

#[derive(Clone)]
pub struct Reader<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    pub fn at(buf: &'a [u8], pos: usize) -> Self {
        Reader { buf, pos }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let s = self
            .buf
            .get(self.pos..self.pos + n)
            .ok_or_else(|| Error::Format(format!("read of {n} bytes at {:#x} past end ({:#x})", self.pos, self.buf.len())))?;
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<(), Error> {
        self.bytes(n).map(|_| ())
    }

    pub fn seek(&mut self, pos: usize) -> Result<(), Error> {
        if pos > self.buf.len() {
            return Err(Error::Format(format!("seek to {pos:#x} past end ({:#x})", self.buf.len())));
        }
        self.pos = pos;
        Ok(())
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }

    pub fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn vec<const N: usize>(&mut self) -> Result<[f32; N], Error> {
        let mut v = [0.0; N];
        for x in &mut v {
            *x = self.f32()?;
        }
        Ok(v)
    }

    pub fn symbol(&mut self) -> Result<Symbol, Error> {
        Ok(Symbol(self.u64()?))
    }

    /// Telltale bool: the ASCII byte `'0'` or `'1'`.
    pub fn bool(&mut self) -> Result<bool, Error> {
        match self.u8()? {
            b'0' => Ok(false),
            b'1' => Ok(true),
            b => Err(Error::Format(format!("bad bool byte {b:#04x} at {:#x}", self.pos - 1))),
        }
    }

    /// u32 length + bytes.
    pub fn string(&mut self) -> Result<String, Error> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }

    /// Read a block's size prefix and return the absolute end offset.
    pub fn block(&mut self) -> Result<usize, Error> {
        let start = self.pos;
        let size = self.u32()? as usize;
        if size < 4 || start + size > self.buf.len() {
            return Err(Error::Format(format!("bad block size {size} at {start:#x}")));
        }
        Ok(start + size)
    }

    pub fn skip_block(&mut self) -> Result<(), Error> {
        let end = self.block()?;
        self.seek(end)
    }

    /// A string wrapped in a block (how strings appear as struct members).
    pub fn block_string(&mut self) -> Result<String, Error> {
        let end = self.block()?;
        let s = self.string()?;
        self.seek(end)?;
        Ok(s)
    }
}
