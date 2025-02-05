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
    geom::Rect,
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
const CHUNK_WIDTH: u16 = TEXTURE_CHUNK_DIMENSIONS.0;
const CHUNK_HEIGHT: u16 = TEXTURE_CHUNK_DIMENSIONS.1;

/// One mipmap level of a [`TextureAsset`].
#[derive(Debug)]
pub enum TextureMipLevel {
    SingleChunkTexture {
        /// Offset from the topmost and leftmost chunks where the actual texture
        /// starts.
        offset: (u16, u16),
        /// The dimensions of the texture in pixels.
        size: (u16, u16),
        /// The chunk the texture's pixels are located in. The subregion to
        /// render is described by the `offset` and `size` fields.
        texture_chunk: u32,
    },
    MultiChunkTexture {
        /// The dimensions of the texture in pixels.
        size: (u16, u16),
        /// The chunks the texture is made up of.
        ///
        /// Chunks are allocated for a multi-chunk texture starting from the
        /// top-left, row by row. Each chunk has a 1px clamp-to-edge border,
        /// inside which the actual texture is. The chunks on the right and
        /// bottom edges of the texture are the only ones that don't occupy
        /// their texture chunk entirely, they instead occupy only up to the
        /// texture's `width` and `height` plus the border, effectively taking
        /// up a `width + 2` by `height + 2` region from the top left corner of
        /// those chunks due to the border.
        texture_chunks: Range<u32>,
    },
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
        use pixels::TexPixels;

        const CHUNK_WIDTH: usize = TEXTURE_CHUNK_DIMENSIONS.0 as usize;
        const CHUNK_HEIGHT: usize = TEXTURE_CHUNK_DIMENSIONS.1 as usize;
        const CHUNK_STRIDE: usize = CHUNK_WIDTH * BPP;
        const CHUNK_BYTES: usize = CHUNK_STRIDE * CHUNK_HEIGHT;

        let mut transparent = false;
        let mut pending_chunk_width: usize = 0;
        let mut pending_chunk_height: usize = 0;
        // A pixels array for a max-size chunk, only the relevant region is
        // copied into chunk data.
        let mut buf = std::vec![0; CHUNK_BYTES];
        let mut pending_chunk_tex =
            TexPixels::new(&mut buf, CHUNK_STRIDE, CHUNK_WIDTH, CHUNK_HEIGHT).unwrap();
        let mut pending_chunk_index = output_chunks.len() as u32;

