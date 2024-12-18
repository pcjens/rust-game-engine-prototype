use core::ops::Range;

use crate::PoolBox;

use super::{LoadedChunk, LoadedTextureChunk};

pub const CHUNK_REGION_AUDIO_CLIP_TAG: u8 = 0;

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
    /// The range of bytes in the chunk data portion of the database this
    /// texture chunk can be loaded from.
    pub source_bytes: Range<u64>,
    /// A reference to the allocated chunk, if it is currently loaded.
    pub resident: Option<PoolBox<'eng, 'eng, LoadedChunk>>,
}

#[derive(Debug)]
pub struct TextureChunkDescriptor<'eng> {
    /// The width of the texture the chunk contains.
    pub region_width: u16,
    /// The height of the texture the chunk contains.
    pub region_height: u16,
    /// The range of bytes in the chunk data portion of the database this
    /// texture chunk can be loaded from.
    pub source_bytes: Range<u64>,
    /// A reference to the allocated chunk, if it is currently loaded.
    pub resident: Option<PoolBox<'eng, 'eng, LoadedTextureChunk>>,
}
