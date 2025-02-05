#[cfg(feature = "asset-conditioning")]
extern crate std;

#[cfg(feature = "asset-conditioning")]
mod pixels;

use core::ops::Range;

use arrayvec::ArrayVec;
use platform_abstraction_layer::BlendMode;

#[cfg(feature = "asset-conditioning")]
use crate::resources::TextureChunkDescriptor;
use crate::{
    renderer::{DrawQueue, TexQuad},
    resources::{ResourceDatabase, ResourceLoader, TEXTURE_CHUNK_DIMENSIONS},
};

use super::gen_asset_handle_code;

gen_asset_handle_code!(
    TextureAsset,
    TextureHandle,
    find_texture,
    get_texture,
    textures
);

/// The maximum amount of mip levels for a texture.
pub const MAX_MIPS: usize = 12;

/// Bytes per pixel in the texture chunk format, the only format used within
/// this module.
#[cfg(feature = "asset-conditioning")]
const BPP: usize = crate::resources::TEXTURE_CHUNK_FORMAT.bytes_per_pixel();

/// One mipmap level of a [`TextureAsset`].
#[derive(Debug)]
pub struct TextureMipLevel {
    /// Offset from the topmost and leftmost chunks where the actual texture
    /// starts.
    pub offset: (u16, u16),
    /// The dimensions of the texture in pixels.
    pub size: (u16, u16),
    /// The chunks the texture is made up of. Multi-chunk textures are allocated
    /// starting from the top-left of the texture, row-major, with the chunks in
    /// between corners having 1px borders. So where the offset into the chunk
    /// is unclear (e.g. the top row of chunks has the y offset from the
    /// `offset` field of this struct, but the x offset isn't clear aside from
    /// the top-left corner) the offset is 1 and the size is the chunk's
    /// width/height minus 2.
    pub texture_chunks: Range<u32>,
}

/// Drawable image.
#[derive(Debug)]
pub struct TextureAsset {
    /// Whether the texture's alpha should be taken into consideration while
    /// rendering.
    pub transparent: bool,
    /// The actual specific-size textures used for rendering depending on the
    /// size of the texture on screen.
    pub mip_chain: ArrayVec<TextureMipLevel, MAX_MIPS>,
}

