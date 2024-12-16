mod asset_index;
mod assets;
mod deserialize;
mod loaded_chunks;
mod serialize;

use platform_abstraction_layer::{Pal, PixelFormat};

use crate::{linear_allocator::Pool, LinearAllocator};

pub use asset_index::{AssetIndex, AssetIndexHeader};
pub use assets::{
    AudioClipAsset, ChunkDescriptor, ChunkRegion, TextureAsset, TextureChunkDescriptor,
};
pub use deserialize::{deserialize, Deserialize};
pub use loaded_chunks::{LoadedChunk, LoadedTextureChunk};
pub use serialize::{serialize, Serialize};

/// Magic number to store (and assert) at the start of a resource database file.
/// Serialize/deserialize using the respective modules (which store this as
/// little-endian).
pub const RESOURCE_DB_MAGIC_NUMBER: u32 = 0xE97E6D00;
/// Amount of bytes in the regular dynamically allocated chunks. See
/// [`LoadedChunk`].
pub const CHUNK_SIZE: usize = 64 * 1024;
/// Width and height of the dynamically allocated texture chunks. See
/// [`LoadedTextureChunk`].
pub const TEXTURE_CHUNK_DIMENSIONS: (u16, u16) = (128, 128);
/// Pixel format of the dynamically allocated texture chunks. See
/// [`LoadedTextureChunk`].
pub const TEXTURE_CHUNK_FORMAT: PixelFormat = PixelFormat::Rgba;

pub struct Resources<'eng> {
    loaded_chunks: Pool<'eng, LoadedChunk>,
    loaded_texture_chunks: Pool<'eng, LoadedTextureChunk>,
    asset_index: AssetIndex<'eng>,
}

impl<'eng> Resources<'eng> {
    pub fn new(
        platform: &'eng dyn Pal,
        allocator: &'eng LinearAllocator,
        temp_allocator: &LinearAllocator,
    ) -> Option<Resources<'eng>> {
        let db_file = platform.open_file("resources.db")?;
        let asset_index = AssetIndex::new(platform, allocator, temp_allocator, db_file)?;

        Some(Resources {
            loaded_chunks: Pool::new(allocator, asset_index.chunks.len())?,
            loaded_texture_chunks: Pool::new(allocator, asset_index.texture_chunks.len())?,
            asset_index,
        })
    }
}
