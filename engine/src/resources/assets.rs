use core::ops::Range;

use crate::PoolBox;

use super::{Chunksize, LoadedChunk, LoadedTextureChunk};

#[derive(Debug)]
pub struct TextureAsset {
    pub width: u16,
    pub height: u16,
    pub chunks: Range<Chunksize>,
}

#[derive(Debug)]
pub struct AudioClipAsset {
    pub samples_per_second: u32,
    pub samples: u32,
    pub chunks: Range<Chunksize>,
}

#[derive(Debug)]
pub enum ChunkRegion {
    AudioClip {
        start_sample_index: u32,
        samples: u32,
    },
    Texture {
        chunk_width: u16,
        chunk_height: u16,
    },
}

#[derive(Debug)]
pub struct ChunkDescriptor<'re> {
    /// The region of a resource the chunk contains (i.e. a timespan of an
    /// audio clip, or an area of a texture).
    pub region: ChunkRegion,
    /// A reference to the allocated chunk, if it is currently loaded.
    pub live: LiveChunk<'re>,
}

#[derive(Debug)]
pub enum LiveChunk<'re> {
    Unloaded,
    Regular(PoolBox<'re, LoadedChunk>),
    Texture(PoolBox<'re, LoadedTextureChunk>),
}
