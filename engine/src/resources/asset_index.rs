use crate::{FixedVec, LinearAllocator};

use super::{AudioClipAsset, ChunkDescriptor, Chunksize, TextureAsset};

pub struct AssetIndexHeader {
    pub textures: Chunksize,
    pub audio_clips: Chunksize,
    pub chunks: Chunksize,
}

pub struct AssetIndex<'re> {
    pub textures: FixedVec<'re, TextureAsset>,
    pub audio_clips: FixedVec<'re, AudioClipAsset>,
    /// Descriptors for every loadable chunk. The layout of this array matches
    /// the actual chunks in the database file.
    pub chunks: FixedVec<'re, ChunkDescriptor<'re>>,
}

impl AssetIndex<'_> {
    pub fn new<'re>(
        alloc: &'re LinearAllocator,
        header: AssetIndexHeader,
    ) -> Option<AssetIndex<'re>> {
        Some(AssetIndex {
            textures: FixedVec::new(alloc, header.textures as usize)?,
            audio_clips: FixedVec::new(alloc, header.audio_clips as usize)?,
            chunks: FixedVec::new(alloc, header.chunks as usize)?,
        })
    }
}
