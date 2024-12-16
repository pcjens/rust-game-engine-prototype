use std::fs;

use engine::resources::{
    serialize, AssetIndexHeader, TextureAsset, TextureChunkDescriptor, RESOURCE_DB_MAGIC_NUMBER,
};

fn main() {
    let mut dst = vec![0; 1000];

    let header = AssetIndexHeader {
        chunks: 0,
        texture_chunks: 1,
        textures: 1,
        audio_clips: 0,
    };

    let texture_size: usize = 2 * 2 * 4;
    let texture_chunk = TextureChunkDescriptor {
        region_width: 2,
        region_height: 2,
        source_bytes: 0..texture_size as u64,
        resident: None,
    };

    let texture = TextureAsset {
        width: 2,
        height: 2,
        texture_chunks: 0..1,
    };

    let mut cursor = 0;
    serialize(&RESOURCE_DB_MAGIC_NUMBER, &mut dst, &mut cursor);
    serialize(&header, &mut dst, &mut cursor);
    serialize(&texture_chunk, &mut dst, &mut cursor);
    serialize(&texture, &mut dst, &mut cursor);
    dst[cursor..cursor + texture_size].copy_from_slice(&[
        0xFF, 0xFF, 0x00, 0xFF, // Yellow
        0xFF, 0x00, 0xFF, 0xFF, // Pink
        0x00, 0xFF, 0x00, 0xFF, // Green
        0x00, 0xFF, 0xFF, 0xFF, // Cyan
    ]);
    cursor += texture_size;

    fs::write("resources.db", &dst[..cursor]).unwrap();

    println!("This does not import assets yet. Wrote a resources.db for testing though.");
}
