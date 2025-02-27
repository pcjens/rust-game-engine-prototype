// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod assets;
mod chunks;
mod deserialize;
mod loader;
mod serialize;

use assets::{audio_clip::AudioClipAsset, texture::TextureAsset};
use platform::{Box, FileHandle, FileReadTask, PixelFormat, Platform, AUDIO_CHANNELS};

pub use assets::*;
pub use chunks::{ChunkData, ChunkDescriptor, TextureChunkData, TextureChunkDescriptor};
pub use deserialize::{deserialize, Deserialize};
pub use loader::ResourceLoader;
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

/// The amount of audio samples that fit in each chunk.
pub const AUDIO_SAMPLES_PER_CHUNK: usize = CHUNK_SIZE as usize / size_of::<[i16; AUDIO_CHANNELS]>();

/// Basic info about a [`ResourceDatabase`] used in its initialization and for
/// de/serializing the db file.
#[derive(Clone, Copy)]
pub struct ResourceDatabaseHeader {
    /// The amount of regular chunks in the database.
    pub chunks: u32,
    /// The amount of texture chunks in the database.
    pub texture_chunks: u32,
    /// The amount of [`TextureAsset`]s in the database.
    pub textures: u32,
    /// The amount of [`AudioClipAsset`]s in the database.
    pub audio_clips: u32,
}

impl ResourceDatabaseHeader {
    /// Returns the byte offset into the resource database file where the chunks
    /// start.
    ///
    /// This is the size of the header, chunk descriptors, and asset metadata.
    pub const fn chunk_data_offset(&self) -> u64 {
        use serialize::Serialize as Ser;
        <ResourceDatabaseHeader as Ser>::SERIALIZED_SIZE as u64
            + self.chunks as u64 * <ChunkDescriptor as Ser>::SERIALIZED_SIZE as u64
            + self.texture_chunks as u64 * <TextureChunkDescriptor as Ser>::SERIALIZED_SIZE as u64
            + self.textures as u64 * <NamedAsset<TextureAsset> as Ser>::SERIALIZED_SIZE as u64
            + self.audio_clips as u64 * <NamedAsset<AudioClipAsset> as Ser>::SERIALIZED_SIZE as u64
    }
}

/// The resource database.
///
/// Game code should mostly use this for the `find_*` and `get_*` functions to
/// query for assets, which implement the relevant logic for each asset type.
pub struct ResourceDatabase {
    // Asset metadata
    textures: FixedVec<'static, NamedAsset<TextureAsset>>,
    audio_clips: FixedVec<'static, NamedAsset<AudioClipAsset>>,
    // Chunk loading metadata
    chunk_data_file: FileHandle,
    chunk_data_offset: u64,
    chunk_descriptors: FixedVec<'static, ChunkDescriptor>,
    texture_chunk_descriptors: FixedVec<'static, TextureChunkDescriptor>,
    // In-memory chunks
    /// The regular chunks currently loaded in-memory. Loaded via
    /// [`ResourceLoader`], usually by functions making use of an asset.
    // TODO: expose chunks via getters that maintain LRU timestamps for eviction
    pub chunks: SparseArray<'static, ChunkData>,
    /// The texture chunks currently loaded in-memory. Loaded via
    /// [`ResourceLoader`], usually by functions making use of an asset.
    pub texture_chunks: SparseArray<'static, TextureChunkData>,
}

