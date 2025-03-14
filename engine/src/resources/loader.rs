// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use platform::Platform;

use crate::{allocators::LinearAllocator, collections::Queue};

use super::{
    file_reader::{FileReadError, FileReader},
    ChunkData, ResourceDatabase, SpriteChunkData,
};

#[derive(Debug, PartialEq, Eq)]
enum LoadCategory {
    Chunk,
    SpriteChunk,
}

#[derive(Debug)]
struct ChunkReadInfo {
    chunk_index: u32,
    category: LoadCategory,
}

/// Asynchronous loader for resource chunks.
///
/// Holds some staging memory where the chunk data is written by
/// platform-defined asynchronous file reading utilities after
/// [`ResourceLoader::dispatch_reads`]. The chunk data is read later to
/// initialize chunks in [`ResourceLoader::finish_reads`]. Chunks are loaded in
/// the order [`ResourceLoader::queue_chunk`] and
/// [`ResourceLoader::queue_sprite_chunk`] are called.
///
/// Many asset usage related functions take this struct as a parameter for
/// queueing up relevant chunks to be loaded.
pub struct ResourceLoader {
    file_reader: FileReader,
    queued_reads: Queue<'static, ChunkReadInfo>,
}

impl ResourceLoader {
    /// Creates a resource loader around the file reader.
    ///
    /// The file reader's `staging_buffer_size` should be at least
    /// [`ResourceDatabase::largest_chunk_source`].
    #[track_caller]
    pub fn new(
        arena: &'static LinearAllocator,
        file_reader: FileReader,
        resource_db: &ResourceDatabase,
    ) -> Option<ResourceLoader> {
        assert!(
            file_reader.staging_buffer_size() as u64 >= resource_db.largest_chunk_source(),
            "resource loader file reader's staging buffer size is smaller than the resource database's largest chunk source",
        );

        let total_chunks = resource_db.chunks.array_len() + resource_db.sprite_chunks.array_len();
        Some(ResourceLoader {
            file_reader,
            queued_reads: Queue::new(arena, total_chunks)?,
        })
    }

    /// Queues the regular chunk at `chunk_index` to be loaded.
    ///
    /// Note that this doesn't necessarily actually queue up a read operation,
    /// the chunk might not be queued for read if e.g. it's already been loaded,
    /// it's already been queued, or if the queue can't fit the request.
    pub fn queue_chunk(&mut self, chunk_index: u32, resources: &ResourceDatabase) {
        self.queue_load(chunk_index, LoadCategory::Chunk, resources);
    }

    /// Queues the sprite chunk at `chunk_index` to be loaded.
    ///
    /// Note that this doesn't necessarily actually queue up a read operation,
    /// the chunk might not be queued for read if e.g. it's already been loaded,
    /// it's already been queued, or if the queue can't fit the request.
    pub fn queue_sprite_chunk(&mut self, chunk_index: u32, resources: &ResourceDatabase) {
        self.queue_load(chunk_index, LoadCategory::SpriteChunk, resources);
    }

    fn queue_load(
        &mut self,
        chunk_index: u32,
        category: LoadCategory,
        resources: &ResourceDatabase,
    ) {
        profiling::function_scope!();

        // Don't queue if the chunk has already been loaded.
        if (category == LoadCategory::Chunk && resources.chunks.get(chunk_index).is_some())
            || (category == LoadCategory::SpriteChunk
                && resources.sprite_chunks.get(chunk_index).is_some())
        {
            return;
        }

        // Don't queue if the chunk has already been queued.
        let already_queued =
            |read: &ChunkReadInfo| read.chunk_index == chunk_index && read.category == category;
        if self.queued_reads.iter().any(already_queued) {
            return;
        }

        let chunk_source = match category {
            LoadCategory::Chunk => &resources.chunk_descriptors[chunk_index as usize].source_bytes,
            LoadCategory::SpriteChunk => {
                &resources.sprite_chunk_descriptors[chunk_index as usize].source_bytes
            }
        };
        let first_byte = resources.chunk_data_offset + chunk_source.start;
        let size = (chunk_source.end - chunk_source.start) as usize;
        // Attempt to queue:
        if !self.queued_reads.is_full() && self.file_reader.push_read(first_byte, size) {
            self.queued_reads
                .push_back(ChunkReadInfo {
                    chunk_index,
                    category,
                })
                .unwrap();
        }
    }

    /// Starts file read operations for the queued up chunk loading requests.
    pub fn dispatch_reads(&mut self, platform: &dyn Platform) {
        self.file_reader.dispatch_reads(platform);
    }

    /// Checks for finished file read requests and writes their results into the
    /// resource database.
    ///
    /// The `max_readers` parameter can be used to limit the time it takes to
    /// run this function when the queue has a lot of reads to process.
    pub fn finish_reads(
        &mut self,
        resources: &mut ResourceDatabase,
        platform: &dyn Platform,
        max_reads: usize,
    ) {
        profiling::function_scope!();
        for _ in 0..max_reads {
            let read_result = self.file_reader.pop_read(platform, false, |source_bytes| {
                profiling::scope!("process file read");
                let ChunkReadInfo {
                    chunk_index,
                    category,
                    ..
                } = self.queued_reads.pop_front().unwrap();

                match category {
                    LoadCategory::Chunk => {
                        let desc = &resources.chunk_descriptors[chunk_index as usize];
                        let init_fn = || Some(ChunkData::empty());
                        if let Some(dst) = resources.chunks.insert(chunk_index, init_fn) {
                            dst.update(desc, source_bytes);
                        }
                    }

                    LoadCategory::SpriteChunk => {
                        let desc = &resources.sprite_chunk_descriptors[chunk_index as usize];
                        let init_fn = || SpriteChunkData::empty(platform);
                        if let Some(dst) = resources.sprite_chunks.insert(chunk_index, init_fn) {
                            dst.update(desc, source_bytes, platform);
                        }
                    }
                }
            });

            match read_result {
                Ok(_) => {}
                Err(FileReadError::NoReadsQueued | FileReadError::WouldBlock) => break,
                Err(err) => {
                    let info = self.queued_reads.pop_front().unwrap();
                    platform.println(format_args!(
                        "resource loader read ({info:?}) failed: {err:?}"
                    ));
                }
            }
        }
    }
}
