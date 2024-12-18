#![no_std]

mod input;
mod io;
mod render;

use arrayvec::ArrayVec;

use core::{ffi::c_void, time::Duration};

pub use input::*;
pub use io::*;
pub use render::*;

pub type InputDevices = ArrayVec<InputDevice, 15>;

/// The "engine side" of [Pal], for passing the engine to the platform layer
/// implementation for event and update callbacks.
///
/// This is not the most ideal design, ideally it'd just all be downstream from
/// Engine, but emscripten as a platform is very much designed around callbacks
/// instead of a regular game loop the engine could own. So here we are.
pub trait EngineCallbacks {
    /// Run one iteration of the game loop.
    fn iterate(&mut self, platform: &dyn Pal);
    /// Handle an event. The duration passed in should refer to the time the
    /// event happened, using the same clock as [`Pal::elapsed`].
    fn event(&mut self, event: Event, elapsed: Duration, platform: &dyn Pal);
}

/// "Platform abstraction layer": a trait for using platform-dependent features
/// from the engine without depending on any platform directly. A full
/// implementation should implement this trait, and also call the engine's
/// "iterate" and "event" methods at appropriate times.
///
/// All the functions have a `&self` parameter, so that the methods can access
/// some (possibly internally mutable) state, but still keeping the platform
/// object as widely usable as possible (a "platform" is about as global an
/// object as you get). Also, none of these functions are (supposed to be) hot,
/// and this trait is object safe, so using &dyn [`Pal`] should be fine
/// performance-wise, and will hopefully help with compilation times by avoiding
/// generics.
pub trait Pal {
    /// Get the current screen size. Could be physical pixels, could be
    /// "logical" pixels, depends on the platform, but it's the same coordinate
    /// system as the [`Vertex`]es passed into [`Pal::draw_triangles`].
    fn draw_area(&self) -> (f32, f32);

    /// Render out a pile of triangles.
    fn draw_triangles(&self, vertices: &[Vertex], indices: &[u32], settings: DrawSettings);

    /// Create a texture of the given size and format. Returns None if the
    /// texture could not be created due to any reason (texture dimensions too
    /// large, out of vram, etc.). See [`Vertex`] and [`DrawSettings`] for
    /// sampler details.
    ///
    /// ## Implementation note
    ///
    /// These are never freed during the lifetime of the engine, which could be
    /// exploited for performance benefits. Internally, individual textures are
    /// reused as textures and sprites get streamed in and out.
    fn create_texture(&self, width: u16, height: u16, format: PixelFormat) -> Option<TextureRef>;

    /// Update the pixel data of a texture within a region. Pixels are tightly
    /// packed and in the same format as passed into the creation function.
    fn update_texture(
        &self,
        texture: TextureRef,
        x_offset: u16,
        y_offset: u16,
        width: u16,
        height: u16,
        pixels: &[u8],
    );

    /// Open a file for reading. Returns None if the file can't be read.
    fn open_file(&self, path: &str) -> Option<FileHandle>;

    /// Start an asynchronous read operation to fill `buffer` from the `file` at
    /// offset `first_byte`.
    ///
    /// ## Safety
    ///
    /// The returned [`FileReadTask`] must not be dropped, but instead be passed
    /// to [`Pal::poll_file_read`]. This rule ensures that the `buffer` passed
    /// into this function is not accessed while it's still being written to, as
    /// the buffer's mutable borrow would end after dropping the
    /// [`FileReadTask`], which is unsafe!
    #[must_use]
    fn begin_file_read<'a>(
        &self,
        file: FileHandle,
        first_byte: u64,
        buffer: &'a mut [u8],
    ) -> FileReadTask<'a>;

    /// Poll if a read has completed successfully, returning the buffer
    /// containing the data if it has. If not, but the read is still being
    /// processed, the task is returned back, to be polled again later. If the
    /// read fails, an `Err(None)` is returned.
    ///
    /// ## Safety
    ///
    /// The `Err(Some(task))` result from this function implies that the read is
    /// still processing. The returned [`FileReadTask`] must be dealt with
    /// according to the rules explained in [`Pal::begin_file_read`].
    fn poll_file_read<'a>(
        &self,
        task: FileReadTask<'a>,
    ) -> Result<&'a mut [u8], Option<FileReadTask<'a>>>;

    /// Get a list of the currently connected input devices.
    fn input_devices(&self) -> InputDevices;

    /// Get the default button for one of the generic action categories for the
    /// given input device, if a default exists.
    fn default_button_for_action(
        &self,
        action: ActionCategory,
        device: InputDevice,
    ) -> Option<Button>;

    /// Returns the amount of time elapsed since the platform was initialized.
    fn elapsed(&self) -> Duration;

    /// Print out a string. For very crude debugging.
    fn println(&self, message: &str);

    /// Request the process to exit, with `clean: false` if intending to signal
    /// failure. On a clean exit, the exit may be delayed until a moment later,
    /// e.g. at the end of the current frame of the game loop, and after
    /// resource clean up. In failure cases, the idea is to bail asap, but it's
    /// up to the platform.
    fn exit(&self, clean: bool);

    /// Allocate the given amount of bytes (returning a null pointer on error).
    /// Not called often from the engine, memory is allocated in big chunks, so
    /// this can be slow and defensively implemented.
    fn malloc(&self, size: usize) -> *mut c_void;

    /// Free the memory allocated by [`Pal::malloc`]. Not called often from the
    /// engine, memory is allocated in big chunks, so this can be slow and
    /// defensively implemented.
    ///
    /// ## Safety
    ///
    /// - Since the implementation is free to free the memory, the memory
    ///   pointed at by the given pointer shouldn't be accessed after calling
    ///   this.
    unsafe fn free(&self, ptr: *mut c_void);
}
