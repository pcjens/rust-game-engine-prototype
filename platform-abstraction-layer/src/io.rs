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

/// Handle to an asynchronous file reading operation.
///
/// If dropped, the Drop implementation will block until the file reading
/// operation is finished.
pub struct FileReadTask<'a> {
    file: FileHandle,
    task_id: u64,
    buffer: Option<&'a mut [u8]>,
    platform: &'a dyn Pal,
}

impl<'a> FileReadTask<'a> {
    pub fn new(
        file: FileHandle,
        task_id: u64,
        buffer: &'a mut [u8],
        platform: &'a dyn Pal,
    ) -> FileReadTask<'a> {
        FileReadTask {
            file,
            task_id,
            buffer: Some(buffer),
            platform,
        }
    }

    pub fn file(&self) -> FileHandle {
        self.file
    }

    pub fn task_id(&self) -> u64 {
        self.task_id
    }

    pub fn read_size(&self) -> usize {
        self.buffer.as_ref().unwrap().len()
    }

    /// Blocks until the read operation finishes, returning the slice or `None`
    /// if the read operation failed for any reason.
    pub fn read_to_end(self) -> Option<&'a mut [u8]> {
        self.platform.finish_file_read(self)
    }

    /// Deconstructs this into the inner buffer. Intended for platform layers
    /// implementing [`Pal::finish_file_read`].
    ///
    /// ## Safety
    ///
    /// The platform may have shared a pointer to this buffer with e.g. the
    /// kernel for async writing. The caller must ensure that when calling this
    /// fucntion, such a shared pointer will not be used anymore.
    pub unsafe fn into_inner(mut self) -> &'a mut [u8] {
        self.buffer.take().unwrap()
    }
}

impl Drop for FileReadTask<'_> {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            // Since the buffer has not been `take`n, `FileReadTask::into_inner`
            // hasn't been called, which in turn means that the read has not
            // necessarily finished. So we create a temporary owned version of
            // this file read task, to finish reading it, after which we can be
            // sure that `buffer` isn't being used anymore, so we can let this
            // task drop.
            let temp = FileReadTask {
                file: self.file,
                task_id: self.task_id,
                buffer: Some(buffer),
                platform: self.platform,
            };
            temp.read_to_end();
            // This drop impl will not recurse because `read_to_end` will call
            // `into_inner`, after which this branch won't be taken.
        }
    }
}
