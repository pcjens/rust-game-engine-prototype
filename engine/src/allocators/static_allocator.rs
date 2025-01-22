use core::ops::{Deref, DerefMut};

use super::LinearAllocator;

/// Creates a [`StaticAllocator`] with the given amount of bytes of backing
/// memory.
///
/// Note that this creates an allocator backed by a static byte array, i.e. the
/// memory isn't dynamically allocated nor freed, it's just a big static
/// variable, *one for each call of this macro.* Generally this'll appear once
/// per crate if needed, there shouldn't be much of a reason to have multiple of
/// these, if any.
///
/// ### Example
///
/// ```
/// use engine::allocators::{StaticAllocator, static_allocator_new};
/// static PERSISTENT_ARENA: StaticAllocator = static_allocator_new!(1024 * 1024 * 1024);
/// ```
#[macro_export]
macro_rules! static_allocator_new {
    ($size:expr) => {
        const {
            static mut MEM: [u8; $size] = [0; $size];
            // Safety (LinearAllocator::from_raw_slice): MEM is only accessible
            // in this scope, and this scope only creates one allocator from it
            // (since this is a const scope initializing a static variable).
            //
            // Safety (StaticAllocator::new): from_raw_slice creates a
            // LinearAllocator without a platform reference.
            unsafe {
                $crate::allocators::StaticAllocator::from_allocator(
                    $crate::allocators::LinearAllocator::from_raw_slice(&raw mut MEM),
                )
            }
        }
    };
}

pub use static_allocator_new;

/// [`LinearAllocator`] but shareable between threads. Created with
/// [`static_allocator_new`].
pub struct StaticAllocator {
    inner: LinearAllocator<'static>,
}

impl StaticAllocator {
    /// ### Safety
    ///
    /// The `inner` allocator must not have a platform
    #[doc(hidden)]
    pub const unsafe fn from_allocator(inner: LinearAllocator<'static>) -> StaticAllocator {
        StaticAllocator { inner }
    }
}

impl Deref for StaticAllocator {
    type Target = LinearAllocator<'static>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for StaticAllocator {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Safety: the parts of [`LinearAllocator`] which are not Sync already are the
/// backing memory pointer and the platform borrow.
/// - The backing memory pointer is fine to share between threads, because the
///   whole logic of the allocator makes sure to not create aliasing mutable
///   borrows to the memory it points to. The *mut pointer may not have safety
///   guards, but LinearAllocator does.
/// - &dyn Pal is not necessarily sync, which is the reason StaticAllocator's
///   constructor requires a LinearAllocator without a platform. Since the
///   platform is always None, dyn Pal not being Sync shouldn't be an issue.
unsafe impl Sync for StaticAllocator {}
