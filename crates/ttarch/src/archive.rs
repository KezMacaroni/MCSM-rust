//! `.ttarch2` archives.
//!
//! Outer container (layout as documented by ttarchext, GPL-2.0-or-later, see CREDITS.md):
//! - `ECTT`: chunked, each chunk Blowfish7-encrypted then deflated
//! - `ZCTT`: chunked, deflated only
//! - `NCTT`: u64 size, then the inner stream stored plainly
//! - `3ATT` / `4ATT`: the inner stream with no container
//!
//! Chunked header: magic, u32 chunk size, u32 chunk count, then count+1 u64 file offsets
//! bounding each compressed chunk. A chunk whose stored size equals the chunk size is not
//! compressed. Compressed chunks are zlib or raw deflate.
//!
//! Inner stream (`4ATT`; `3ATT` has one extra u32 after the magic): u32 names size,
//! u32 file count, then per file { u64 name hash, u64 offset, u32 size, u32 unknown,
//! u16 name block, u16 name offset }, then the name table, then file data. Names live at
//! `names[block * 0x10000 + offset]`, NUL-terminated; offsets are relative to the data start.

use crate::blowfish::Blowfish7;
use crate::Error;
use flate2::read::{DeflateDecoder, ZlibDecoder};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const ECTT: u32 = u32::from_le_bytes(*b"ECTT");
const ZCTT: u32 = u32::from_le_bytes(*b"ZCTT");
const NCTT: u32 = u32::from_le_bytes(*b"NCTT");
const ATT3: u32 = u32::from_le_bytes(*b"3ATT");
const ATT4: u32 = u32::from_le_bytes(*b"4ATT");

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub name_hash: u64,
    /// Offset in the inner stream (already includes the data base).
    pub offset: u64,
    pub size: u32,
}

/// The decoded inner stream, served on demand with a one-chunk cache.
struct Stream {
    path: PathBuf,
    file: File,
    kind: StreamKind,
    cache: Option<(usize, Vec<u8>)>,
}

enum StreamKind {
    Plain { base: u64 },
    Chunked { chunk_size: u64, bounds: Vec<u64>, cipher: Option<Blowfish7> },
}

impl Stream {
    fn chunk(&mut self, idx: usize) -> Result<&[u8], Error> {
        if self.cache.as_ref().is_some_and(|(i, _)| *i == idx) {
            return Ok(&self.cache.as_ref().unwrap().1);
        }
        let StreamKind::Chunked { chunk_size, bounds, cipher } = &self.kind else { unreachable!() };
        let (start, end) = (bounds[idx], bounds[idx + 1]);
        let mut raw = vec![0; (end - start) as usize];
        self.file.seek(SeekFrom::Start(start)).map_err(|e| Error::io(&self.path, e))?;
        self.file.read_exact(&mut raw).map_err(|e| Error::io(&self.path, e))?;
        if let Some(bf) = cipher {
            bf.decrypt(&mut raw);
        }
        let out = if raw.len() as u64 == *chunk_size { raw } else { inflate(&raw, *chunk_size as usize)
            .ok_or_else(|| Error::Format(self.path.clone(), format!("chunk {idx} does not inflate")))? };
        self.cache = Some((idx, out));
        Ok(&self.cache.as_ref().unwrap().1)
    }

    fn read_at(&mut self, mut offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        match self.kind {
            StreamKind::Plain { base } => {
                self.file.seek(SeekFrom::Start(base + offset)).map_err(|e| Error::io(&self.path, e))?;
                self.file.read_exact(buf).map_err(|e| Error::io(&self.path, e))
            }
            StreamKind::Chunked { chunk_size, .. } => {
                let mut done = 0;
                while done < buf.len() {
                    let idx = (offset / chunk_size) as usize;
                    let within = (offset % chunk_size) as usize;
                    let path = self.path.clone();
                    let chunk = self.chunk(idx)?;
                    let n = (chunk.len().saturating_sub(within)).min(buf.len() - done);
                    if n == 0 {
                        return Err(Error::Format(path, "read past end of archive".into()));
                    }
                    buf[done..done + n].copy_from_slice(&chunk[within..within + n]);
                    done += n;
                    offset += n as u64;
                }
                Ok(())
            }
        }
    }
}

