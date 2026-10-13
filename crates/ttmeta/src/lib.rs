//! Readers for Telltale Tool MetaStream (`5VSM`) objects as used by Minecraft: Story Mode (PC).
//! Everything here is format knowledge; no game data is embedded.

pub mod anim;
pub mod chore;
pub mod dialog;
pub mod font;
pub mod mesh;
pub mod props;
pub mod scene;
pub mod skeleton;
pub mod stream;
pub mod symbol;
pub mod text;
pub mod texture;

pub use anim::Animation;
pub use chore::Chore;
pub use font::Font;
pub use mesh::Mesh;
pub use props::{PropertySet, Value};
pub use scene::Scene;
pub use skeleton::Skeleton;
pub use symbol::{crc64, Names, Symbol};
pub use texture::Texture;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Format(String),
    /// Well-formed data using a feature this crate does not decode yet.
    #[error("unsupported: {0}")]
    Unsupported(String),
}
