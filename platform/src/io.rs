// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::Box;

/// Platform-specific file handle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileHandle(u64);

impl FileHandle {
    /// Creates a new [`FileHandle`]. Should only be created in the platform
    /// implementation, which also knows how the inner value is going to be
    /// used.
    pub fn new(id: u64) -> FileHandle {
        FileHandle(id)
    }

    /// Returns the inner value passed into [`FileHandle::new`]. Generally only
    /// relevant to the platform implementation.
    pub fn inner(self) -> u64 {
        self.0
    }
}

/// Handle to an asynchronous file reading operation.
pub struct FileReadTask {
    file: FileHandle,
    task_id: u64,
    buffer: Box<[u8]>,
}

impl FileReadTask {
    /// Creates a new [`FileReadTask`] with the task id differentiating
    /// different [`FileReadTask`]s. The platform implementation should create
    /// and keep track of these.
    pub fn new(file: FileHandle, task_id: u64, buffer: Box<[u8]>) -> FileReadTask {
        FileReadTask {
            file,
            task_id,
            buffer,
        }
    }

    /// Returns the [`FileHandle`] this task is using.
    pub fn file(&self) -> FileHandle {
        self.file
    }

    /// Returns the task id for this particular task, the same one passed into
    /// [`FileReadTask::new`].
    pub fn task_id(&self) -> u64 {
        self.task_id
    }

    /// Returns the size of the buffer, i.e. the amount of bytes read by this task.
    pub fn read_size(&self) -> usize {
        self.buffer.len()
    }

    /// Deconstructs this into the inner buffer. Intended for platform layers
    /// implementing
    /// [`Platform::finish_file_read`](crate::Platform::finish_file_read).
    ///
    /// ### Safety
    ///
    /// The platform may have shared a pointer to this buffer with e.g. the
    /// kernel for async writing. The caller must ensure that when calling this
    /// function, such a shared pointer will not be used anymore, as this
    /// function makes said memory writable again (not owned and hidden in this
    /// struct).
    pub unsafe fn into_inner(self) -> Box<[u8]> {
        self.buffer
    }
}
