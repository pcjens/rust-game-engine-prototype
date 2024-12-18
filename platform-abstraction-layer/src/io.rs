use core::fmt::Debug;

#[allow(unused_imports)] // used in docs
use super::Pal;

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

    pub fn inner(self) -> u64 {
        self.0
    }
}

/// Handle to an asynchronous file reading operation. Instead of dropping, these
/// *must* be passed to a [`Pal::poll_file_read`] call until they are consumed.
/// It is not safe to use the buffer contained in the task until the read
/// operation is finished.
pub struct FileReadTask<'a> {
    file: FileHandle,
    task_id: u64,
    buffer: &'a mut [u8],
}

impl<'a> FileReadTask<'a> {
    pub fn new(file: FileHandle, task_id: u64, buffer: &'a mut [u8]) -> FileReadTask<'a> {
        FileReadTask {
            file,
            task_id,
            buffer,
        }
    }

    pub fn file(&self) -> FileHandle {
        self.file
    }

    pub fn task_id(&self) -> u64 {
        self.task_id
    }

    pub fn read_size(&self) -> usize {
        self.buffer.len()
    }

    /// ## Safety
    /// The platform may have shared a pointer to this buffer with e.g. the
    /// kernel for async writing. The caller must ensure that at this point,
    /// such a shared pointer will not be used anymore.
    pub unsafe fn into_inner(self) -> &'a mut [u8] {
        self.buffer
    }
}
