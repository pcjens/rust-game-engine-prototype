mod descriptors {
    use core::ops::Range;

    pub const CHUNK_REGION_AUDIO_CLIP_TAG: u8 = 0;

    #[derive(Debug)]
    pub enum ChunkRegion {
        AudioClip {
            start_sample_index: u32,
            samples: u32,
        },
    }

    #[derive(Debug)]
    pub struct ChunkDescriptor {
        /// The region of a resource the chunk contains (e.g. a timespan of an
        /// audio clip).
        pub region: ChunkRegion,
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
}

mod loaded {
    use core::fmt::Debug;

    use platform_abstraction_layer::TextureRef;

    use crate::resources::{CHUNK_SIZE, TEXTURE_CHUNK_DIMENSIONS, TEXTURE_CHUNK_FORMAT};

    /// Loaded memory for a single regular chunk. Contains [`CHUNK_SIZE`] bytes.
    #[repr(C, align(64))]
    pub struct LoadedChunk(pub [u8; CHUNK_SIZE as usize]);

    impl Debug for LoadedChunk {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "LoadedChunk({} KiB of data)", CHUNK_SIZE / 1024)
        }
    }

    /// Loaded (video) memory for a single texture chunk. Contains a reference to a
    /// loaded texture, ready for drawing, with the size and format
    /// [`TEXTURE_CHUNK_DIMENSIONS`] and [`TEXTURE_CHUNK_FORMAT`].
    pub struct LoadedTextureChunk(pub TextureRef);

    impl Debug for LoadedTextureChunk {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let (w, h) = TEXTURE_CHUNK_DIMENSIONS;
            let bpp = TEXTURE_CHUNK_FORMAT.bytes_per_pixel();
            let kibs = w as usize * h as usize * bpp / 1024;
            write!(f, "LoadedTextureChunk({w}x{h} texture, {kibs} KiB)")
        }
    }
}

pub use descriptors::{
    ChunkDescriptor, ChunkRegion, TextureChunkDescriptor, CHUNK_REGION_AUDIO_CLIP_TAG,
};
pub use loaded::{LoadedChunk, LoadedTextureChunk};
