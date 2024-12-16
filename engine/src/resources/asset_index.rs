use platform_abstraction_layer::{FileHandle, Pal};

use crate::{FixedVec, LinearAllocator};

use super::{
    assets::TextureChunkDescriptor, deserialize, AudioClipAsset, ChunkDescriptor, Deserialize,
    TextureAsset, RESOURCE_DB_MAGIC_NUMBER,
};

#[derive(Clone, Copy)]
pub struct AssetIndexHeader {
    pub chunks: u32,
    pub texture_chunks: u32,
    pub textures: u32,
    pub audio_clips: u32,
}

pub struct AssetIndex<'eng> {
    pub chunks: FixedVec<'eng, ChunkDescriptor<'eng>>,
    pub texture_chunks: FixedVec<'eng, TextureChunkDescriptor<'eng>>,
    pub textures: FixedVec<'eng, TextureAsset>,
    pub audio_clips: FixedVec<'eng, AudioClipAsset>,
}

impl AssetIndex<'_> {
    pub fn new<'eng>(
        platform: &dyn Pal,
        arena: &'eng LinearAllocator,
        temp_arena: &LinearAllocator,
        file: FileHandle,
    ) -> Option<AssetIndex<'eng>> {
        let mut header_bytes = [0; u32::SERIALIZED_SIZE + AssetIndexHeader::SERIALIZED_SIZE];
        blocking_read_file(platform, file, 0, &mut header_bytes).ok()?;

        let mut c = 0;
        let magic = deserialize::<u32>(&header_bytes, &mut c);
        if magic != RESOURCE_DB_MAGIC_NUMBER {
            return None;
        }
        let AssetIndexHeader {
            chunks,
            texture_chunks,
            textures,
            audio_clips,
        } = deserialize::<AssetIndexHeader>(&header_bytes, &mut c);

        Some(AssetIndex {
            chunks: read_array(platform, arena, temp_arena, file, &mut c, chunks)?,
            texture_chunks: read_array(platform, arena, temp_arena, file, &mut c, texture_chunks)?,
            textures: read_array(platform, arena, temp_arena, file, &mut c, textures)?,
            audio_clips: read_array(platform, arena, temp_arena, file, &mut c, audio_clips)?,
        })
    }
}

fn read_array<'eng, D: Deserialize>(
    platform: &dyn Pal,
    alloc: &'eng LinearAllocator,
    temp_allocator: &LinearAllocator,
    file: FileHandle,
    cursor: &mut usize,
    count: u32,
) -> Option<FixedVec<'eng, D>> {
    let file_size = count as usize * D::SERIALIZED_SIZE;
    let mut file_bytes = FixedVec::<u8>::new(temp_allocator, file_size)?;
    file_bytes.fill_with_zeroes();
    blocking_read_file(platform, file, *cursor as u64, &mut file_bytes).ok()?;
    *cursor += file_size;

    let mut vec = FixedVec::new(alloc, count as usize)?;
    for element_bytes in file_bytes.chunks(D::SERIALIZED_SIZE) {
        let Ok(_) = vec.push(D::deserialize(element_bytes)) else {
            unreachable!()
        };
    }
    Some(vec)
}

fn blocking_read_file(
    platform: &dyn Pal,
    file: FileHandle,
    first_byte: u64,
    buffer: &mut [u8],
) -> Result<(), ()> {
    let mut task = platform.begin_file_read(file, first_byte, buffer);
    loop {
        match platform.poll_file_read(task) {
            Ok(_) => return Ok(()),
            Err(None) => return Err(()),
            Err(Some(returned_task)) => task = returned_task,
        }
    }
}
