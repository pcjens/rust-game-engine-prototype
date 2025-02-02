#![no_std]

mod boxed;
pub mod channel;
mod input;
mod io;
mod render;
mod semaphore;
pub mod thread_pool;

use arrayvec::ArrayVec;

use core::{fmt::Arguments, time::Duration};

pub use boxed::*;
pub use input::*;
pub use io::*;
pub use render::*;
pub use semaphore::*;
pub use thread_pool::{TaskChannel, ThreadState};

/// Shorthand for an [`ArrayVec`] of [`InputDevice`].
///
/// Exported so that platforms don't need to explicitly depend on [`arrayvec`]
/// just for the [`Pal::input_devices`] typing.
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
    /// ### Implementation note
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
    /// Implementations can assume that `'a` will last until
    /// [`Pal::finish_file_read`] is called with the task returned from this
    /// function, since [`FileReadTask`] can't (safely) be dropped without it
    /// getting called.
    #[must_use]
    fn begin_file_read(&self, file: FileHandle, first_byte: u64, buffer: Box<[u8]>)
        -> FileReadTask;

    /// Returns true if the read task has finished (in success or failure),
    /// false if it's still pending.
    fn is_file_read_finished(&self, task: &FileReadTask) -> bool;

    /// Blocks until the read task finishes, and returns the buffer which the
    /// file contents were written into, if successful. If the read fails, the
    /// memory is returned wrapped in an `Err`, and the buffer contents are not
    /// guaranteed.
    fn finish_file_read(&self, task: FileReadTask) -> Result<Box<[u8]>, Box<[u8]>>;

    /// Creates a semaphore.
    ///
    /// Multi-threaded platforms should use [`Semaphore::new`] and implement the
    /// functions so that they make use of OS synchronization primitives.
    /// Single-threaded platforms can use [`Semaphore::single_threaded`].
    fn create_semaphore(&self) -> Semaphore;

    /// Returns how many threads the system could process in parallel
    /// efficiently.
    ///
    /// Note that this count shouldn't be decremented by one to "leave room for
    /// the main thread," because the main thread often sleeps while waiting for
    /// worker threads to finish their work.
    ///
    /// If this returns 1, the thread pool will not utilize worker threads, and
    /// `spawn_pool_thread` can be left `unimplemented!`.
    fn available_parallelism(&self) -> usize;

    /// Spawns a thread for a thread pool, using the given channels to pass
    /// tasks back and forth.
    ///
    /// Implementation note: unless the build has `panic = "abort"`, the worker
    /// thread should catch panics, and if they happen, call `signal_panic` on
    /// the task and send the task back to the main thread via the results
    /// channel, and *then* resume the panic. This will avoid the main thread
    /// silently hanging when joining tasks that panicked the thread they were
    /// running on, it'll panic instead, with a message about a thread pool
    /// thread panicking.
    fn spawn_pool_thread(&self, channels: [TaskChannel; 2]) -> ThreadState;

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
    fn println(&self, message: Arguments);

    /// Request the process to exit, with `clean: false` if intending to signal
    /// failure. On a clean exit, the exit may be delayed until a moment later,
    /// e.g. at the end of the current frame of the game loop, and after
    /// resource clean up. In failure cases, the idea is to bail asap, but it's
    /// up to the platform.
    fn exit(&self, clean: bool);
}
