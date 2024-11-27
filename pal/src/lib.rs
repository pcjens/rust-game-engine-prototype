#![no_std]

use core::ffi::c_void;

/// Trait for using platform-dependent features from the engine without
/// depending on any platform directly.
pub trait Pal {
    /// Exit the process, with `clean: false` if intending to signal failure.
    fn exit(&mut self, clean: bool) -> !;

    /// Allocate the given amount of bytes (returning a null pointer on error).
    fn malloc(size: usize) -> *mut c_void;
    /// Free the memory allocated by `malloc`.
    ///
    /// ## Safety
    ///
    /// The backing memory should never be accessed after calling this function.
    unsafe fn free(ptr: *mut c_void);
}
