use core::fmt::Debug;

use platform_abstraction_layer::TextureRef;

use crate::resources::{TEXTURE_CHUNK_DIMENSIONS, TEXTURE_CHUNK_FORMAT};

use super::CHUNK_SIZE;

/// Loaded memory for a single regular chunk. Contains [`CHUNK_SIZE`] bytes.
#[repr(C, align(64))]
pub struct LoadedChunk(pub [u8; CHUNK_SIZE]);

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
