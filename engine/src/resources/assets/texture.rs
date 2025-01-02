use core::ops::Range;

use platform_abstraction_layer::BlendMode;

use crate::{
    renderer::{DrawQueue, TexQuad},
    resources::{ResourceDatabase, TEXTURE_CHUNK_DIMENSIONS},
    FixedVec,
};

use super::gen_asset_handle_code;

gen_asset_handle_code!(
    TextureAsset,
    TextureHandle,
    find_texture,
    get_texture,
    textures
);

#[derive(Debug)]
pub struct TextureAsset {
    /// The width of the whole texture.
    pub width: u16,
    /// The height of the whole texture.
    pub height: u16,
    /// Whether the texture's alpha should be taken into consideration while
    /// rendering.
    pub transparent: bool,
    /// The chunks the texture is made up of. Multi-chunk textures are allocated
    /// starting from the top-left of the texture, row-major.
    pub texture_chunks: Range<u32>,
}

impl TextureAsset {
    // TODO: There should probably be some de/serialization logic at this level,
    // since this is the level where we know if we have e.g. multiple textures
    // in one, if we need padding pixels between them, or pretend-wrapping
    // pixels to compensate for not actually being on a texture border, etc.

    pub fn draw(
        &self,
        xywh: [f32; 4],
        draw_order: u8,
        draw_queue: &mut DrawQueue,
        resources: &ResourceDatabase,
        texture_chunk_load_requests: &mut FixedVec<'_, u32>,
    ) {
        // TODO: implement multi-chunk-texture draws
        assert_eq!(1, self.texture_chunks.end - self.texture_chunks.start);

        if let Some(chunk) = resources.texture_chunks.get(self.texture_chunks.start) {
            let _ = draw_queue.quads.push(TexQuad {
                xywh,
                texture_xywh: [
                    0.0,
                    0.0,
                    self.width as f32 / TEXTURE_CHUNK_DIMENSIONS.0 as f32,
                    self.height as f32 / TEXTURE_CHUNK_DIMENSIONS.1 as f32,
                ],
                draw_order,
                blend_mode: if self.transparent {
                    BlendMode::Blend
                } else {
                    BlendMode::None
                },
                texture: chunk.0,
            });
        } else {
            let _ = texture_chunk_load_requests.push(self.texture_chunks.start);
        }
    }
}
