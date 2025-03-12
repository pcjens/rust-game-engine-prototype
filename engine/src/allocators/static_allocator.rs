// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

/// Creates a static [`LinearAllocator`](super::LinearAllocator) with the given
/// amount of bytes of backing memory.
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
/// use engine::allocators::{LinearAllocator, static_allocator};
/// static PERSISTENT_ARENA: &LinearAllocator = static_allocator!(1024 * 1024);
/// ```
#[macro_export]
macro_rules! static_allocator {
    ($size:expr) => {{
        static mut MEM: [u8; $size] = [0; $size];
        // Safety (LinearAllocator::from_raw_slice): MEM is only accessible in
        // this scope, and this scope only creates one allocator from it, since
        // the allocator is stored in a static variable, so MEM won't be shared.
        // Since MEM is a static variable, the pointer is valid for 'static.
        static ALLOCATOR: $crate::allocators::LinearAllocator =
            unsafe { $crate::allocators::LinearAllocator::from_raw_slice(&raw mut MEM) };
        &ALLOCATOR
    }};
}

pub use static_allocator;
