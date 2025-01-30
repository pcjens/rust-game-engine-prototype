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

pub struct ResourceLoader {
    staging_buffer: RingBuffer<u8>,
    to_load_queue: Queue<'static, LoadRequest>,
    in_flight_queue: Queue<'static, LoadTask>,
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
            to_load_queue: Queue::new(arena, total_chunks)?,
            in_flight_queue: Queue::new(arena, total_chunks)?,
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
        let mut dispatched_any = false;
        while let Some(LoadRequest { size, .. }) = self.to_load_queue.peek_front() {
            let Some(staging_slice) = self.staging_buffer.allocate(*size) else {
                if !dispatched_any && self.in_flight_queue.is_empty() {
                    panic!("resource loader has no in-flight loads, but staging buffer can't fit a single read (staging_buffer_size too low?)");
                }
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

            dispatched_any = true;
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
