pub mod asset_index;
mod assets;
mod chunks;
mod deserialize;
mod serialize;

use platform_abstraction_layer::PixelFormat;

pub use assets::{AudioClipAsset, TextureAsset};
pub use chunks::{
    ChunkDescriptor, ChunkRegion, LoadedChunk, LoadedTextureChunk, TextureChunkDescriptor,
};
pub use deserialize::{deserialize, Deserialize};
pub use serialize::{serialize, Serialize};

/// Magic number to store (and assert) at the start of a resource database file.
/// Serialize/deserialize using the respective modules (which store this as
/// little-endian).
pub const RESOURCE_DB_MAGIC_NUMBER: u32 = 0xE97E6D00;
/// Amount of bytes in the regular dynamically allocated chunks. See
/// [`LoadedChunk`].
pub const CHUNK_SIZE: u32 = 64 * 1024;
/// Width and height of the dynamically allocated texture chunks. See
/// [`LoadedTextureChunk`].
pub const TEXTURE_CHUNK_DIMENSIONS: (u16, u16) = (128, 128);
/// Pixel format of the dynamically allocated texture chunks. See
/// [`LoadedTextureChunk`].
pub const TEXTURE_CHUNK_FORMAT: PixelFormat = PixelFormat::Rgba;
