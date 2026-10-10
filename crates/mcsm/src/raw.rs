//! Placeholder loader: any Telltale file as raw bytes. Real per-format loaders
//! (`.d3dtx`, `.d3dmesh`, `.prop`, ...) replace entries here as they are written.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;

#[derive(Asset, TypePath, Debug)]
pub struct RawFile {
    pub bytes: Vec<u8>,
}

#[derive(Default, TypePath)]
struct RawFileLoader;

#[derive(Debug, thiserror::Error)]
#[error("could not read file: {0}")]
struct RawFileError(#[from] std::io::Error);

impl AssetLoader for RawFileLoader {
    type Asset = RawFile;
    type Settings = ();
    type Error = RawFileError;

    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<RawFile, RawFileError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(RawFile { bytes })
    }

    fn extensions(&self) -> &[&str] {
        &["prop", "scene", "chore", "dlog", "landb", "lua", "skl", "anm", "d3dmesh", "d3dtx", "font", "bank"]
    }
}

pub struct RawFilePlugin;

impl Plugin for RawFilePlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<RawFile>().init_asset_loader::<RawFileLoader>();
    }
}
