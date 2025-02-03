use std::{fs, io::Cursor, str::FromStr};

use arrayvec::ArrayString;
use engine::resources::{
    assets::TextureAsset, serialize, NamedAsset, ResourceDatabaseHeader, TextureChunkDescriptor,
    TEXTURE_CHUNK_FORMAT,
};
use image::imageops::FilterType;

fn main() {
    let mut dst = vec![0; 1_000_000];

    let mut chunk_data: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut texture_chunks: Vec<TextureChunkDescriptor> = Vec::new();

    let texture = {
        let image =
            image::load_from_memory(include_bytes!("../../example/resources/kellot.jpeg")).unwrap();
        let width = image.width() as u16;
        let height = image.height() as u16;
        NamedAsset {
            name: ArrayString::from_str("testing texture").unwrap(),
            asset: TextureAsset::create(
                width,
                height,
                |w, h, stride, pixels| {
                    const BPP: usize = TEXTURE_CHUNK_FORMAT.bytes_per_pixel();
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
                &mut chunk_data,
                &mut texture_chunks,
            ),
        }
    };

    let header = ResourceDatabaseHeader {
        chunks: 0,
        texture_chunks: texture_chunks.len() as u32,
        textures: 1,
        audio_clips: 0,
    };

    let mut cursor = 0;
    // Header
    serialize(&header, &mut dst, &mut cursor);
    // Texture chunks
    for texture_chunk in &texture_chunks {
        serialize(texture_chunk, &mut dst, &mut cursor);
    }
    // Assets
    serialize(&texture, &mut dst, &mut cursor);
    // Chunk data
    let chunk_data = chunk_data.into_inner();
    dst[cursor..cursor + chunk_data.len()].copy_from_slice(&chunk_data);
    cursor += chunk_data.len();

    fs::write("resources.db", &dst[..cursor]).unwrap();

    println!("This does not import assets yet. Wrote a resources.db for testing though.");
}
