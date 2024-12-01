#![no_std]

use core::ffi::c_void;

/// "Platform abstraction layer": a trait for using platform-dependent features
/// from the engine without depending on any platform directly. All the
/// functions have a `&self` parameter, so that the methods can access some
/// (possibly internally mutable) state, but still keeping the platform object
/// as widely usable as possible (a "platform" is about as global an object as
/// you get). Also, none of these functions are (supposed to be) hot, and this
/// trait is object safe, so using &dyn [Pal] should be fine performance-wise,
/// and will hopefully help with compilation times by avoiding generics.
pub trait Pal {
    /// Print out a string. For very crude debugging.
    fn println(&self, message: &str);
    /// Exit the process, with `clean: false` if intending to signal failure.
    fn exit(&self, clean: bool) -> !;

    /// Allocate the given amount of bytes (returning a null pointer on error).
    /// Not called often from the engine, memory is allocated in big chunks, so
    /// this can be slow and defensively implemented.
    fn malloc(&self, size: usize) -> *mut c_void;
    /// Free the memory allocated by `malloc`. Not called often from the engine,
    /// memory is allocated in big chunks, so this can be slow and defensively
    /// implemented.
    ///
    /// ## Safety
    ///
    /// - Since the implementation is free to free the memory, the memory
    ///   pointed at by the given pointer shouldn't be accessed after calling
    ///   this.
    /// - The `size` parameter must be the same value passed into the matching
    ///   `malloc` call.
    unsafe fn free(&self, ptr: *mut c_void, size: usize);
}
