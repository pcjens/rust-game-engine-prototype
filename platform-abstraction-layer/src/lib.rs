#![no_std]

mod input;
mod render;

use arrayvec::ArrayVec;

use core::{ffi::c_void, time::Duration};

pub use input::*;
pub use render::*;

pub type InputDevices = ArrayVec<InputDevice, 15>;

/// "Platform abstraction layer": a trait for using platform-dependent features
/// from the engine without depending on any platform directly. A full
/// implementation should implement this trait, and also call the engine's
/// "iterate" and "event" methods at appropriate times.
///
/// All the functions have a `&self` parameter, so that the methods can access
/// some (possibly internally mutable) state, but still keeping the platform
/// object as widely usable as possible (a "platform" is about as global an
/// object as you get). Also, none of these functions are (supposed to be) hot,
/// and this trait is object safe, so using &dyn [Pal] should be fine
/// performance-wise, and will hopefully help with compilation times by avoiding
/// generics.
pub trait Pal {
    /// Get the current screen size. Could be physical pixels, could be
    /// "logical" pixels, depends on the platform, but it's the same coordinate
    /// system as the [Vertex]es passed into [Pal::draw_triangles].
    fn draw_area(&self) -> (f32, f32);

    /// Render out a pile of triangles.
    fn draw_triangles(&self, vertices: &[Vertex], indices: &[u32], settings: DrawSettings);

    /// Create a texture of the given size and format. Returns None if the
    /// texture could not be created due to any reason (texture dimensions too
    /// large, out of vram, etc.). See [Vertex] and [DrawSettings] for sampler
    /// details.
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

    /// Free the memory allocated by [Pal::malloc]. Not called often from the
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
