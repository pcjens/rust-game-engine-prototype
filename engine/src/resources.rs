pub mod assets;
pub mod chunks;
mod deserialize;
mod serialize;

use assets::{AudioClipAsset, TextureAsset};
use chunks::{ChunkData, ChunkDescriptor, TextureChunkData, TextureChunkDescriptor};
use platform_abstraction_layer::{FileHandle, FileReadTask, Pal, PixelFormat};

pub use deserialize::{deserialize, Deserialize};
pub use serialize::{serialize, Serialize};

use crate::{
    allocators::LinearAllocator,
    collections::{FixedVec, SparseArray},
};

/// Magic number used when de/serializing [`ResourceDatabaseHeader`].
pub const RESOURCE_DB_MAGIC_NUMBER: u32 = 0xE97E6D00;
/// Amount of bytes in the regular dynamically allocated chunks.
pub const CHUNK_SIZE: u32 = 64 * 1024;
/// Width and height of the dynamically allocated texture chunks.
pub const TEXTURE_CHUNK_DIMENSIONS: (u16, u16) = (128, 128);
/// Pixel format of the dynamically allocated texture chunks.
pub const TEXTURE_CHUNK_FORMAT: PixelFormat = PixelFormat::Rgba;

/// Basic info about a [`ResourceDatabase`] used in its initialization and for
/// de/serializing the db file.
#[derive(Clone, Copy)]
pub struct ResourceDatabaseHeader {
    pub chunks: u32,
    pub texture_chunks: u32,
    pub textures: u32,
    pub audio_clips: u32,
}

/// The resource database.
///
/// The internals are exposed for `import-asset`, but game code should mostly
/// use this for the `find_*` and `get_*` functions to query for assets, which
/// implement the relevant logic for each asset type.
pub struct ResourceDatabase<'eng> {
    // Asset metadata
    pub textures: FixedVec<'eng, NamedAsset<TextureAsset>>,
    pub audio_clips: FixedVec<'eng, NamedAsset<AudioClipAsset>>,
    // Chunk loading metadata
    pub chunk_data_file: FileHandle,
    pub chunk_data_offset: u64,
    pub chunk_descriptors: FixedVec<'eng, ChunkDescriptor>,
    pub texture_chunk_descriptors: FixedVec<'eng, TextureChunkDescriptor>,
    // In-memory chunks
    pub chunks: SparseArray<'eng, ChunkData>,
    pub texture_chunks: SparseArray<'eng, TextureChunkData>,
}

impl ResourceDatabase<'_> {
    pub(crate) fn new<'eng>(
        platform: &dyn Pal,
        arena: &'eng LinearAllocator,
        temp_arena: &LinearAllocator,
        file: FileHandle,
        max_loaded_chunks: u32,
        max_loaded_texture_chunks: u32,
    ) -> Option<ResourceDatabase<'eng>> {
        let mut header_bytes = [0; <ResourceDatabaseHeader as Deserialize>::SERIALIZED_SIZE];
        let header_read = platform.begin_file_read(file, 0, &mut header_bytes);
        let header_bytes = blocking_read_file(platform, header_read).ok()?;

        let mut cursor = 0;

        let ResourceDatabaseHeader {
            chunks,
            texture_chunks,
            textures,
            audio_clips,
        } = deserialize::<ResourceDatabaseHeader>(header_bytes, &mut cursor);

        let mut buffer = alloc_file_buf::<ChunkDescriptor>(temp_arena, chunks)?;
        let chunk_descriptors = platform.begin_file_read(file, cursor as u64, &mut buffer);
        cursor += chunk_descriptors.read_size();

        let mut buffer = alloc_file_buf::<TextureChunkDescriptor>(temp_arena, texture_chunks)?;
        let tex_chunk_descs = platform.begin_file_read(file, cursor as u64, &mut buffer);
        cursor += tex_chunk_descs.read_size();

        let mut buffer = alloc_file_buf::<NamedAsset<TextureAsset>>(temp_arena, textures)?;
        let textures = platform.begin_file_read(file, cursor as u64, &mut buffer);
        cursor += textures.read_size();

        let mut buffer = alloc_file_buf::<NamedAsset<AudioClipAsset>>(temp_arena, audio_clips)?;
        let audio_clips = platform.begin_file_read(file, cursor as u64, &mut buffer);
        cursor += audio_clips.read_size();

        let chunk_data_offset = cursor as u64;

        Some(ResourceDatabase {
            chunk_data_file: file,
            chunk_data_offset,
            chunks: SparseArray::new(arena, chunks, max_loaded_chunks)?,
            texture_chunks: SparseArray::new(arena, texture_chunks, max_loaded_texture_chunks)?,
            chunk_descriptors: deserialize_from_file(platform, arena, chunk_descriptors)?,
            texture_chunk_descriptors: deserialize_from_file(platform, arena, tex_chunk_descs)?,
            textures: sorted(deserialize_from_file(platform, arena, textures)?),
            audio_clips: sorted(deserialize_from_file(platform, arena, audio_clips)?),
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

pub use named_asset::{NamedAsset, ASSET_NAME_LENGTH};
mod named_asset {
    use core::cmp::Ordering;

    use arrayvec::ArrayString;

    #[allow(unused_imports)] // used in docs
    use super::ResourceDatabase;

    /// Maximum length for the unique names of assets.
    pub const ASSET_NAME_LENGTH: usize = 27;

    /// A unique name and a `T`. Used in [`ResourceDatabase`] and when creating
    /// the db file.
    ///
    /// Implements equality and comparison operators purely based on the name,
    /// as assets with a specific name should be unique within a resource
    /// database.
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
}
