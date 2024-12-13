use core::ops::Range;

use crate::PoolBox;

use super::{Chunksize, LoadedChunk, LoadedTextureChunk};

#[derive(Debug)]
pub struct AudioClipAsset {
    pub samples_per_second: u32,
    pub samples: u32,
    pub chunks: Range<Chunksize>,
}

#[derive(Debug)]
pub struct TextureAsset {
    /// The width of the whole texture.
    pub width: u16,
    /// The height of the whole texture.
    pub height: u16,
    /// The chunks the texture is made up of. Multi-chunk textures are allocated
    /// starting from the top-left of the texture, row-major.
    pub texture_chunks: Range<Chunksize>,
}

#[derive(Debug)]
pub enum ChunkRegion {
    AudioClip {
        start_sample_index: u32,
        samples: u32,
    },
}

#[derive(Debug)]
pub struct ChunkDescriptor<'eng> {
    /// The region of a resource the chunk contains (e.g. a timespan of an
    /// audio clip).
    pub region: ChunkRegion,
    /// A reference to the allocated chunk, if it is currently loaded.
    pub resident: Option<PoolBox<'eng, LoadedChunk>>,
}

#[derive(Debug)]
pub struct TextureChunkDescriptor<'eng> {
    /// The width of the texture the chunk contains.
    pub region_width: u16,
    /// The height of the texture the chunk contains.
    pub region_height: u16,
    /// A reference to the allocated chunk, if it is currently loaded.
    pub resident: Option<PoolBox<'eng, LoadedTextureChunk>>,
}
