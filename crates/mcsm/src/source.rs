//! The `ttarch://` asset source: every file inside the user's mounted `.ttarch2` archives,
//! addressed by its archive-internal name (`ttarch://ui_menu_main.scene`).

use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetSourceBuilder, PathStream, Reader, VecReader,
};
use bevy::prelude::*;
use std::path::Path;
use std::sync::Arc;
use ttarch::install::Vfs;

pub const SOURCE: &str = "ttarch";

#[derive(Resource, Clone)]
pub struct Mounted(pub Arc<Vfs>);

struct TtarchReader(Arc<Vfs>);

impl TtarchReader {
    fn bytes(&self, path: &Path) -> Result<Vec<u8>, AssetReaderError> {
        let name = path.to_string_lossy();
        match self.0.read(&name) {
            Some(Ok(bytes)) => Ok(bytes),
            Some(Err(e)) => Err(AssetReaderError::Io(Arc::new(std::io::Error::other(e.to_string())))),
            None => Err(AssetReaderError::NotFound(path.to_owned())),
        }
    }
}

impl AssetReader for TtarchReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.bytes(path).map(VecReader::new)
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        // Archives carry no Bevy .meta files.
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_owned()))
    }

    async fn read_directory<'a>(&'a self, path: &'a Path) -> Result<Box<PathStream>, AssetReaderError> {
        // Archive namespaces are flat; there are no directories to list.
        Err(AssetReaderError::NotFound(path.to_owned()))
    }

    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}

/// Register the source. Must run before `DefaultPlugins` (Bevy builds sources in `AssetPlugin`).
pub fn register(app: &mut App, vfs: Arc<Vfs>) {
    let for_reader = vfs.clone();
    app.register_asset_source(
        SOURCE,
        AssetSourceBuilder::new(move || Box::new(TtarchReader(for_reader.clone()))),
    );
    app.insert_resource(Mounted(vfs));
}
