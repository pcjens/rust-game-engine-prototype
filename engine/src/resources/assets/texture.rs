use core::ops::Range;

use super::gen_asset_handle_code;

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

gen_asset_handle_code!(
    TextureAsset,
    TextureHandle,
    find_texture,
    get_texture,
    textures
);
