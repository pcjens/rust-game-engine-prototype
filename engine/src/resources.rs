mod asset_index;
mod assets;
mod loaded_chunks;

use platform_abstraction_layer::{Pal, PixelFormat};

use crate::{linear_allocator::Pool, LinearAllocator};

pub use asset_index::{AssetIndex, AssetIndexHeader};
pub use assets::{
    AudioClipAsset, ChunkDescriptor, ChunkRegion, TextureAsset, TextureChunkDescriptor,
};
pub use loaded_chunks::{LoadedChunk, LoadedTextureChunk};

pub const CHUNK_SIZE: usize = 64 * 1024;
pub const TEXTURE_CHUNK_DIMENSIONS: (u16, u16) = (128, 128);
pub const TEXTURE_CHUNK_FORMAT: PixelFormat = PixelFormat::Rgba;

type Chunksize = u32;

pub struct Resources<'eng> {
    loaded_chunks: Pool<'eng, LoadedChunk>,
    loaded_texture_chunks: Pool<'eng, LoadedTextureChunk>,
    asset_index: AssetIndex<'eng>,
}

impl<'eng> Resources<'eng> {
    pub fn new(
        platform: &'eng dyn Pal,
        allocator: &'eng LinearAllocator,
    ) -> Option<Resources<'eng>> {
        // TODO: actually reading this from a file or something pretending to be one
        let mut test_index = AssetIndex::new(
            allocator,
            AssetIndexHeader {
                textures: 1,
                audio_clips: 0,
                chunks: 0,
                texture_chunks: 1,
            },
        )?;

        test_index
            .texture_chunks
            .push(TextureChunkDescriptor {
                region_width: 2,
                region_height: 2,
                resident: None,
            })
            .unwrap();

        test_index
            .textures
            .push(TextureAsset {
                width: 2,
                height: 2,
                texture_chunks: 0..1,
            })
            .unwrap();

        // TODO: set up the chunk reading thing
        // (the idea is that now after reading all the metadata, we could save
        //  the "cursor" here and seek from there to any chunk index to read a
        //  chunk's data)

        Some(Resources {
            loaded_chunks: Pool::new(allocator)?,
            loaded_texture_chunks: Pool::new(allocator)?,
            asset_index: test_index,
        })
    }
}
