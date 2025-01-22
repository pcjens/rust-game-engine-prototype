use platform_abstraction_layer::{FileReadTask, Pal};

use crate::{
    allocators::LinearAllocator,
    collections::{FixedVec, Queue, RingBuffer, RingSlice},
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

pub struct ResourceLoader<'eng> {
    staging_buffer: RingBuffer<'eng>,
    loading_queue: Queue<'eng, LoadRequest>,
    staging_slice_queue: Queue<'eng, RingSlice>,
}

impl<'eng> ResourceLoader<'eng> {
    pub fn new(
        allocator: &'eng LinearAllocator,
        staging_buffer_size: usize,
        resource_db: &ResourceDatabase,
    ) -> Option<ResourceLoader<'eng>> {
        let total_chunks = resource_db.chunks.array_len() + resource_db.texture_chunks.array_len();
        Some(ResourceLoader {
            staging_buffer: RingBuffer::new(allocator, staging_buffer_size)?,
            loading_queue: Queue::new(allocator, total_chunks)?,
            staging_slice_queue: Queue::new(allocator, total_chunks)?,
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
        struct LoadTask<'a> {
            file_read_task: Option<FileReadTask<'a>>,
            index: u32,
            category: LoadCategory,
        }

        if max_chunks_to_load == 0 {
            return;
        }

        let mut tasks = FixedVec::new(arena, max_chunks_to_load).unwrap();

        // Split `self.staging_buffer` into separate mutable slices for the file reads
        let mut staging_slice_handles = FixedVec::new(arena, max_chunks_to_load).unwrap();
        for slice_handle in self.staging_slice_queue.iter().take(max_chunks_to_load) {
            staging_slice_handles.push(slice_handle).unwrap();
        }
        let mut staging_slices = Queue::new(arena, max_chunks_to_load).unwrap();
        self.staging_buffer
            .get_many_mut(&mut staging_slice_handles, &mut staging_slices);

        // Begin reads (this pops from loading_queue, matching staging_slice_queue pops are after the reads are done)
        while let (
            Some(LoadRequest {
                first_byte,
                chunk_index: index,
                category,
            }),
            Some(buffer),
        ) = (self.loading_queue.pop_front(), staging_slices.pop_front())
        {
            let file_read_task =
                Some(platform.begin_file_read(resources.chunk_data_file, first_byte, buffer));
            tasks
                .push(LoadTask {
                    file_read_task,
                    index,
                    category,
                })
                .ok()
                .unwrap();
            if tasks.is_full() {
                break;
            }
        }

        // Write the chunks (TODO: this part should be multithreadable, just needs some AoS -> SoA type of refactoring)
        for LoadTask {
            file_read_task,
            index,
            category,
        } in tasks.iter_mut()
        {
            if let Some(buffer) = file_read_task.take().unwrap().read_to_end() {
                match category {
                    LoadCategory::Chunk => {
                        let desc = &resources.chunk_descriptors[*index as usize];
                        let init_fn = || Some(ChunkData::empty());
                        if let Some(dst) = resources.chunks.insert(*index, init_fn) {
                            dst.update(desc, buffer);
                        }
                    }

                    LoadCategory::TextureChunk => {
                        let desc = &resources.texture_chunk_descriptors[*index as usize];
                        let init_fn = || TextureChunkData::empty(platform);
                        if let Some(dst) = resources.texture_chunks.insert(*index, init_fn) {
                            dst.update(desc, buffer, platform);
                        }
                    }
                }
            }
        }

        // Free up self.staging_buffer for mutation again:
        assert!(tasks.iter().all(|task| task.file_read_task.is_none()));
        drop(tasks);

        // Align loading_queue and staging_slice_queue by popping off the slices used
        let staging_slices_count = staging_slice_handles.len();
        drop(staging_slice_handles);
        for _ in 0..staging_slices_count {
            self.staging_buffer
                .free(self.staging_slice_queue.pop_front().unwrap())
                .unwrap();
        }
    }
}
