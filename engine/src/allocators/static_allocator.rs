// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::ops::{Deref, DerefMut};

use super::LinearAllocator;

/// Creates a static [`StaticAllocator`] with the given amount of bytes of
/// backing memory.
///
/// Note that this creates an allocator backed by a static byte array, i.e. the
/// memory isn't dynamically allocated nor freed, it's just a big static
/// variable, *one for each call of this macro.* Generally this'll appear once
/// per crate if needed, there shouldn't be much of a reason to have multiple of
/// these, if any.
///
/// As such, even though this can be assigned to a variable (e.g. `let arena =
/// static_allocator!(1);`), that variable will only be a borrow of the single
/// static variable that this macro expands to. If such a function is called
/// multiple times, `arena` will get a reference to the same arena every time.
///
/// ### Example
///
/// ```
/// use engine::allocators::{StaticAllocator, static_allocator};
/// static PERSISTENT_ARENA: &StaticAllocator = static_allocator!(1024 * 1024);
/// ```
#[macro_export]
macro_rules! static_allocator {
    ($size:expr) => {{
        static mut MEM: [u8; $size] = [0; $size];
        // Safety (LinearAllocator::from_raw_slice): MEM is only accessible in
        // this scope, and this scope only creates one allocator from it, since
        // the allocator is stored in a static variable.
        //
        // Safety (StaticAllocator::new): from_raw_slice creates a
        // LinearAllocator without a platform reference.
        static ALLOCATOR: $crate::allocators::StaticAllocator = unsafe {
            $crate::allocators::StaticAllocator::from_allocator(
                $crate::allocators::LinearAllocator::from_raw_slice(&raw mut MEM),
            )
        };
        &ALLOCATOR
    }};
}

pub use static_allocator;

/// A [`Sync`] wrapper for [`LinearAllocator`]. See also: [`static_allocator`]
///
/// Since this type is stored in a static variable, it's always immutably
/// borrowed, and thus cannot be reset. Because nothing is ever freed, this
/// allocator is only used for persistent data structures that can be reused for
/// the whole runtime of the engine.
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
