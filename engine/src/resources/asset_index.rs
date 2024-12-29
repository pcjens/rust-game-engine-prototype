use core::cmp::Ordering;

use arrayvec::ArrayString;
use platform_abstraction_layer::{FileHandle, FileReadTask, Pal};

use crate::{FixedVec, LinearAllocator};

use super::{
    assets::{AudioClipAsset, TextureAsset},
    chunks::{ChunkDescriptor, TextureChunkDescriptor},
    deserialize, Deserialize,
};

pub const ASSET_NAME_LENGTH: usize = 27;

/// Wrapper for assets with their unique name. Implement equality and comparison
/// operators purely based on the name, as assets with a specific name should be
/// unique within a resource database.
pub struct NamedAsset<T> {
    pub name: ArrayString<ASSET_NAME_LENGTH>,
    pub asset: T,
}
impl<T> PartialEq for NamedAsset<T> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl<T> Eq for NamedAsset<T> {} // The equality operator just checks the name, and ArrayString is Eq.
impl<T> PartialOrd for NamedAsset<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for NamedAsset<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

#[derive(Clone, Copy)]
pub struct AssetIndexHeader {
    pub chunks: u32,
    pub texture_chunks: u32,
    pub textures: u32,
    pub audio_clips: u32,
}

pub struct AssetIndex<'eng> {
    pub chunks: FixedVec<'eng, ChunkDescriptor>,
    pub texture_chunks: FixedVec<'eng, TextureChunkDescriptor>,
    pub chunk_data_file: FileHandle,
    pub chunk_data_offset: u64,

    pub textures: FixedVec<'eng, NamedAsset<TextureAsset>>,
    pub audio_clips: FixedVec<'eng, NamedAsset<AudioClipAsset>>,
}

impl AssetIndex<'_> {
    pub fn new<'eng>(
        platform: &dyn Pal,
        arena: &'eng LinearAllocator,
        temp_arena: &LinearAllocator,
        file: FileHandle,
    ) -> Option<AssetIndex<'eng>> {
        let mut header_bytes = [0; AssetIndexHeader::SERIALIZED_SIZE];
        let header_read = platform.begin_file_read(file, 0, &mut header_bytes);
        let header_bytes = blocking_read_file(platform, header_read).ok()?;

        let mut cursor = 0;

        let AssetIndexHeader {
            chunks,
            texture_chunks,
            textures,
            audio_clips,
        } = deserialize::<AssetIndexHeader>(header_bytes, &mut cursor);

        let mut buffer = alloc_file_buf::<ChunkDescriptor>(temp_arena, chunks)?;
        let chunks = platform.begin_file_read(file, cursor as u64, &mut buffer);
        cursor += chunks.read_size();

        let mut buffer = alloc_file_buf::<TextureChunkDescriptor>(temp_arena, texture_chunks)?;
        let texture_chunks = platform.begin_file_read(file, cursor as u64, &mut buffer);
        cursor += texture_chunks.read_size();

        let mut buffer = alloc_file_buf::<NamedAsset<TextureAsset>>(temp_arena, textures)?;
        let textures = platform.begin_file_read(file, cursor as u64, &mut buffer);
        cursor += textures.read_size();

        let mut buffer = alloc_file_buf::<NamedAsset<AudioClipAsset>>(temp_arena, audio_clips)?;
        let audio_clips = platform.begin_file_read(file, cursor as u64, &mut buffer);
        cursor += audio_clips.read_size();

        let chunk_data_offset = cursor as u64;

        Some(AssetIndex {
            chunks: deserialize_from_file(platform, arena, chunks)?,
            texture_chunks: deserialize_from_file(platform, arena, texture_chunks)?,
            textures: sorted(deserialize_from_file(platform, arena, textures)?),
            audio_clips: sorted(deserialize_from_file(platform, arena, audio_clips)?),
            chunk_data_file: file,
            chunk_data_offset,
        })
    }
}

fn sorted<T: Ord>(mut input: FixedVec<'_, T>) -> FixedVec<'_, T> {
    input.sort_unstable();
    input
}

fn deserialize_from_file<'eng, D: Deserialize>(
    platform: &dyn Pal,
    alloc: &'eng LinearAllocator,
    file_read: FileReadTask,
) -> Option<FixedVec<'eng, D>> {
    let file_bytes = blocking_read_file(platform, file_read).ok()?;
    let count = file_bytes.len() / D::SERIALIZED_SIZE;
    let mut vec = FixedVec::new(alloc, count)?;
    for element_bytes in file_bytes.chunks(D::SERIALIZED_SIZE) {
        let Ok(_) = vec.push(D::deserialize(element_bytes)) else {
            unreachable!()
        };
    }
    Some(vec)
}

fn alloc_file_buf<'a, D: Deserialize>(
    temp_allocator: &'a LinearAllocator,
    count: u32,
) -> Option<FixedVec<'a, u8>> {
    let file_size = count as usize * D::SERIALIZED_SIZE;
    let mut file_bytes = FixedVec::<u8>::new(temp_allocator, file_size)?;
    file_bytes.fill_with_zeroes();
    Some(file_bytes)
}

fn blocking_read_file<'a>(
    platform: &dyn Pal,
    mut task: FileReadTask<'a>,
) -> Result<&'a mut [u8], ()> {
    loop {
        match platform.poll_file_read(task) {
            Ok(result) => return Ok(result),
            Err(None) => return Err(()),
            Err(Some(returned_task)) => task = returned_task,
        }
    }
}
