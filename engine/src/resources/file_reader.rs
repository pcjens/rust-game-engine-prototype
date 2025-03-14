// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use platform::{FileHandle, FileReadTask, Platform};

use crate::{
    allocators::LinearAllocator,
    collections::{Queue, RingAllocationMetadata, RingBuffer, RingSlice},
};

/// The possible errors from [`FileReader::pop_read`].
#[derive(Debug)]
pub enum FileReadError {
    /// There was no file reading operation queued.
    NoReadsQueued,
    /// The read was requested to not block, and had not finished.
    WouldBlock,
    /// The underlying file reading operation failed.
    Platform,
}

struct LoadRequest {
    first_byte: u64,
    size: usize,
}

struct LoadTask {
    file_read_task: FileReadTask,
    read_buffer_metadata: RingAllocationMetadata,
}

/// File reading utility.
///
/// Can be used for both asynchronous and synchronous reads, as long as they're
/// read in the same order they were queued up.
pub struct FileReader {
    staging_buffer: RingBuffer<'static, u8>,
    to_load_queue: Queue<'static, LoadRequest>,
    in_flight_queue: Queue<'static, LoadTask>,
    file: FileHandle,
}

impl FileReader {
    /// Creates a new [`FileReader`] for reading `file`, with a maximum of
    /// `queue_capacity` concurrent read operations.
    pub fn new(
        arena: &'static LinearAllocator,
        file: FileHandle,
        staging_buffer_size: usize,
        queue_capacity: usize,
    ) -> Option<FileReader> {
        Some(FileReader {
            staging_buffer: RingBuffer::new(arena, staging_buffer_size)?,
            to_load_queue: Queue::new(arena, queue_capacity)?,
            in_flight_queue: Queue::new(arena, queue_capacity)?,
            file,
        })
    }

    /// Returns the size of the staging buffer where file read operations write
    /// to, i.e. the size of the largest read supported by this file reader.
    pub fn staging_buffer_size(&self) -> usize {
        self.staging_buffer.capacity()
    }

    /// Queues up a read operation starting at `first_byte`, reading `size`
    /// bytes. Returns `true` if the request fit in the queue.
    ///
    /// If `size` is larger than [`FileReader::staging_buffer_size`], this will
    /// always return `false`.
    #[must_use]
    pub fn push_read(&mut self, first_byte: u64, size: usize) -> bool {
        if size > self.staging_buffer_size() {
            return false;
        }

        self.to_load_queue
            .push_back(LoadRequest { first_byte, size })
            .is_ok()
    }

    /// Starts file read operations for the queued up loading requests.
    pub fn dispatch_reads(&mut self, platform: &dyn Platform) {
        profiling::function_scope!();
        while let Some(LoadRequest { size, .. }) = self.to_load_queue.peek_front() {
            profiling::scope!("dispatch");
            let Some(staging_slice) = self.staging_buffer.allocate(*size) else {
                break;
            };
            let (buffer, read_buffer_metadata) = staging_slice.into_parts();

            let LoadRequest {
                first_byte,
                size: _,
            } = self.to_load_queue.pop_front().unwrap();

            let file_read_task = platform.begin_file_read(self.file, first_byte, buffer);

            self.in_flight_queue
                .push_back(LoadTask {
                    file_read_task,
                    read_buffer_metadata,
                })
                .ok()
                .unwrap();
        }
    }

    /// Finishes the read operation at the front of the queue, and passes the
    /// results to the closure if the read was successful.
    ///
    /// If `blocking` is `true` and [`FileReader::dispatch_reads`] has not been
    /// called, this will call it for convenience.
    pub fn pop_read<T, F>(
        &mut self,
        platform: &dyn Platform,
        blocking: bool,
        use_result: F,
    ) -> Result<T, FileReadError>
    where
        F: FnOnce(&mut [u8]) -> T,
    {
        profiling::function_scope!();
        if blocking && self.in_flight_queue.is_empty() {
            self.dispatch_reads(platform);
        }

        let Some(LoadTask { file_read_task, .. }) = self.in_flight_queue.peek_front() else {
            return Err(FileReadError::NoReadsQueued);
        };

        if !blocking && !platform.is_file_read_finished(file_read_task) {
            return Err(FileReadError::WouldBlock);
        }

        let LoadTask {
            file_read_task,
            read_buffer_metadata,
        } = self.in_flight_queue.pop_front().unwrap();

        let (mut buffer, read_success) = match platform.finish_file_read(file_read_task) {
            Ok(buffer) => (buffer, true),
            Err(buffer) => (buffer, false),
        };

        let result = if read_success {
            Ok(use_result(&mut buffer))
        } else {
            Err(FileReadError::Platform)
        };

        // Safety: each LoadTask gets its parts from one RingSlice, and
        // these are from this specific LoadTask, so these are a pair.
        let slice = unsafe { RingSlice::from_parts(buffer, read_buffer_metadata) };
        self.staging_buffer.free(slice).unwrap();

        result
    }
}
