// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use platform_abstraction_layer::{FileReadTask, Pal};

use crate::{
    allocators::StaticAllocator,
    collections::{Queue, RingAllocationMetadata, RingBuffer, RingSlice},
};

use super::{ChunkData, ResourceDatabase, TextureChunkData};

#[derive(Debug, PartialEq, Eq)]
enum LoadCategory {
    Chunk,
    TextureChunk,
}

#[derive(Debug)]
struct LoadRequest {
    first_byte: u64,
    size: usize,
    chunk_index: u32,
    category: LoadCategory,
}

struct LoadTask {
    file_read_task: FileReadTask,
    read_buffer_metadata: RingAllocationMetadata,
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
/// [`ResourceLoader::queue_texture_chunk`] are called.
///
/// Many asset usage related functions take this struct as a parameter for
/// queueing up relevant chunks to be loaded.
pub struct ResourceLoader {
    staging_buffer: RingBuffer<'static, u8>,
    to_load_queue: Queue<'static, LoadRequest>,
    in_flight_queue: Queue<'static, LoadTask>,
}

impl ResourceLoader {
    /// Creates a resource loader with the given amount of staging memory.
    ///
    /// `staging_buffer_size` should be at least
    /// [`ResourceDatabase::largest_chunk_source`].
    #[track_caller]
    pub fn new(
        arena: &'static StaticAllocator,
        staging_buffer_size: usize,
        resource_db: &ResourceDatabase,
    ) -> Option<ResourceLoader> {
        assert!(
            staging_buffer_size as u64 >= resource_db.largest_chunk_source(),
            "staging_buffer_size is smaller than the resource database's largest_chunk_source()",
        );

        let total_chunks = resource_db.chunks.array_len() + resource_db.texture_chunks.array_len();
        Some(ResourceLoader {
            staging_buffer: RingBuffer::new(arena, staging_buffer_size)?,
            to_load_queue: Queue::new(arena, total_chunks)?,
            in_flight_queue: Queue::new(arena, total_chunks)?,
        })
    }

    /// Queues the regular chunk at `chunk_index` to be loaded.
    pub fn queue_chunk(&mut self, chunk_index: u32, resources: &ResourceDatabase) {
        self.queue_load(chunk_index, LoadCategory::Chunk, resources);
    }

    /// Queues the texture chunk at `chunk_index` to be loaded.
    pub fn queue_texture_chunk(&mut self, chunk_index: u32, resources: &ResourceDatabase) {
        self.queue_load(chunk_index, LoadCategory::TextureChunk, resources);
    }

    fn queue_load(
        &mut self,
        chunk_index: u32,
        category: LoadCategory,
        resources: &ResourceDatabase,
    ) {
        let chunk_source = &resources.texture_chunk_descriptors[chunk_index as usize].source_bytes;
        let chunk_size = (chunk_source.end - chunk_source.start) as usize;
        if (self.to_load_queue.iter())
            .any(|req| req.chunk_index == chunk_index && req.category == category)
            || (self.in_flight_queue.iter())
                .any(|req| req.chunk_index == chunk_index && req.category == category)
        {
            // Already in the queue or being read from the file.
            return;
        }
        self.to_load_queue
            .push_back(LoadRequest {
                first_byte: resources.chunk_data_offset + chunk_source.start,
                size: chunk_size,
                chunk_index,
                category,
            })
            .unwrap();
    }

    /// Starts file read operations for the queued up chunk loading requests.
    pub fn dispatch_reads(&mut self, resources: &ResourceDatabase, platform: &dyn Pal) {
        while let Some(LoadRequest { size, .. }) = self.to_load_queue.peek_front() {
            let Some(staging_slice) = self.staging_buffer.allocate(*size) else {
                break;
            };
            let (buffer, read_buffer_metadata) = staging_slice.into_parts();

            let LoadRequest {
                first_byte,
                size: _,
                chunk_index,
                category,
            } = self.to_load_queue.pop_front().unwrap();

            let file_read_task =
                platform.begin_file_read(resources.chunk_data_file, first_byte, buffer);

            self.in_flight_queue
                .push_back(LoadTask {
                    file_read_task,
                    read_buffer_metadata,
                    chunk_index,
                    category,
                })
                .ok()
                .unwrap();
        }
    }

    /// Checks for finished file read requests and writes their results into the
    /// resource database.
    pub fn finish_reads(&mut self, resources: &mut ResourceDatabase, platform: &dyn Pal) {
        while let Some(LoadTask { file_read_task, .. }) = self.in_flight_queue.peek_front() {
            if !platform.is_file_read_finished(file_read_task) {
                break;
            }

            let LoadTask {
                file_read_task,
                read_buffer_metadata,
                chunk_index,
                category,
            } = self.in_flight_queue.pop_front().unwrap();

            let (buffer, read_success) = match platform.finish_file_read(file_read_task) {
                Ok(buffer) => (buffer, true),
                Err(buffer) => (buffer, false),
            };

            if read_success {
                match category {
                    LoadCategory::Chunk => {
                        let desc = &resources.chunk_descriptors[chunk_index as usize];
                        let init_fn = || Some(ChunkData::empty());
                        if let Some(dst) = resources.chunks.insert(chunk_index, init_fn) {
                            dst.update(desc, &buffer);
                        }
                    }

                    LoadCategory::TextureChunk => {
                        let desc = &resources.texture_chunk_descriptors[chunk_index as usize];
                        let init_fn = || TextureChunkData::empty(platform);
                        if let Some(dst) = resources.texture_chunks.insert(chunk_index, init_fn) {
                            dst.update(desc, &buffer, platform);
                        }
                    }
                }
            } else {
                platform.println(format_args!("failed to read {category:?} #{chunk_index}"));
            }

            // Safety: each LoadTask gets its parts from one RingSlice, and
            // these are from this specific LoadTask, so these are a pair.
            let slice = unsafe { RingSlice::from_parts(buffer, read_buffer_metadata) };
            self.staging_buffer.free(slice).unwrap();
        }
    }
}
