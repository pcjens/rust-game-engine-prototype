// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! This crate mainly revolves around the [`Platform`] trait, which can be
//! implemented to provide a "platform implementation" for the game engine.
//! Otherwise, this crate mostly contains some low-level parts of the engine
//! which are necessarily needed to implement [`Platform`], such as some
//! multithreading utilities.
//!
//! This is split off of the main engine crate so that the engine and the
//! platform implementation can be compiled independently, which appears to
//! speed up compilation time.

#![no_std]
#![warn(missing_docs)]

mod boxed;
pub mod channel;
mod input;
mod io;
mod render;
mod semaphore;
pub mod thread_pool;
mod time;

use arrayvec::ArrayVec;

use core::fmt::Arguments;

pub use boxed::*;
pub use input::*;
pub use io::*;
pub use render::*;
pub use semaphore::*;
pub use thread_pool::{TaskChannel, ThreadState};
pub use time::*;

/// Sample rate for the audio data played back by the engine.
pub const AUDIO_SAMPLE_RATE: u32 = 48000;

/// The amount of channels of audio data played back by the engine.
pub const AUDIO_CHANNELS: usize = 2;

/// Shorthand for an [`ArrayVec`] of [`InputDevice`].
///
/// Exported so that platforms don't need to explicitly depend on [`arrayvec`]
/// just for the [`Platform::input_devices`] typing.
pub type InputDevices = ArrayVec<InputDevice, 15>;

/// Interface to the engine for the platform implementation.
///
/// Used to allow engine to do its thing each frame, and to pass events to it.
pub trait EngineCallbacks {
    /// Run one frame of the game loop.
    fn run_frame(
        &mut self,
        platform: &dyn Platform,
        run_game_frame: &mut dyn FnMut(Instant, &dyn Platform, &mut Self),
    );

    /// Handle an event.
    fn event(&mut self, event: Event, timestamp: Instant);
}

/// A trait for using platform-dependent features from the engine without
/// depending on any platform implementation directly. A platform implementation
/// should implement this trait, and also call the engine's "iterate" and
/// "event" methods at appropriate times.
///
/// All the functions have a `&self` parameter, so that the methods can access
/// some (possibly internally mutable) state, but still keeping the platform
/// object as widely usable as possible (a "platform" is about as global an
/// object as you get). Also, none of these functions are (supposed to be) hot,
/// and this trait is object safe, so using &dyn [`Platform`] should be fine
/// performance-wise, and will hopefully help with compilation times by avoiding
/// generics.
pub trait Platform {
    /// Get the current screen size. Could be physical pixels, could be
    /// "logical" pixels, depends on the platform, but it's the same coordinate
    /// system as the [`Vertex2D`]es passed into [`Platform::draw_2d`].
    fn draw_area(&self) -> (f32, f32);

    /// Get the current screen scale factor. When multiplied with
    /// [`Platform::draw_area`] should match the resolution of the framebuffer
    /// (i.e. the resolution which sprites should match for pixel perfect
    /// rendering).
    fn draw_scale_factor(&self) -> f32;

    /// Render out a pile of possibly textured 2D triangles.
    fn draw_2d(&self, vertices: &[Vertex2D], indices: &[u32], settings: DrawSettings2D);

    /// Create a sprite of the given size and format. Returns None if the sprite
    /// could not be created due to any reason (sprite dimensions too large, out
    /// of vram, etc.). See [`Vertex2D`] and [`DrawSettings2D`] for sampler
    /// details.
    ///
    /// ### Implementation note
    ///
    /// These are never freed during the lifetime of the engine. Internally,
    /// individual sprites are reused as they get streamed in and out.
    fn create_sprite(&self, width: u16, height: u16, format: PixelFormat) -> Option<SpriteRef>;

    /// Update the pixel data of a sprite within a region. Pixels are tightly
    /// packed and in the same format as passed into the creation function.
    fn update_sprite(
        &self,
        sprite: SpriteRef,
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
    /// [`Platform::finish_file_read`] is called with the task returned from
    /// this function, since [`FileReadTask`] can't (safely) be dropped without
    /// it getting called.
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

    /// Passes a buffer of audio samples to be played back, and the playback
    /// position where the samples start.
    ///
    /// Each sample should be a tuple containing the left and right channels'
    /// audio samples for stereo playback, in that order.
    ///
    /// The playback position where the platform will start reading can be
    /// queried with [`Platform::audio_playback_position`].
    fn update_audio_buffer(&self, first_position: u64, samples: &[[i16; AUDIO_CHANNELS]]);

    /// Returns the playback position of the next sample the platform will play,
    /// and the timestamp which it should be considered to be synchronized with.
    ///
    /// Any samples submitted with [`Platform::update_audio_buffer`] before this
    /// position will be ignored.
    fn audio_playback_position(&self) -> (u64, Instant);

    /// Get a list of the currently connected input devices.
    fn input_devices(&self) -> InputDevices;

    /// Get the default button for one of the generic action categories for the
    /// given input device, if a default exists.
    fn default_button_for_action(
        &self,
        action: ActionCategory,
        device: InputDevice,
    ) -> Option<Button>;

    /// Returns the current point in time according to the platform
    /// implementation.
    fn now(&self) -> Instant;

    /// Print out a string. For very crude debugging.
    fn println(&self, message: Arguments);

    /// Request the process to exit, with `clean: false` if intending to signal
    /// failure. On a clean exit, the exit may be delayed until a moment later,
    /// e.g. at the end of the current frame of the game loop, and after
    /// resource clean up. In failure cases, the idea is to bail asap, but it's
    /// up to the platform.
    fn exit(&self, clean: bool);
}
