use core::ops::Range;

use platform_abstraction_layer::TextureRef;

use crate::resources::CHUNK_SIZE;
#[allow(unused_imports)] // used in docs
use crate::resources::{TEXTURE_CHUNK_DIMENSIONS, TEXTURE_CHUNK_FORMAT};

#[derive(Debug)]
pub struct ChunkDescriptor {
    /// The range of bytes in the chunk data portion of the database this
    /// texture chunk can be loaded from.
    pub source_bytes: Range<u64>,
}

#[derive(Debug)]
pub struct TextureChunkDescriptor {
    /// The width of the texture the chunk contains.
    pub region_width: u16,
    /// The height of the texture the chunk contains.
    pub region_height: u16,
    /// The range of bytes in the chunk data portion of the database this
    /// texture chunk can be loaded from.
    pub source_bytes: Range<u64>,
}

/// Loaded memory for a single regular chunk. Contains [`CHUNK_SIZE`] bytes.
#[repr(C, align(64))]
pub struct LoadedChunk(pub [u8; CHUNK_SIZE as usize]);

/// Loaded (video) memory for a single texture chunk. Contains a reference to a
/// loaded texture, ready for drawing, with the size and format
/// [`TEXTURE_CHUNK_DIMENSIONS`] and [`TEXTURE_CHUNK_FORMAT`].
pub struct LoadedTextureChunk(pub TextureRef);
