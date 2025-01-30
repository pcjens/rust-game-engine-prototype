#[cfg(feature = "asset-conditioning")]
extern crate std;

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

/// One mipmap level of a [`TextureAsset`].
#[derive(Debug)]
pub struct TextureMipLevel {
    /// Offset from the topmost and leftmost chunks where the actual texture
    /// starts.
    pub offset: (u16, u16),
    /// The dimensions of the texture in pixels.
    pub size: (u16, u16),
    /// The chunks the texture is made up of. Multi-chunk textures are allocated
    /// starting from the top-left of the texture, row-major.
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
    pub fn create<F: Fn(u16, u16, usize, &mut [u8]), W: std::io::Write + std::io::Seek>(
        width: u16,
        height: u16,
        render_texture: F,
        chunk_data: &mut W,
        output_chunks: &mut std::vec::Vec<TextureChunkDescriptor>,
    ) -> TextureAsset {
        use crate::resources::TEXTURE_CHUNK_FORMAT;

        const BPP: usize = TEXTURE_CHUNK_FORMAT.bytes_per_pixel();
        const CHUNK_BYTES: usize =
            TEXTURE_CHUNK_DIMENSIONS.0 as usize * TEXTURE_CHUNK_DIMENSIONS.1 as usize * BPP;
        const CHUNK_WIDTH: usize = TEXTURE_CHUNK_DIMENSIONS.0 as usize;
        const CHUNK_HEIGHT: usize = TEXTURE_CHUNK_DIMENSIONS.1 as usize;
        const CHUNK_STRIDE: usize = CHUNK_WIDTH * BPP;

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
                            .copy_from_slice(&pixels[y * stride..(y + 1) * stride]);
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
            let pixels_with_border = &mut pixels[..height_with_border * stride];
            let pixels = &mut pixels_with_border
                [BPP + stride..(1 + width) * BPP + (height_with_border - 1) * stride];
            render_texture(width as u16, height as u16, stride, pixels);

            // The following section just adds a 1 pixel wide border around the
            // texture to make bilinear samples not mix up with neighbors in the
            // texture chunk.
            let mut copy_from_to = |from: (usize, usize), to: (usize, usize)| {
                let (x0, y0) = from;
                let (x1, y1) = to;
                for c in 0..BPP {
                    pixels_with_border[c + x1 * BPP + y1 * stride] =
                        pixels_with_border[c + x0 * BPP + y0 * stride];
                }
            };
            // Fill out the top and bottom border (without corners)
            for (y, y_from) in [(0, 1), (height_with_border - 1, height_with_border - 2)] {
                for x in 1..1 + width {
                    copy_from_to((x, y_from), (x, y));
                }
            }
            // Fill out the left and right border (without corners)
            for y in 1..1 + height {
                for (x, x_from) in [(0, 1), (width_with_border - 1, width_with_border - 2)] {
                    copy_from_to((x_from, y), (x, y));
                }
            }
            // Fill out the corners
            let x_last = width_with_border - 1; // x coord of the right border
            let y_last = height_with_border - 1; // y coord of the bottom border
            copy_from_to((1, 0), (0, 0));
            copy_from_to((x_last - 1, 0), (x_last, 0));
            copy_from_to((1, y_last), (0, y_last));
            copy_from_to((x_last - 1, y_last), (x_last, y_last));

            mip_chain.push(allocate(
                width_with_border,
                height_with_border,
                pixels_with_border,
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
    pub fn draw(
        &self,
        (x, y, width, height): (f32, f32, f32, f32),
        draw_order: u8,
        draw_queue: &mut DrawQueue,
        resources: &ResourceDatabase,
        resource_loader: &mut ResourceLoader,
    ) {
        let mip = &self.mip_chain[0];
        assert_eq!(1, mip.texture_chunks.end - mip.texture_chunks.start);
        let chunk_index = mip.texture_chunks.start;

        if let Some(chunk) = resources.texture_chunks.get(chunk_index) {
            let (x0, x1) = (mip.offset.0, mip.offset.0 + mip.size.0);
            let (y0, y1) = (mip.offset.1, mip.offset.1 + mip.size.1);
            let _ = draw_queue.quads.push(TexQuad {
                position_top_left: (x, y),
                position_bottom_right: (x + width, y + height),
                texcoord_top_left: (
                    x0 as f32 / TEXTURE_CHUNK_DIMENSIONS.0 as f32,
                    y0 as f32 / TEXTURE_CHUNK_DIMENSIONS.1 as f32,
                ),
                texcoord_bottom_right: (
                    y1 as f32 / TEXTURE_CHUNK_DIMENSIONS.0 as f32,
                    x1 as f32 / TEXTURE_CHUNK_DIMENSIONS.1 as f32,
                ),
                draw_order,
                blend_mode: if self.transparent {
                    BlendMode::Blend
                } else {
                    BlendMode::None
                },
                texture: chunk.0,
            });
        } else {
            resource_loader.queue_texture_chunk(chunk_index, resources);
        }
    }
}
