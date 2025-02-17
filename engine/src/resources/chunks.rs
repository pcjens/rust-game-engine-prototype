use core::ops::Range;

use platform_abstraction_layer::{Pal, TextureRef};

use crate::resources::CHUNK_SIZE;
#[allow(unused_imports)] // used in docs
use crate::resources::{TEXTURE_CHUNK_DIMENSIONS, TEXTURE_CHUNK_FORMAT};

/// Metadata for loading in a [`ChunkData`].
#[derive(Debug, Clone)]
pub struct ChunkDescriptor {
    /// The range of bytes in the chunk data portion of the database this
    /// texture chunk can be loaded from.
    pub source_bytes: Range<u64>,
}

/// Metadata for loading in a [`TextureChunkData`].
#[derive(Debug, Clone)]
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
pub struct ChunkData(pub [u8; CHUNK_SIZE as usize]);

impl ChunkData {
    /// Creates a zeroed-out [`ChunkData`].
    pub const fn empty() -> ChunkData {
        ChunkData([0; CHUNK_SIZE as usize])
    }

    /// Replaces the chunk contents with the given buffer, based on the
    /// [`ChunkDescriptor`] metadata.
    pub fn update(&mut self, descriptor: &ChunkDescriptor, buffer: &[u8]) {
        let len = (descriptor.source_bytes.end - descriptor.source_bytes.start) as usize;
        self.0[..len].copy_from_slice(buffer);
    }
}

/// Loaded (video) memory for a single texture chunk. Contains a reference to a
/// loaded texture, ready for drawing, with the size and format
/// [`TEXTURE_CHUNK_DIMENSIONS`] and [`TEXTURE_CHUNK_FORMAT`].
pub struct TextureChunkData(pub TextureRef);

impl TextureChunkData {
    /// Creates a new texture chunk from a newly created platform-dependent
    /// texture.
    pub fn empty(platform: &dyn Pal) -> Option<TextureChunkData> {
        let (w, h) = TEXTURE_CHUNK_DIMENSIONS;
        let format = TEXTURE_CHUNK_FORMAT;
        Some(TextureChunkData(platform.create_texture(w, h, format)?))
    }

    /// Uploads the pixel data from the buffer into the texture, based on the
    /// [`TextureChunkDescriptor`] metadata.
    pub fn update(
        &mut self,
        descriptor: &TextureChunkDescriptor,
        buffer: &[u8],
        platform: &dyn Pal,
    ) {
        platform.update_texture(
            self.0,
            0,
            0,
            descriptor.region_width,
            descriptor.region_height,
            buffer,
        );
    }
}