        // Writes out the pending chunk into `output_chunks` and `chunk_data`.
        let mut flush_pending_chunk =
            |width: usize, height: usize, tex: &TexPixels, chunk_index: &mut u32| {
                let start = chunk_data.stream_position().unwrap();
                for y in 0..height {
                    chunk_data.write_all(&tex.row(y)[..width * BPP]).unwrap();
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
        // writes out the relevant chunks' data.
        let mut allocate = |mut tex: pixels::TexPixels| -> TextureMipLevel {
            if !transparent {
                transparent = tex.has_transparent_pixels();
            }

            // Try to fit the texture in the pending chunk
            {
                // The width and height of the whole texture including the borders
                let req_w = tex.width + 2;
                let req_h = tex.height + 2;

                // Try to fit the texture to the right of the reserved region
                if pending_chunk_width + req_w <= CHUNK_WIDTH && req_h <= CHUNK_HEIGHT {
                    let x_offset = pending_chunk_width;

                    let mut dst_with_border = pending_chunk_tex
                        .subregion(x_offset, 0, req_w, req_h)
                        .unwrap();
                    let mut dst = dst_with_border.shrink().unwrap();
                    dst.copy_from(&tex);
                    dst_with_border.fill_border();

                    pending_chunk_width += req_w;
                    pending_chunk_height = pending_chunk_height.max(req_h);
                    return TextureMipLevel::SingleChunkTexture {
                        offset: (x_offset as u16 + 1, 1),
                        size: (tex.width as u16, tex.height as u16),
                        texture_chunk: pending_chunk_index,
                    };
                }

                // Try to fit the texture below the reserved region
                if req_w <= CHUNK_WIDTH && pending_chunk_height + req_h <= CHUNK_HEIGHT {
                    let y_offset = pending_chunk_height;

                    let mut dst_with_border = pending_chunk_tex
                        .subregion(0, y_offset, req_w, req_h)
                        .unwrap();
                    let mut dst = dst_with_border.shrink().unwrap();
                    dst.copy_from(&tex);
                    dst_with_border.fill_border();

                    pending_chunk_width = pending_chunk_width.max(req_w);
                    pending_chunk_height += req_h;
                    return TextureMipLevel::SingleChunkTexture {
                        offset: (1, y_offset as u16 + 1),
                        size: (tex.width as u16, tex.height as u16),
                        texture_chunk: pending_chunk_index,
                    };
                }
            }

            // Create and write out any amount of required chunks, leaving the
            // last chunk pending. Borders are considered on a per-chunk basis.
            let first_chunk = pending_chunk_index + 1;
            let max_width_per_chunk = CHUNK_WIDTH - 2;
            let max_height_per_chunk = CHUNK_HEIGHT - 2;
            for y in 0..tex.height.div_ceil(max_height_per_chunk) {
                let y0 = y * max_height_per_chunk;
                let y1 = ((y + 1) * max_height_per_chunk).min(tex.height);
                let chunk_height = y1 - y0;
                for x in 0..tex.width.div_ceil(max_width_per_chunk) {
                    let x0 = x * max_width_per_chunk;
                    let x1 = ((x + 1) * max_width_per_chunk).min(tex.width);
                    let chunk_width = x1 - x0;

                    // Flush out the pending chunk (either from a previous
                    // iteration or a whole another allocate-call)
                    flush_pending_chunk(
                        pending_chunk_width,
                        pending_chunk_height,
                        &pending_chunk_tex,
                        &mut pending_chunk_index,
                    );
                    pending_chunk_tex.pixels.fill(0);

                    // Copy over slice (x, y) of the texture to the new pending
                    // chunk, with the last chunk possibly leaving space for
                    // smaller mipmaps in future allocate calls
                    pending_chunk_width = chunk_width + 2;
                    pending_chunk_height = chunk_height + 2;
                    let chunk_tex = tex.subregion(x0, y0, chunk_width, chunk_height).unwrap();
                    let mut dst_with_border = pending_chunk_tex
                        .subregion(0, 0, chunk_width + 2, chunk_height + 2)
                        .unwrap();
                    let mut dst = dst_with_border.shrink().unwrap();
                    dst.copy_from(&chunk_tex);
                    dst_with_border.fill_border();
                }
            }

            if first_chunk == pending_chunk_index {
                TextureMipLevel::SingleChunkTexture {
                    offset: (1, 1),
                    size: (tex.width as u16, tex.height as u16),
                    texture_chunk: first_chunk,
                }
            } else {
                TextureMipLevel::MultiChunkTexture {
                    size: (tex.width as u16, tex.height as u16),
                    texture_chunks: first_chunk..pending_chunk_index + 1,
                }
            }
        };

        // Write out each mip level (calls `allocate` a bunch of times, which
        // uses the pending chunk and flushes it as it runs out of room)
        let (mut width, mut height) = (width as usize, height as usize);
        let mut pixels = std::vec![0u8; width * height * BPP];
        let mut mip_chain = ArrayVec::new();
        for _ in 0..MAX_MIPS {
            let stride = width * BPP;
            let pixels = &mut pixels[..height * stride];
            let tex = pixels::TexPixels::new(pixels, stride, width, height).unwrap();
            render_texture(tex.width as u16, tex.height as u16, tex.stride, tex.pixels);
            mip_chain.push(allocate(tex));

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
            &pending_chunk_tex,
            &mut pending_chunk_index,
        );

        TextureAsset {
            transparent,
            mip_chain,
        }
    }

    /// Draw this texture into the `dst` rectangle.
    ///
    /// Returns false if the texture couldn't be drawn due to the draw queue
    /// filling up.
    #[must_use]
    pub fn draw(
        &self,
        dst: Rect,
        mip: usize, // FIXME: replace with a proper mip level calculation
        draw_order: u8,
        draw_queue: &mut DrawQueue,
        resources: &ResourceDatabase,
        resource_loader: &mut ResourceLoader,
    ) -> bool {
        let draws_left = draw_queue.quads.spare_capacity();
        let mut draw = |chunk_index: u32, dst: Rect, tex: Rect| {
            if let Some(chunk) = resources.texture_chunks.get(chunk_index) {
                let quad = TexQuad {
                    position_top_left: (dst.x, dst.y),
                    position_bottom_right: (dst.x + dst.w, dst.y + dst.h),
                    texcoord_top_left: (tex.x, tex.y),
                    texcoord_bottom_right: (tex.x + tex.w, tex.y + tex.h),
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
        };

        let mip = &self.mip_chain[mip];

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
                    // FIXME: remove these -1 and +2's, just debugging the borders
                    x: (offset.0 - 1) as f32 / CHUNK_WIDTH as f32,
                    y: (offset.1 - 1) as f32 / CHUNK_HEIGHT as f32,
                    w: (size.0 + 2) as f32 / CHUNK_WIDTH as f32,
                    h: (size.1 + 2) as f32 / CHUNK_HEIGHT as f32,
                };
                draw(*texture_chunk, dst, tex_src);

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

                self.draw_multi_chunk(
                    dst,
                    *size,
                    texture_chunks.clone(),
                    (chunks_x, chunks_y),
                    draw,
                );

                true
            }
        }
    }

    fn draw_multi_chunk(
        &self,
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
                    // FIXME: remove these -1 and +2's, just separating chunks for clarity
                    x: x + tex_x_pos as f32 * scale_x + 1.0,
                    y: y + tex_y_pos as f32 * scale_y + 1.0,
                    w: curr_chunk_w as f32 * scale_x - 2.0,
                    h: curr_chunk_h as f32 * scale_y - 2.0,
                };

                let tex_src = Rect {
                    // FIXME: remove these -1 and +2's, just debugging the borders
                    x: ((tex_x_pos + 1 - 1) % (CHUNK_WIDTH - 2)) as f32 / CHUNK_WIDTH as f32,
                    y: ((tex_y_pos + 1 - 1) % (CHUNK_HEIGHT - 2)) as f32 / CHUNK_HEIGHT as f32,
                    w: (curr_chunk_w + 2) as f32 / CHUNK_WIDTH as f32,
                    h: (curr_chunk_h + 2) as f32 / CHUNK_HEIGHT as f32,
                };

                draw(curr_chunk_index, dst, tex_src);

                tex_x_pos += curr_chunk_w;
            }
            tex_y_pos += curr_chunk_h;
            tex_x_pos = 0;
        }
    }
}
