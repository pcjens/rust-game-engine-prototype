mod asset_index;
mod assets;
mod loaded_chunks;

use platform_abstraction_layer::{Pal, PixelFormat};

use crate::{linear_allocator::Pool, LinearAllocator};

pub use asset_index::{AssetIndex, AssetIndexHeader};
pub use assets::{AudioClipAsset, ChunkDescriptor, ChunkRegion, LiveChunk, TextureAsset};
pub use loaded_chunks::{LoadedChunk, LoadedTextureChunk};

pub const CHUNK_SIZE: usize = 64 * 1024;
pub const TEXTURE_CHUNK_DIMENSIONS: (u16, u16) = (128, 128);
pub const TEXTURE_CHUNK_FORMAT: PixelFormat = PixelFormat::Rgba;

type Chunksize = u32;

pub struct Resources<'pl, 're> {
    platform: &'pl dyn Pal,
    loaded_chunks: Pool<'re, LoadedChunk>,
    loaded_texture_chunks: Pool<'re, LoadedTextureChunk>,
}

impl<'pl, 're> Resources<'pl, 're> {
    pub fn new(
        platform: &'pl dyn Pal,
        allocator: &'re LinearAllocator,
    ) -> Option<Resources<'pl, 're>> {
        // TODO: actually reading this from a file or something pretending to be one
        let mut test_index = AssetIndex::new(
            allocator,
            AssetIndexHeader {
                textures: 1,
                audio_clips: 0,
                chunks: 1,
            },
        )?;

        test_index
            .chunks
            .push(ChunkDescriptor {
                region: ChunkRegion::Texture {
                    chunk_width: 2,
                    chunk_height: 2,
                },
                live: LiveChunk::Unloaded,
            })
            .unwrap();

        test_index
            .textures
            .push(TextureAsset {
                width: 2,
                height: 2,
                chunks: 0..1,
            })
            .unwrap();

        // TODO: read the chunks
        // (the idea is that now after reading all the metadata, we could save
        //  the "cursor" here and seek from there to any chunk index to read a
        //  chunk's data)

        Some(Resources {
            platform,
            loaded_chunks: Pool::new(allocator)?,
            loaded_texture_chunks: Pool::new(allocator)?,
        })
    }
}
