use platform_abstraction_layer::{FileReadTask, Pal};

use crate::{
    allocators::LinearAllocator,
    collections::{FixedVec, RingBuffer, RingSlice},
};

use super::{ChunkData, ResourceDatabase, TextureChunkData};

#[derive(Debug)]
enum LoadCategory {
    Chunk,
    TextureChunk,
}

struct LoadRequest {
    first_byte: u64,
    index: u32,
    category: LoadCategory,
}

#[derive(Debug)]
struct LoadTask<'a> {
    file_read_task: Option<FileReadTask<'a>>,
    index: u32,
    category: LoadCategory,
}

pub struct ResourceLoader<'eng> {
    staging_buffer: RingBuffer<'eng, u8>,
    loading_queue: Option<LoadRequest>, // TODO: make a Queue and use it here
    staging_slice_queue: Option<RingSlice>, // TODO: make a Queue and use it here
}

impl<'eng> ResourceLoader<'eng> {
    pub fn new(
        allocator: &'eng LinearAllocator,
        staging_buffer_size: usize,
        queue_len: usize,
    ) -> Option<ResourceLoader<'eng>> {
        Some(ResourceLoader {
            staging_buffer: RingBuffer::new(allocator, staging_buffer_size)?,
            loading_queue: None,
            staging_slice_queue: None,
        })
    }

    pub fn queue_chunk(&mut self, index: u32, resources: &ResourceDatabase) {
        self.queue_load(index, LoadCategory::Chunk, resources);
    }

    pub fn queue_texture_chunk(&mut self, index: u32, resources: &ResourceDatabase) {
        self.queue_load(index, LoadCategory::TextureChunk, resources);
    }

    fn queue_load(&mut self, index: u32, category: LoadCategory, resources: &ResourceDatabase) {
        let chunk_source = &resources.texture_chunk_descriptors[index as usize].source_bytes;
        let chunk_size = (chunk_source.end - chunk_source.start) as usize;
        if !self.staging_buffer.would_fit(chunk_size) || self.loading_queue.is_some() {
            return;
        }
        let staging_slice = self.staging_buffer.allocate(chunk_size).unwrap();
        self.loading_queue = Some(LoadRequest {
            first_byte: chunk_source.start,
            index,
            category,
        });
        self.staging_slice_queue = Some(staging_slice);
    }

    /// Loads the currently queued chunks.
    ///
    /// # Panics
    ///
    /// Panics if `arena` doesn't have enough memory for the
    /// loading tasks.
    pub fn load_queue(
        &mut self,
        resources: &mut ResourceDatabase,
        platform: &dyn Pal,
        arena: &LinearAllocator,
    ) {
        let mut tasks = FixedVec::new(arena, 1 /* TODO: use loading_queue length */).unwrap();

        // Begin reads
        // TODO: drain through the loading queue, peek through the staging slice queue
        if let (
            Some(LoadRequest {
                first_byte,
                index,
                category,
            }),
            Some(staging_slice),
        ) = (self.loading_queue.take(), self.staging_slice_queue.as_ref())
        {
            let buffer = self.staging_buffer.get_mut(staging_slice);
            let file_read_task =
                Some(platform.begin_file_read(resources.chunk_data_file, first_byte, buffer));
            tasks
                .push(LoadTask {
                    file_read_task,
                    index,
                    category,
                })
                .unwrap();
        }

        // Write the chunks (TODO: this part should be multithreadable, just needs some AoS -> SoA type of refactoring)
        for LoadTask {
            file_read_task,
            index,
            category,
        } in tasks.iter_mut()
        {
            while let Some(task) = file_read_task.take() {
                match platform.poll_file_read(task) {
                    Ok(buffer) => match category {
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
                    },
                    Err(Some(task)) => {
                        *file_read_task = Some(task);
                    }
                    Err(None) => {}
                }
            }
        }

        // Free up self.staging_buffer for mutation again:
        assert!(tasks.iter().all(|task| task.file_read_task.is_none()));
        drop(tasks);

        // TODO: drain through the loading queue
        if let Some(staging_slice) = self.staging_slice_queue.take() {
            self.staging_buffer.free(staging_slice).unwrap();
        }
    }
}
