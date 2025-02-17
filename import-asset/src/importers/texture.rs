mod pixels;

use std::{
    fs,
    io::{Seek, Write},
    path::Path,
};

use anyhow::Context;
use arrayvec::ArrayVec;
use engine::resources::{
    texture::{TextureAsset, TextureMipLevel, MAX_MIPS},
    TextureChunkDescriptor, TEXTURE_CHUNK_DIMENSIONS, TEXTURE_CHUNK_FORMAT,
};
use image::{imageops::FilterType, load_from_memory, DynamicImage};
use pixels::TexPixels;
use tracing::trace;

use crate::database::RelatedChunkData;

/// Bytes per pixel in the texture chunk format, the only format used within
/// this module.
const BPP: usize = TEXTURE_CHUNK_FORMAT.bytes_per_pixel();
const CHUNK_WIDTH: usize = TEXTURE_CHUNK_DIMENSIONS.0 as usize;
const CHUNK_HEIGHT: usize = TEXTURE_CHUNK_DIMENSIONS.1 as usize;
const CHUNK_STRIDE: usize = CHUNK_WIDTH * BPP;
const CHUNK_BYTES: usize = CHUNK_STRIDE * CHUNK_HEIGHT;

pub fn import(image_path: &Path, db: &mut RelatedChunkData) -> anyhow::Result<TextureAsset> {
    let image_bytes = fs::read(image_path).context("Failed to open texture file for importing")?;
    let image = load_from_memory(&image_bytes)
        .context("Failed to read image file as an image (unsupported format?)")?;

    let width = image.width() as u16;
    let height = image.height() as u16;

    if width * height == 0 {
        return Err(anyhow::anyhow!("Texture must have at least one pixel"));
    }

    let mut transparent = false;
    let mut pending_chunk_width = 0;
    let mut pending_chunk_height = 0;
    let mut pending_pixels = std::vec![0; CHUNK_BYTES];
    let mut pending_chunk_tex =
        TexPixels::new(&mut pending_pixels, CHUNK_STRIDE, CHUNK_WIDTH, CHUNK_HEIGHT).unwrap();
    let mut pending_chunk_index = db.texture_chunks.len() as u32;

    // Writes out the pending chunk into `output_chunks` and `chunk_data`.
    let mut flush_pending_chunk = |width: usize,
                                   height: usize,
                                   tex: &TexPixels,
                                   chunk_index: &mut u32| {
        let start = db.chunk_data.stream_position().unwrap();
        for y in 0..height {
            db.chunk_data.write_all(&tex.row(y)[..width * BPP]).unwrap();
        }
        let end = db.chunk_data.stream_position().unwrap();
        trace!("Writing out a {width}x{height} texture chunk at (this asset's) chunk index {chunk_index} and byte range {start}..{end}.");
        db.texture_chunks.push(TextureChunkDescriptor {
            region_width: width as u16,
            region_height: height as u16,
            source_bytes: start..end,
        });
        *chunk_index += 1;
    };

    // Allocates space from the texture chunk (or multiple, if needed), and
    // writes out the relevant chunks' data.
    let mut allocate = |mut tex: pixels::TexPixels| -> TextureMipLevel {
        trace!("Allocating texture chunks for: {tex:?}");

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
        let mut first_chunk = None;
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
                if pending_chunk_width > 0 {
                    flush_pending_chunk(
                        pending_chunk_width,
                        pending_chunk_height,
                        &pending_chunk_tex,
                        &mut pending_chunk_index,
                    );
                    pending_chunk_tex.pixels.fill(0);
                }

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

                if first_chunk.is_none() {
                    first_chunk = Some(pending_chunk_index);
                }
            }
        }

        let first_chunk = first_chunk.unwrap();
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
        render_texture(&image, tex.width, tex.height, tex.stride, tex.pixels);
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

    Ok(TextureAsset {
        transparent,
        mip_chain,
    })
}

fn render_texture(
    image: &DynamicImage,
    width: usize,
    height: usize,
    stride: usize,
    pixels: &mut [u8],
) {
    assert_eq!(
        4, BPP,
        "texture import logic needs updating for non-rgba engine texture format"
    );
    let image = image.resize_exact(width as u32, height as u32, FilterType::CatmullRom);
    let image = image.into_rgba8();
    for y in 0..height {
        for x in 0..width {
            let [r, g, b, a] = image.get_pixel(x as u32, y as u32).0;
            pixels[x * BPP + y * stride] = r;
            pixels[x * BPP + 1 + y * stride] = g;
            pixels[x * BPP + 2 + y * stride] = b;
            pixels[x * BPP + 3 + y * stride] = a;
        }
    }
}
