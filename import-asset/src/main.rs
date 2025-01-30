use std::{fs, io::Cursor, str::FromStr};

use arrayvec::ArrayString;
use engine::resources::{
    assets::TextureAsset, serialize, NamedAsset, ResourceDatabaseHeader, TextureChunkDescriptor,
    TEXTURE_CHUNK_FORMAT,
};

fn main() {
    let mut dst = vec![0; 1000];

    let header = ResourceDatabaseHeader {
        chunks: 0,
        texture_chunks: 1,
        textures: 1,
        audio_clips: 0,
    };

    let mut chunk_data: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut texture_chunks: Vec<TextureChunkDescriptor> = Vec::new();

    let texture = NamedAsset {
        name: ArrayString::from_str("testing texture").unwrap(),
        asset: TextureAsset::create(
            2,
            2,
            |w, h, stride, pixels| {
                const BPP: usize = TEXTURE_CHUNK_FORMAT.bytes_per_pixel();
                const PIXELS: [u8; 2 * 2 * BPP] = [
                    0xFF, 0xFF, 0x00, 0xFF, // Yellow
                    0xFF, 0x00, 0xFF, 0xFF, // Pink
                    0x00, 0xFF, 0x00, 0xFF, // Green
                    0x00, 0xFF, 0xFF, 0xFF, // Cyan
                ];
                const PIXEL_STRIDE: usize = 2 * BPP;
                assert_eq!(2, w);
                assert_eq!(2, h);
                for y in 0..h as usize {
                    pixels[y * stride..w as usize * BPP + y * stride]
                        .copy_from_slice(&PIXELS[y * PIXEL_STRIDE..(y + 1) * PIXEL_STRIDE]);
                }
            },
            &mut chunk_data,
            &mut texture_chunks,
        ),
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