fn inflate(data: &[u8], cap: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(cap);
    if ZlibDecoder::new(data).read_to_end(&mut out).is_ok() {
        return Some(out);
    }
    out.clear();
    DeflateDecoder::new(data).read_to_end(&mut out).ok().map(|_| out)
}

pub struct Archive {
    path: PathBuf,
    entries: Vec<Entry>,
    by_name: HashMap<String, usize>,
    stream: Mutex<Stream>,
}

impl Archive {
    /// Open an archive. `cipher` is needed only for `ECTT` archives.
    pub fn open(path: &Path, cipher: Option<&Blowfish7>) -> Result<Self, Error> {
        let mut file = File::open(path).map_err(|e| Error::io(path, e))?;
        let mut head = [0u8; 12];
        file.read_exact(&mut head).map_err(|e| Error::io(path, e))?;
        let magic = u32::from_le_bytes(head[0..4].try_into().unwrap());
        let kind = match magic {
            ECTT | ZCTT => {
                let chunk_size = u64::from(u32::from_le_bytes(head[4..8].try_into().unwrap()));
                let count = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
                let mut table = vec![0u8; (count + 1) * 8];
                file.read_exact(&mut table).map_err(|e| Error::io(path, e))?;
                let bounds = table.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
                let cipher = if magic == ECTT {
                    Some(cipher.ok_or_else(|| Error::NeedsKey(path.to_owned()))?.clone())
                } else {
                    None
                };
                StreamKind::Chunked { chunk_size, bounds, cipher }
            }
            NCTT => StreamKind::Plain { base: 12 },
            ATT3 | ATT4 => StreamKind::Plain { base: 0 },
            _ => return Err(Error::Format(path.to_owned(), format!("unknown magic {:?}", &head[0..4]))),
        };
        let mut stream = Stream { path: path.to_owned(), file, kind, cache: None };

        let word = |s: &mut Stream, off: &mut u64| -> Result<u32, Error> {
            let mut b = [0u8; 4];
            s.read_at(*off, &mut b)?;
            *off += 4;
            Ok(u32::from_le_bytes(b))
        };
        let mut off = 0u64;
        let inner = word(&mut stream, &mut off)?;
        match inner {
            ATT4 => {}
            ATT3 => {
                word(&mut stream, &mut off)?;
            }
            _ => return Err(Error::Format(path.to_owned(), format!("inner magic {inner:#010x} is not 3ATT/4ATT"))),
        }
        let names_size = word(&mut stream, &mut off)? as usize;
        let files = word(&mut stream, &mut off)? as usize;

        let mut info = vec![0u8; files * 28];
        stream.read_at(off, &mut info)?;
        off += info.len() as u64;
        let mut names = vec![0u8; names_size];
        stream.read_at(off, &mut names)?;
        let data_base = off + names_size as u64;

        let mut entries = Vec::with_capacity(files);
        for rec in info.chunks_exact(28) {
            let u64_at = |o: usize| u64::from_le_bytes(rec[o..o + 8].try_into().unwrap());
            let u16_at = |o: usize| u16::from_le_bytes(rec[o..o + 2].try_into().unwrap()) as usize;
            let name_at = u16_at(24) * 0x10000 + u16_at(26);
            let raw = names.get(name_at..).ok_or_else(|| Error::Format(path.to_owned(), "name offset out of range".into()))?;
            let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            entries.push(Entry {
                name: String::from_utf8_lossy(&raw[..len]).into_owned(),
                name_hash: u64_at(0),
                offset: data_base + u64_at(8),
                size: u32::from_le_bytes(rec[16..20].try_into().unwrap()),
            });
        }
        let by_name = entries.iter().enumerate().map(|(i, e)| (e.name.to_ascii_lowercase(), i)).collect();
        Ok(Archive { path: path.to_owned(), entries, by_name, stream: Mutex::new(stream) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Case-insensitive lookup, matching how the engine resolves resource names.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(&name.to_ascii_lowercase()).map(|&i| &self.entries[i])
    }

    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>, Error> {
        let mut buf = vec![0; entry.size as usize];
        self.stream.lock().unwrap().read_at(entry.offset, &mut buf)?;
        Ok(buf)
    }
}
