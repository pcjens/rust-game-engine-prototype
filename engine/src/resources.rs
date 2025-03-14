// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod assets;
mod chunks;
mod deserialize;
mod file_reader;
mod loader;
mod serialize;

use assets::{audio_clip::AudioClipAsset, sprite::SpriteAsset};
use platform::{PixelFormat, Platform, AUDIO_CHANNELS};

pub use assets::*;
pub use chunks::{ChunkData, ChunkDescriptor, SpriteChunkData, SpriteChunkDescriptor};
pub use deserialize::{deserialize, Deserialize};
pub use file_reader::FileReader;
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
/// Width and height of the dynamically allocated sprite chunks.
pub const SPRITE_CHUNK_DIMENSIONS: (u16, u16) = (128, 128);
/// Pixel format of the dynamically allocated sprite chunks.
pub const SPRITE_CHUNK_FORMAT: PixelFormat = PixelFormat::Rgba;

/// The amount of audio samples that fit in each chunk.
pub const AUDIO_SAMPLES_PER_CHUNK: usize = CHUNK_SIZE as usize / size_of::<[i16; AUDIO_CHANNELS]>();

/// Basic info about a [`ResourceDatabase`] used in its initialization and for
/// de/serializing the db file.
#[derive(Clone, Copy)]
pub struct ResourceDatabaseHeader {
    /// The amount of regular chunks in the database.
    pub chunks: u32,
    /// The amount of sprite chunks in the database.
    pub sprite_chunks: u32,
    /// The amount of [`SpriteAsset`]s in the database.
    pub sprites: u32,
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
            + self.sprite_chunks as u64 * <SpriteChunkDescriptor as Ser>::SERIALIZED_SIZE as u64
            + self.sprites as u64 * <NamedAsset<SpriteAsset> as Ser>::SERIALIZED_SIZE as u64
            + self.audio_clips as u64 * <NamedAsset<AudioClipAsset> as Ser>::SERIALIZED_SIZE as u64
    }
}

/// The resource database.
///
/// Game code should mostly use this for the `find_*` and `get_*` functions to
/// query for assets, which implement the relevant logic for each asset type.
pub struct ResourceDatabase {
    // Asset metadata
    sprites: FixedVec<'static, NamedAsset<SpriteAsset>>,
    audio_clips: FixedVec<'static, NamedAsset<AudioClipAsset>>,
    // Chunk loading metadata
    chunk_data_offset: u64,
    chunk_descriptors: FixedVec<'static, ChunkDescriptor>,
    sprite_chunk_descriptors: FixedVec<'static, SpriteChunkDescriptor>,
    // In-memory chunks
    /// The regular chunks currently loaded in-memory. Loaded via
    /// [`ResourceLoader`], usually by functions making use of an asset.
    pub chunks: SparseArray<'static, ChunkData>,
    /// The sprite chunks currently loaded in-memory. Loaded via
    /// [`ResourceLoader`], usually by functions making use of an asset.
    pub sprite_chunks: SparseArray<'static, SpriteChunkData>,
}

impl ResourceDatabase {
    pub(crate) fn new(
        platform: &dyn Platform,
        arena: &'static LinearAllocator,
        file_reader: &mut FileReader,
        max_loaded_chunks: u32,
        max_loaded_sprite_chunks: u32,
    ) -> Option<ResourceDatabase> {
        profiling::function_scope!();
        use Deserialize as De;
        let header_size = <ResourceDatabaseHeader as De>::SERIALIZED_SIZE;

        assert!(file_reader.push_read(0, header_size));
        let header = file_reader
            .pop_read(platform, true, |header_bytes| {
                deserialize::<ResourceDatabaseHeader>(header_bytes, &mut 0)
            })
            .expect("resource database file should be readable");

        let chunk_data_offset = header.chunk_data_offset();
        let ResourceDatabaseHeader {
            chunks,
            sprite_chunks,
            sprites,
            audio_clips,
        } = header;

        let mut cursor = header_size;
        let mut queue_read = |size: usize| {
            assert!(file_reader.push_read(cursor as u64, size));
            cursor += size;
        };

        queue_read(chunks as usize * <ChunkDescriptor as De>::SERIALIZED_SIZE);
        queue_read(sprite_chunks as usize * <SpriteChunkDescriptor as De>::SERIALIZED_SIZE);
        queue_read(sprites as usize * <NamedAsset<SpriteAsset> as De>::SERIALIZED_SIZE);
        queue_read(audio_clips as usize * <NamedAsset<AudioClipAsset> as De>::SERIALIZED_SIZE);

        // NOTE: These deserialize_vec calls must be in the same order as the queue_reads above.
        let chunk_descriptors = deserialize_vec(arena, file_reader, platform)?;
        let sprite_chunk_descriptors = deserialize_vec(arena, file_reader, platform)?;
        let sprites = sorted(deserialize_vec(arena, file_reader, platform)?);
        let audio_clips = sorted(deserialize_vec(arena, file_reader, platform)?);

        Some(ResourceDatabase {
            sprites,
            audio_clips,
            chunk_data_offset,
            chunk_descriptors,
            sprite_chunk_descriptors,
            chunks: SparseArray::new(arena, chunks, max_loaded_chunks)?,
            sprite_chunks: SparseArray::new(arena, sprite_chunks, max_loaded_sprite_chunks)?,
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
        let largest_sprite_chunk_source = (self.sprite_chunk_descriptors.iter())
            .map(|chunk| chunk.source_bytes.end - chunk.source_bytes.start)
            .max()
            .unwrap_or(0);
        largest_chunk_source.max(largest_sprite_chunk_source)
    }
}

fn sorted<T: Ord>(mut input: FixedVec<'_, T>) -> FixedVec<'_, T> {
    input.sort_unstable();
    input
}

fn deserialize_vec<'a, D: Deserialize>(
    alloc: &'a LinearAllocator,
    file_reader: &mut FileReader,
    platform: &dyn Platform,
) -> Option<FixedVec<'a, D>> {
    file_reader
        .pop_read(platform, true, |src| {
            let count = src.len() / D::SERIALIZED_SIZE;
            let mut vec = FixedVec::new(alloc, count)?;
            assert_eq!(0, vec.len() % D::SERIALIZED_SIZE);
            for element_bytes in src.chunks_exact(D::SERIALIZED_SIZE) {
                let Ok(_) = vec.push(D::deserialize(element_bytes)) else {
                    unreachable!()
                };
            }
            Some(vec)
        })
        .expect("resource db file header should be readable")
}

pub use named_asset::{NamedAsset, ASSET_NAME_LENGTH};
mod named_asset {
    use core::cmp::Ordering;

    use arrayvec::ArrayString;

    /// Maximum length for the unique names of assets.
    pub const ASSET_NAME_LENGTH: usize = 27;

    /// A unique name and a `T`. Used in
    /// [`ResourceDatabase`](super::ResourceDatabase) and when creating the db
    /// file.
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
