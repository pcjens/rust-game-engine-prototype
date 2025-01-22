use platform_abstraction_layer::{FileReadTask, Pal};

use crate::{
    allocators::{LinearAllocator, StaticAllocator},
    collections::{Queue, RingBuffer, RingSlice, RingSliceMetadata},
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
    chunk_index: u32,
    category: LoadCategory,
}

pub struct ResourceLoader {
    staging_buffer: RingBuffer,
    loading_queue: Queue<'static, LoadRequest>,
    staging_slice_queue: Queue<'static, RingSlice>,
}

impl ResourceLoader {
    pub fn new(
        arena: &'static StaticAllocator,
        staging_buffer_size: usize,
        resource_db: &ResourceDatabase,
    ) -> Option<ResourceLoader> {
        let total_chunks = resource_db.chunks.array_len() + resource_db.texture_chunks.array_len();
        Some(ResourceLoader {
            staging_buffer: RingBuffer::new(arena, staging_buffer_size)?,
            loading_queue: Queue::new(arena, total_chunks)?,
            staging_slice_queue: Queue::new(arena, total_chunks)?,
        })
    }

    pub fn queue_chunk(&mut self, chunk_index: u32, resources: &ResourceDatabase) {
        self.queue_load(chunk_index, LoadCategory::Chunk, resources);
    }

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
        if !self.staging_buffer.would_fit(chunk_size)
            || self
                .loading_queue
                .iter()
                .any(|req| req.chunk_index == chunk_index && req.category == category)
        {
            return;
        }
        let staging_slice = self.staging_buffer.allocate(chunk_size).unwrap();
        self.loading_queue
            .push_back(LoadRequest {
                first_byte: resources.chunk_data_offset + chunk_source.start,
                chunk_index,
                category,
            })
            .unwrap();
        self.staging_slice_queue.push_back(staging_slice).unwrap();
    }

    /// Loads up to `max_chunks_to_load` queued chunks.
    ///
    /// ### Panics
    ///
    /// Panics if `arena` doesn't have enough memory for the
    /// loading tasks.
    pub fn load_queue(
        &mut self,
        max_chunks_to_load: usize,
        resources: &mut ResourceDatabase,
        platform: &dyn Pal,
        arena: &LinearAllocator,
    ) {
        struct LoadTask {
            file_read_task: FileReadTask,
            read_buffer_metadata: RingSliceMetadata,
            index: u32,
            category: LoadCategory,
        }

        if max_chunks_to_load == 0 {
            return;
        }

        let mut tasks = Queue::new(arena, max_chunks_to_load).unwrap();

        // Begin reads (this pops from loading_queue, matching staging_slice_queue pops are after the reads are done)
        while let (
            Some(LoadRequest {
                first_byte,
                chunk_index: index,
                category,
            }),
            Some(buffer),
        ) = (
            self.loading_queue.pop_front(),
            self.staging_slice_queue.pop_front(),
        ) {
            let (buffer, read_buffer_metadata) = buffer.into_parts();
            let file_read_task =
                platform.begin_file_read(resources.chunk_data_file, first_byte, buffer);
            tasks
                .push_back(LoadTask {
                    file_read_task,
                    read_buffer_metadata,
                    index,
                    category,
                })
                .ok()
                .unwrap();
            if tasks.is_full() {
                break;
            }
        }

        // Write the chunks
        while let Some(LoadTask {
            file_read_task,
            read_buffer_metadata,
            index,
            category,
        }) = tasks.pop_front()
        {
            let (buffer, read_success) = match platform.finish_file_read(file_read_task) {
                Ok(buffer) => (buffer, true),
                Err(buffer) => (buffer, false),
            };

            if read_success {
                match category {
                    LoadCategory::Chunk => {
                        let desc = &resources.chunk_descriptors[index as usize];
                        let init_fn = || Some(ChunkData::empty());
                        if let Some(dst) = resources.chunks.insert(index, init_fn) {
                            dst.update(desc, &buffer);
                        }
                    }

                    LoadCategory::TextureChunk => {
                        let desc = &resources.texture_chunk_descriptors[index as usize];
                        let init_fn = || TextureChunkData::empty(platform);
                        if let Some(dst) = resources.texture_chunks.insert(index, init_fn) {
                            dst.update(desc, &buffer, platform);
                        }
                    }
                }
            } else {
                platform.println(format_args!("failed to read {category:?} #{index}"));
            }

            // Safety: each LoadTask gets its parts from one RingSlice, and
            // these are from this specific LoadTask, so these are a pair.
            let slice = unsafe { RingSlice::from_parts(buffer, read_buffer_metadata) };
            self.staging_buffer.free(slice).unwrap();
        }
    }
}
