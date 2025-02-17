//! Texture drawing specifics.
//!
//! This is the "runtime-half" of TextureAsset, the other half being the
//! "import-half" implemented in `import_asset::importers::texture`. These two
//! modules are very tightly linked: this module assumes the texture chunks are
//! laid out in a specific way, and the importer is responsible for writing the
//! texture chunks out in said layout.

use core::ops::Range;

use platform_abstraction_layer::BlendMode;

use crate::{
    geom::Rect,
    resources::{
        texture::TextureMipLevel, ResourceDatabase, ResourceLoader, TEXTURE_CHUNK_DIMENSIONS,
    },
};

use super::{DrawQueue, TexQuad};

const CHUNK_WIDTH: u16 = TEXTURE_CHUNK_DIMENSIONS.0;
const CHUNK_HEIGHT: u16 = TEXTURE_CHUNK_DIMENSIONS.1;

/// Render-time relevant parts of a texture.
pub struct RenderableTexture<'a> {
    /// A list of the texture's mipmaps, with index 0 being the original
    /// texture, and the indices after that each having half the width and
    /// height of the previous level.
    pub mip_chain: &'a [TextureMipLevel],
    /// Should be set to true if the texture has any non-opaque pixels to avoid
    /// rendering artifacts.
    pub transparent: bool,
    /// The draw order used when drawing this texture. See
    /// [`TexQuad::draw_order`].
    pub draw_order: u8,
}

/// The main textured rendering function.
///
/// May push more than one draw command into the [`DrawQueue`] when rendering
/// large textures at large sizes, as the texture may consist of multiple
/// texture chunks (see [`TEXTURE_CHUNK_DIMENSIONS`] for the size of each
/// chunk).
///
/// Returns false if the draw queue does not have enough free space to draw this
/// texture.
pub fn draw(
    src: RenderableTexture,
    dst: Rect,
    draw_queue: &mut DrawQueue,
    resources: &ResourceDatabase,
    resource_loader: &mut ResourceLoader,
) -> bool {
    let draws_left = draw_queue.quads.spare_capacity();

    let mut draw_chunk = |chunk_index: u32, dst: Rect, tex: Rect| {
        if let Some(chunk) = resources.texture_chunks.get(chunk_index) {
            let quad = TexQuad {
                position_top_left: (dst.x, dst.y),
                position_bottom_right: (dst.x + dst.w, dst.y + dst.h),
                texcoord_top_left: (tex.x, tex.y),
                texcoord_bottom_right: (tex.x + tex.w, tex.y + tex.h),
                draw_order: src.draw_order,
                blend_mode: if src.transparent {
                    BlendMode::Blend
                } else {
                    BlendMode::None
                },
                texture: chunk.0,
            };

            draw_queue.quads.push(quad).unwrap();
        } else {
            resource_loader.queue_texture_chunk(chunk_index, resources);
        }
    };

    let texture_by_render_resolution_ratio = match &src.mip_chain[0] {
        TextureMipLevel::SingleChunkTexture { size, .. }
        | TextureMipLevel::MultiChunkTexture { size, .. } => {
            let width_scale = size.0 / (dst.w * draw_queue.scale_factor) as u16;
            let height_scale = size.1 / (dst.h * draw_queue.scale_factor) as u16;
            width_scale.min(height_scale)
        }
    };

    // Since every mip is half the resolution, with index 0 being the
    // highest, log2 of the scale between the actual texture and the
    // rendered size matches the index of the mip that matches the rendered
    // size the closest. ilog2 rounds down, which is fine, as that'll end up
    // picking the higher resolution mip of the two mips around the real
    // log2 result.
    let mip_level = texture_by_render_resolution_ratio
        .checked_ilog2()
        .unwrap_or(0) as usize;

    let max_mip = src.mip_chain.len() - 1;
    let mip = &src.mip_chain[mip_level.min(max_mip)];

    match mip {
        TextureMipLevel::SingleChunkTexture {
            offset,
            size,
            texture_chunk,
        } => {
            if draws_left == 0 {
                return false;
            }

            let tex_src = Rect {
                x: offset.0 as f32 / CHUNK_WIDTH as f32,
                y: offset.1 as f32 / CHUNK_HEIGHT as f32,
                w: size.0 as f32 / CHUNK_WIDTH as f32,
                h: size.1 as f32 / CHUNK_HEIGHT as f32,
            };
            draw_chunk(*texture_chunk, dst, tex_src);

            true
        }

        TextureMipLevel::MultiChunkTexture {
            size,
            texture_chunks,
        } => {
            let chunks_x = size.0.div_ceil(CHUNK_WIDTH - 2) as u32;
            let chunks_y = size.1.div_ceil(CHUNK_HEIGHT - 2) as u32;
            assert_eq!(
                chunks_x * chunks_y,
                texture_chunks.end - texture_chunks.start,
                "resource database has a corrupt chunk, amount of chunks does not match the texture size",
            );

            if draws_left < (chunks_x * chunks_y) as usize {
                return false;
            }

            draw_multi_chunk_texture(
                dst,
                *size,
                texture_chunks.clone(),
                (chunks_x, chunks_y),
                draw_chunk,
            );

            true
        }
    }
}

fn draw_multi_chunk_texture(
    Rect { x, y, w, h }: Rect,
    (tex_width, tex_height): (u16, u16),
    chunks: Range<u32>,
    (chunks_x, chunks_y): (u32, u32),
    mut draw: impl FnMut(u32, Rect, Rect),
) {
    let scale_x = w / tex_width as f32;
    let scale_y = h / tex_height as f32;

    let mut tex_x_pos = 0;
    let mut tex_y_pos = 0;
    for cy in 0..chunks_y {
        let curr_chunk_h = (tex_height - tex_y_pos).min(CHUNK_HEIGHT - 2);
        for cx in 0..chunks_x {
            let curr_chunk_index = chunks.start + cx + cy * chunks_x;
            let curr_chunk_w = (tex_width - tex_x_pos).min(CHUNK_WIDTH - 2);

            let dst = Rect {
                x: x + tex_x_pos as f32 * scale_x,
                y: y + tex_y_pos as f32 * scale_y,
                w: curr_chunk_w as f32 * scale_x,
                h: curr_chunk_h as f32 * scale_y,
            };

            let tex_src = Rect {
                x: 1. / CHUNK_WIDTH as f32,
                y: 1. / CHUNK_HEIGHT as f32,
                w: curr_chunk_w as f32 / CHUNK_WIDTH as f32,
                h: curr_chunk_h as f32 / CHUNK_HEIGHT as f32,
            };

            draw(curr_chunk_index, dst, tex_src);

            tex_x_pos += curr_chunk_w;
        }
        tex_y_pos += curr_chunk_h;
        tex_x_pos = 0;
    }
}
