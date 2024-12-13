use crate::{FixedVec, LinearAllocator};

use super::{
    assets::TextureChunkDescriptor, AudioClipAsset, ChunkDescriptor, Chunksize, TextureAsset,
};

#[derive(Clone, Copy)]
pub struct AssetIndexHeader {
    pub textures: Chunksize,
    pub audio_clips: Chunksize,
    pub chunks: Chunksize,
    pub texture_chunks: Chunksize,
}

pub struct AssetIndex<'eng> {
    pub textures: FixedVec<'eng, TextureAsset>,
    pub audio_clips: FixedVec<'eng, AudioClipAsset>,
    pub chunks: FixedVec<'eng, ChunkDescriptor<'eng>>,
    pub texture_chunks: FixedVec<'eng, TextureChunkDescriptor<'eng>>,
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
            texture_chunks: FixedVec::new(alloc, header.texture_chunks as usize)?,
        })
    }
}
