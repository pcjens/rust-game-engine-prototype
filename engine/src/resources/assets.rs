use core::ops::Range;

#[derive(Debug)]
pub struct AudioClipAsset {
    pub samples_per_second: u32,
    pub samples: u32,
    pub chunks: Range<u32>,
}

#[derive(Debug)]
pub struct TextureAsset {
    /// The width of the whole texture.
    pub width: u16,
    /// The height of the whole texture.
    pub height: u16,
    /// The chunks the texture is made up of. Multi-chunk textures are allocated
    /// starting from the top-left of the texture, row-major.
    pub texture_chunks: Range<u32>,
}
