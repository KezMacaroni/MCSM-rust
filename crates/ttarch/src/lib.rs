//! Read Telltale Tool `.ttarch2` archives from a user's own Minecraft: Story Mode install.
//!
//! Nothing game-specific ships with this crate: the Blowfish tables and archive key are read
//! from the user's `MinecraftStoryMode.exe` at runtime (see [`exe`]).

pub mod archive;
pub mod blowfish;
pub mod exe;
pub mod install;
pub mod resdesc;
pub mod script;

pub use archive::{Archive, Entry};
pub use blowfish::{Blowfish7, Tables};
pub use install::Install;

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}: no Telltale Blowfish key found (is this MinecraftStoryMode.exe?)")]
    KeyNotFound(PathBuf),
    #[error("{0}: encrypted archive opened without a key")]
    NeedsKey(PathBuf),
    #[error("{0}: {1}")]
    Format(PathBuf, String),
    #[error("{0}")]
    Config(String),
}

impl Error {
    fn io(path: &Path, e: std::io::Error) -> Self {
        Error::Io(path.to_owned(), e)
    }
}