impl TextureAsset {
    #[cfg(feature = "asset-conditioning")]
    pub fn create(
        width: u16,
        height: u16,
        render_texture: impl Fn(u16, u16, usize, &mut [u8]),
        chunk_data: &mut (impl std::io::Write + std::io::Seek),
        output_chunks: &mut std::vec::Vec<TextureChunkDescriptor>,
    ) -> TextureAsset {
        const CHUNK_WIDTH: usize = TEXTURE_CHUNK_DIMENSIONS.0 as usize;
        const CHUNK_HEIGHT: usize = TEXTURE_CHUNK_DIMENSIONS.1 as usize;
        const CHUNK_STRIDE: usize = CHUNK_WIDTH * BPP;
        const CHUNK_BYTES: usize = CHUNK_STRIDE * CHUNK_HEIGHT;

        let mut transparent = false;
        let mut pending_chunk_width = 0;
        let mut pending_chunk_height = 0;
        // A pixels array for a max-size chunk, only the relevant region is
        // copied into chunk data.
        let mut pending_chunk_pixels = std::vec![0; CHUNK_BYTES];
        let mut pending_chunk_index = output_chunks.len() as u32;

        let mut flush_pending_chunk =
            |width: usize, height: usize, chunk_pixels: &[u8], chunk_index: &mut u32| {
                let start = chunk_data.stream_position().unwrap();
                for y in 0..height {
                    chunk_data
                        .write_all(&chunk_pixels[y * CHUNK_STRIDE..width * BPP + y * CHUNK_STRIDE])
                        .unwrap();
                }
                let end = chunk_data.stream_position().unwrap();
                output_chunks.push(TextureChunkDescriptor {
                    region_width: width as u16,
                    region_height: height as u16,
                    source_bytes: start..end,
                });
                *chunk_index += 1;
            };

        // Allocates space from the texture chunk (or multiple, if needed), and
        // writes out the relevant chunks' data. The width, height and pixels
        // contain the 1px border.
        let mut allocate = |width: usize, height: usize, pixels: &[u8]| -> TextureMipLevel {
            assert_eq!(width * height * 4, pixels.len());
            if !transparent {
                transparent = pixels.chunks_exact(4).any(|rgba| rgba[3] != 0xFF);
            }

            let stride = width * BPP;

            // TODO: replace the inline blitting logic with Texture helpers

            // Try to fit the texture to the right of the reserved region
            if pending_chunk_width + width <= CHUNK_WIDTH && height <= CHUNK_HEIGHT {
                let x_offset = pending_chunk_width;
                for y in 0..height {
                    let from_start = y * stride;
                    let from_end = (y + 1) * stride;
                    let to_start = x_offset * BPP + y * CHUNK_STRIDE;
                    let to_end = (x_offset + width) * BPP + y * CHUNK_STRIDE;
                    pending_chunk_pixels[to_start..to_end]
                        .copy_from_slice(&pixels[from_start..from_end]);
                }
                pending_chunk_width += width;
                pending_chunk_height = pending_chunk_height.max(height);
                return TextureMipLevel {
                    size: (width as u16 - 2, height as u16 - 2),
                    offset: (x_offset as u16 + 1, 1),
                    texture_chunks: pending_chunk_index..pending_chunk_index + 1,
                };
            }

            // Try to fit the texture below the reserved region
            if width <= CHUNK_WIDTH && pending_chunk_height + height <= CHUNK_HEIGHT {
                let y_offset = pending_chunk_height;
                for y in 0..height {
                    let from_start = y * stride;
                    let from_end = (y + 1) * stride;
                    let to_start = (y + y_offset) * CHUNK_STRIDE;
                    let to_end = width * BPP + (y + y_offset) * CHUNK_STRIDE;
                    pending_chunk_pixels[to_start..to_end]
                        .copy_from_slice(&pixels[from_start..from_end]);
                }
                pending_chunk_width = pending_chunk_width.max(width);
                pending_chunk_height += height;
                return TextureMipLevel {
                    size: (width as u16 - 2, height as u16 - 2),
                    offset: (1, y_offset as u16 + 1),
                    texture_chunks: pending_chunk_index..pending_chunk_index + 1,
                };
            }

            // Create and write out the required chunks, leaving the last chunk pending
            let first_chunk = pending_chunk_index + 1;
            for y in 0..height.div_ceil(CHUNK_HEIGHT) {
                let y0 = y * CHUNK_HEIGHT;
                let y1 = ((y + 1) * CHUNK_HEIGHT).min(height);
                for x in 0..width.div_ceil(CHUNK_WIDTH) {
                    let x0 = x * CHUNK_WIDTH;
                    let x1 = ((x + 1) * CHUNK_WIDTH).min(width);

                    // Flush out the pending chunk
                    flush_pending_chunk(
                        pending_chunk_width,
                        pending_chunk_height,
                        &pending_chunk_pixels,
                        &mut pending_chunk_index,
                    );
                    pending_chunk_pixels.fill(0);

                    // Copy over slice (x, y) of the texture to the pending
                    // chunk, with the last chunk possibly leaving space for
                    // smaller mipmaps in future allocate calls
                    pending_chunk_width = x1 - x0;
                    pending_chunk_height = y1 - y0;
                    for y in y0..y1 {
                        pending_chunk_pixels
                            [(y - y0) * CHUNK_STRIDE..(x1 - x0) * BPP + (y - y0) * CHUNK_STRIDE]
                            .copy_from_slice(&pixels[x0 * BPP + y * stride..x1 * BPP + y * stride]);
                    }
                }
            }

            TextureMipLevel {
                size: (width as u16 - 2, height as u16 - 2),
                offset: (1, 1),
                texture_chunks: first_chunk..pending_chunk_index + 1,
            }
        };

        let (mut width, mut height) = (width as usize, height as usize);
        let mut pixels = std::vec![0u8; (width + 2) * (height + 2) * BPP];
        let mut mip_chain = ArrayVec::new();
        for _ in 0..MAX_MIPS {
            let (width_with_border, height_with_border) = (width + 2, height + 2);
            let stride = width_with_border * BPP;
            let pixels = &mut pixels[..height_with_border * stride];
            let mut tex_with_border =
                pixels::TexPixels::new(pixels, stride, width_with_border, height_with_border)
                    .unwrap();
            let tex_inner = tex_with_border.shrink().unwrap();
            render_texture(
                tex_inner.width as u16,
                tex_inner.height as u16,
                tex_inner.stride,
                tex_inner.pixels,
            );

            tex_with_border.fill_border();

            mip_chain.push(allocate(
                tex_with_border.width,
                tex_with_border.height,
                tex_with_border.pixels,
            ));

            (width, height) = (width.div_ceil(2), height.div_ceil(2));
            if width == 1 && height == 1 {
                break;
            }
            (width, height) = (width.max(2), height.max(2));
        }

        // Flush out the final chunk
        flush_pending_chunk(
            pending_chunk_width,
            pending_chunk_height,
            &pending_chunk_pixels,
            &mut pending_chunk_index,
        );

        TextureAsset {
            transparent,
            mip_chain,
        }
    }

