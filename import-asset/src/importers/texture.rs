use std::{fs, path::Path};

use anyhow::Context;
use engine::resources::{assets::TextureAsset, TEXTURE_CHUNK_FORMAT};
use image::{imageops::FilterType, load_from_memory};

use crate::database::Database;

pub fn import(image_path: &Path, db: &mut Database) -> anyhow::Result<TextureAsset> {
    let image_bytes = fs::read(image_path).context("Failed to open texture file for importing")?;
    let image = load_from_memory(&image_bytes)
        .context("Failed to read image file as an image (unsupported format?)")?;

    let width = image.width() as u16;
    let height = image.height() as u16;
    Ok(TextureAsset::create(
        width,
        height,
        |w, h, stride, pixels| {
            const BPP: usize = TEXTURE_CHUNK_FORMAT.bytes_per_pixel();
            assert_eq!(
                4, BPP,
                "texture import logic needs updating for non-rgba engine texture format"
            );
            let image = image.resize_exact(w as u32, h as u32, FilterType::CatmullRom);
            let image = image.into_rgba8();
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let [r, g, b, a] = image.get_pixel(x as u32, y as u32).0;
                    pixels[x * BPP + y * stride] = r;
                    pixels[x * BPP + 1 + y * stride] = g;
                    pixels[x * BPP + 2 + y * stride] = b;
                    pixels[x * BPP + 3 + y * stride] = a;
                }
            }
        },
        &mut db.chunk_data,
        &mut db.texture_chunk_descriptors,
    ))
}
