//! Locating the user's game install and mounting its archives.

use crate::{exe, resdesc, Archive, Blowfish7, Entry, Error};
use std::path::{Path, PathBuf};

pub const EXE_NAME: &str = "MinecraftStoryMode.exe";
pub const ENV_VAR: &str = "MCSM_GAME_DIR";
pub const CONFIG_FILE: &str = "mcsm.toml";

pub struct Install {
    pub root: PathBuf,
    pub exe: PathBuf,
    pub cipher: Blowfish7,
}

impl Install {
    /// Resolve the install from, in order: `explicit`, `$MCSM_GAME_DIR`, `game_dir = "..."` in
    /// `./mcsm.toml`. Each may name the game folder or the exe itself.
    pub fn locate(explicit: Option<&Path>) -> Result<Self, Error> {
        let given = match explicit {
            Some(p) => p.to_owned(),
            None => std::env::var_os(ENV_VAR)
                .map(PathBuf::from)
                .or_else(config_game_dir)
                .ok_or_else(|| {
                    Error::Config(format!(
                        "game install not configured: pass a path, set {ENV_VAR}, or put game_dir = \"...\" in {CONFIG_FILE}"
                    ))
                })?,
        };
        Self::open(&given)
    }

    pub fn open(path: &Path) -> Result<Self, Error> {
        let (root, exe) = if path.is_file() {
            (path.parent().unwrap_or(Path::new(".")).to_owned(), path.to_owned())
        } else {
            (path.to_owned(), path.join(EXE_NAME))
        };
        if !exe.is_file() {
            return Err(Error::Config(format!("{} not found", exe.display())));
        }
        let tables = exe::tables_from_exe(&exe)?;
        Ok(Install { root, exe, cipher: Blowfish7::new(&tables) })
    }

    pub fn archives_dir(&self) -> PathBuf {
        self.root.join("Archives")
    }

    /// All `.ttarch2` files, sorted by name for a stable mount order.
    pub fn archive_paths(&self) -> Result<Vec<PathBuf>, Error> {
        let dir = self.archives_dir();
        let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| Error::Io(dir.clone(), e))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ttarch2")))
            .collect();
        out.sort();
        Ok(out)
    }

    pub fn open_archive(&self, path: &Path) -> Result<Archive, Error> {
        Archive::open(path, Some(&self.cipher))
    }

    /// Archives in lookup order: as the engine's resource sets order them (highest priority
    /// first, language packs excluded), or every archive by name if there are no set files.
    pub fn mount_order(&self) -> Result<Vec<PathBuf>, Error> {
        let dir = self.archives_dir();
        let sets = resdesc::load(&dir, &self.cipher)?;
        if sets.is_empty() {
            return self.archive_paths();
        }
        Ok(resdesc::archive_order(&dir, &sets))
    }

    /// Mount the game's archives in lookup order. Failures are returned alongside, not fatal.
    pub fn mount(&self) -> Result<Vfs, Error> {
        let mut archives = Vec::new();
        let mut errors = Vec::new();
        for p in self.mount_order()? {
            match self.open_archive(&p) {
                Ok(a) => archives.push(a),
                Err(e) => errors.push(e),
            }
        }
        Ok(Vfs { archives, errors })
    }
}

/// Every mounted archive, searched in mount order: the first archive containing a name wins,
/// so patch sets override the base game.
pub struct Vfs {
    pub archives: Vec<Archive>,
    pub errors: Vec<Error>,
}

impl Vfs {
    pub fn find(&self, name: &str) -> Option<(&Archive, &Entry)> {
        self.archives.iter().find_map(|a| a.find(name).map(|e| (a, e)))
    }

    pub fn read(&self, name: &str) -> Option<Result<Vec<u8>, Error>> {
        self.find(name).map(|(a, e)| a.read(e))
    }

    pub fn file_count(&self) -> usize {
        self.archives.iter().map(|a| a.entries().len()).sum()
    }
}

/// `game_dir` from `mcsm.toml` in the working directory, or else next to the running exe
/// (shortcuts may start it elsewhere).
fn config_game_dir() -> Option<PathBuf> {
    let beside_exe = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join(CONFIG_FILE)));
    [Some(PathBuf::from(CONFIG_FILE)), beside_exe].into_iter().flatten().find_map(|path| {
        let text = std::fs::read_to_string(path).ok()?;
        text.lines().filter(|l| !l.trim_start().starts_with('#')).find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim() == "game_dir").then(|| PathBuf::from(v.trim().trim_matches(['"', '\''])))
        })
    })
}