    /// Draw this texture at coordinates x and y with some width and height.
    /// Returns false if the texture couldn't be drawn due to the draw queue
    /// filling up.
    pub fn draw(
        &self,
        (x, y, width, height): (f32, f32, f32, f32),
        mip: usize, // FIXME: replace with a proper mip level calculation
        draw_order: u8,
        draw_queue: &mut DrawQueue,
        resources: &ResourceDatabase,
        resource_loader: &mut ResourceLoader,
    ) -> bool {
        const CHUNK_WIDTH: u16 = TEXTURE_CHUNK_DIMENSIONS.0;
        const CHUNK_HEIGHT: u16 = TEXTURE_CHUNK_DIMENSIONS.1;

        let mip = &self.mip_chain[mip];
        let chunks_x = mip.size.0.div_ceil(CHUNK_WIDTH) as u32;
        let chunks_y = mip.size.1.div_ceil(CHUNK_HEIGHT) as u32;
        assert_eq!(
            chunks_x * chunks_y,
            mip.texture_chunks.end - mip.texture_chunks.start,
            "resource database has a corrupt chunk, amount of chunks does not match the texture size",
        );

        if draw_queue.quads.spare_capacity() < (chunks_x * chunks_y) as usize {
            return false;
        }

        let tex_offset_x = mip.offset.0 as f32;
        let tex_offset_y = mip.offset.1 as f32;
        let tex_width = mip.size.0 as f32;
        let tex_height = mip.size.1 as f32;
        let scale_x = width / tex_width;
        let scale_y = height / tex_height;

        let mut tex_x_pos = tex_offset_x;
        let mut tex_y_pos = tex_offset_y;
        for cy in 0..chunks_y {
            let row_first_desc = &resources.texture_chunk_descriptors
                [(mip.texture_chunks.start + cy * chunks_x) as usize];
            let chunk_height = if cy == 0 {
                ((row_first_desc.region_height - mip.offset.1) as f32).min(tex_height)
            } else {
                let y_off_into_texture = tex_y_pos - mip.offset.1 as f32;
                (tex_height - y_off_into_texture).min(row_first_desc.region_height as f32)
            };

            for cx in 0..chunks_x {
                let chunk_index = mip.texture_chunks.start + cx + cy * chunks_x;
                let desc = &resources.texture_chunk_descriptors[chunk_index as usize];

                let chunk_width = if cx == 0 {
                    ((desc.region_width - mip.offset.0) as f32).min(tex_width)
                } else {
                    let x_off_into_texture = tex_x_pos - mip.offset.0 as f32;
                    (tex_width - x_off_into_texture).min(desc.region_width as f32)
                };

                if let Some(chunk) = resources.texture_chunks.get(chunk_index) {
                    let x = x + (tex_x_pos - tex_offset_x) * scale_x;
                    let y = y + (tex_y_pos - tex_offset_y) * scale_y;
                    let width = chunk_width * scale_x;
                    let height = chunk_height * scale_y;

                    let chunk_x_pos = tex_x_pos % CHUNK_WIDTH as f32;
                    let chunk_y_pos = tex_y_pos % CHUNK_HEIGHT as f32;
                    let (tex_x0, tex_x1) = (
                        chunk_x_pos / CHUNK_WIDTH as f32,
                        (chunk_x_pos + chunk_width) / CHUNK_WIDTH as f32,
                    );
                    let (tex_y0, tex_y1) = (
                        chunk_y_pos / CHUNK_HEIGHT as f32,
                        (chunk_y_pos + chunk_height) / CHUNK_HEIGHT as f32,
                    );

                    let quad = TexQuad {
                        position_top_left: (x, y),
                        position_bottom_right: (x + width, y + height),
                        texcoord_top_left: (tex_x0, tex_y0),
                        texcoord_bottom_right: (tex_x1, tex_y1),
                        draw_order,
                        blend_mode: if self.transparent {
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

                tex_x_pos += chunk_width;
            }
            tex_y_pos += chunk_height;
            tex_x_pos = mip.offset.0 as f32;
        }

        true
    }
}