impl ResourceDatabase {
    pub(crate) fn new(
        platform: &dyn Platform,
        arena: &'static LinearAllocator,
        file: FileHandle,
        max_loaded_chunks: u32,
        max_loaded_texture_chunks: u32,
    ) -> Option<ResourceDatabase> {
        // TODO: replace all these memory-leaky persistent allocations with some
        // file reading utility that uses a ring buffer to allocate the Boxes
        // and reclaims memory.
        let header_bytes = arena
            .try_alloc_boxed_slice_zeroed(<ResourceDatabaseHeader as Deserialize>::SERIALIZED_SIZE)
            .expect("arena should have enough space for resource db header");
        let header_read = platform.begin_file_read(file, 0, header_bytes);
        let header_bytes = platform
            .finish_file_read(header_read)
            .expect("resource database file's header should be readable");

        let mut cursor = 0;

        let header = deserialize::<ResourceDatabaseHeader>(&header_bytes, &mut cursor);
        // FIXME: header_bytes is leaked here

        let chunk_data_offset = header.chunk_data_offset();
        let ResourceDatabaseHeader {
            chunks,
            texture_chunks,
            textures,
            audio_clips,
        } = header;

        let buffer = alloc_file_buf::<ChunkDescriptor>(arena, chunks)?;
        let chunk_descriptors = platform.begin_file_read(file, cursor as u64, buffer);
        cursor += chunk_descriptors.read_size();

        let buffer = alloc_file_buf::<TextureChunkDescriptor>(arena, texture_chunks)?;
        let tex_chunk_descs = platform.begin_file_read(file, cursor as u64, buffer);
        cursor += tex_chunk_descs.read_size();

        let buffer = alloc_file_buf::<NamedAsset<TextureAsset>>(arena, textures)?;
        let textures = platform.begin_file_read(file, cursor as u64, buffer);
        cursor += textures.read_size();

        let buffer = alloc_file_buf::<NamedAsset<AudioClipAsset>>(arena, audio_clips)?;
        let audio_clips = platform.begin_file_read(file, cursor as u64, buffer);

        Some(ResourceDatabase {
            chunk_data_file: file,
            chunk_data_offset,
            chunks: SparseArray::new(arena, chunks, max_loaded_chunks)?,
            texture_chunks: SparseArray::new(arena, texture_chunks, max_loaded_texture_chunks)?,
            chunk_descriptors: deserialize_from_file(arena, chunk_descriptors, platform)?,
            texture_chunk_descriptors: deserialize_from_file(arena, tex_chunk_descs, platform)?,
            textures: sorted(deserialize_from_file(arena, textures, platform)?),
            audio_clips: sorted(deserialize_from_file(arena, audio_clips, platform)?),
        })
    }

    /// Returns the longest source bytes length of all the chunks, i.e. the
    /// minimum amount of staging memory required to be able to load any chunk
    /// in this database.
    pub fn largest_chunk_source(&self) -> u64 {
        let largest_chunk_source = (self.chunk_descriptors.iter())
            .map(|chunk| chunk.source_bytes.end - chunk.source_bytes.start)
            .max()
            .unwrap_or(0);
        let largest_texture_chunk_source = (self.texture_chunk_descriptors.iter())
            .map(|chunk| chunk.source_bytes.end - chunk.source_bytes.start)
            .max()
            .unwrap_or(0);
        largest_chunk_source.max(largest_texture_chunk_source)
    }
}

fn sorted<T: Ord>(mut input: FixedVec<'_, T>) -> FixedVec<'_, T> {
    input.sort_unstable();
    input
}

fn deserialize_from_file<'eng, D: Deserialize>(
    alloc: &'eng LinearAllocator,
    file_read: FileReadTask,
    platform: &dyn Platform,
) -> Option<FixedVec<'eng, D>> {
    let file_bytes = platform
        .finish_file_read(file_read)
        .expect("resource database file's index should be readable");
    let count = file_bytes.len() / D::SERIALIZED_SIZE;
    let mut vec = FixedVec::new(alloc, count)?;
    for element_bytes in file_bytes.chunks(D::SERIALIZED_SIZE) {
        let Ok(_) = vec.push(D::deserialize(element_bytes)) else {
            unreachable!()
        };
    }
    // FIXME: file_bytes is leaked here
    Some(vec)
}

fn alloc_file_buf<D: Deserialize>(
    arena: &'static LinearAllocator,
    count: u32,
) -> Option<Box<[u8]>> {
    let file_size = count as usize * D::SERIALIZED_SIZE;
    arena.try_alloc_boxed_slice_zeroed(file_size)
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
    #[derive(Debug)]
    pub struct NamedAsset<T> {
        /// The unique name of the asset.
        pub name: ArrayString<ASSET_NAME_LENGTH>,
        /// The asset itself.
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
