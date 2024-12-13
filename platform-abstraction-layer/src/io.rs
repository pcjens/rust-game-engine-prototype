/// Wrapper for platform-specific implementations of file reading, with an
/// asynchronous API (though not in the async/await sense).
pub struct FileReader<'a>(
    // TODO: make this struct not rely on a trait object, it's very annoying.
    &'a mut (dyn FileReaderOps + Send),
);

impl FileReader<'_> {
    /// Create a new file reader. On drop, [`FileReaderOps::close`] is called.
    pub fn new(reader_impl: &mut (dyn FileReaderOps + Send)) -> FileReader {
        FileReader(reader_impl)
    }

    /// ## Safety
    /// The returned [`ReadHandle`] must not be dropped, but instead be passed
    /// to [`FileReader::poll`] until the buffer is returned back.
    /// [`ReadHandle::drop`] will panic if this is not followed. Leaking the
    /// [`ReadHandle`] is a way to avoid the panic without polling, as that
    /// ensures that `buffer` cannot be written to.
    #[must_use]
    pub fn read<'a>(&mut self, first_byte: u64, buffer: &'a mut [u8]) -> ReadHandle<'a> {
        self.0.read(first_byte, buffer)
    }

    /// ## Safety
    /// An error from this function implies that the read is still processing.
    /// The returned [`ReadHandle`] must be dealt with according to the rules
    /// explained in [`FileReader::read`].
    pub fn poll<'a>(&mut self, handle: ReadHandle<'a>) -> Result<&'a mut [u8], ReadHandle<'a>> {
        self.0.poll(handle)
    }
}

impl Drop for FileReader<'_> {
    fn drop(&mut self) {
        self.0.close();
    }
}

pub trait FileReaderOps {
    /// ## Safety
    /// See [`FileReader::read`].
    #[must_use]
    fn read<'a>(&mut self, first_byte: u64, buffer: &'a mut [u8]) -> ReadHandle<'a>;

    /// ## Safety
    /// See [`FileReader::poll`].
    fn poll<'a>(&mut self, handle: ReadHandle<'a>) -> Result<&'a mut [u8], ReadHandle<'a>>;

    /// If the implementor is wrapped in a [`FileReader`], this is only called
    /// when the [`FileReader`] is dropped, and none of the other functions are
    /// called after this.
    fn close(&mut self);
}

pub struct ReadHandle<'a> {
    buffer: Option<&'a mut [u8]>,
}

impl<'a> ReadHandle<'a> {
    pub fn new(buffer: &'a mut [u8]) -> ReadHandle<'a> {
        ReadHandle {
            buffer: Some(buffer),
        }
    }

    /// ## Safety
    /// The platform may have shared a pointer to this buffer with e.g. the
    /// kernel for async writing. The caller must ensure that at this point,
    /// such a shared pointer will not be used anymore.
    pub unsafe fn into_inner(mut self) -> &'a mut [u8] {
        self.buffer.take().unwrap()
    }
}

impl Drop for ReadHandle<'_> {
    fn drop(&mut self) {
        if self.buffer.is_some() {
            panic!("ReadHandle dropped instead of being passed into FileReader::poll");
        }
    }
}
