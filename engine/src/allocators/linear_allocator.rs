// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::{
    ffi::c_void,
    fmt::Debug,
    marker::PhantomData,
    mem::{transmute, MaybeUninit},
    slice,
    sync::atomic::{AtomicUsize, Ordering},
};

use bytemuck::{fill_zeroes, Zeroable};
use platform::Box;

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

/// A linear allocator with a constant capacity. Can allocate memory regions
/// with any size or alignment (within the capacity) very fast, but individual
/// allocations can't be freed to make more space while there's still other
/// allocations in use.
pub struct LinearAllocator<'a> {
    backing_mem_lifetime_holder: PhantomData<&'a mut ()>,
    backing_mem_ptr: *mut c_void,
    backing_mem_size: usize,
    /// The amount of bytes allocated starting from `backing_mem_ptr`. Can
    /// overflow `backing_mem_size` when the allocator reaches capacity, but in
    /// such a case, we don't even create a reference to the out-of-bounds area
    /// of memory.
    allocated: AtomicUsize,
}

impl Debug for LinearAllocator<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LinearAllocator")
            .field("backing_mem_ptr", &self.backing_mem_ptr)
            .field("backing_mem_size", &self.backing_mem_size)
            .field("allocated", &self.allocated)
            .finish_non_exhaustive()
    }
}

/// Safety: the only non-Sync part of [`LinearAllocator`] is backing memory
/// pointer, which is fine to access from multiple threads simultaneously as the
/// regions of memory accessed via the pointer are distinct on every access, due
/// to the atomic incrementing of [`LinearAllocator::allocated`].
unsafe impl Sync for LinearAllocator<'_> {}

impl LinearAllocator<'_> {
    /// Creates a new [`LinearAllocator`] with `capacity` bytes of backing
    /// memory. Returns None if allocating the memory fails or if `capacity`
    /// overflows `isize`.
    ///
    /// See [`static_allocator`](super::static_allocator) for bootstrapping one
    /// of these.
    pub fn new<'a>(allocator: &'a LinearAllocator, capacity: usize) -> Option<LinearAllocator<'a>> {
        if capacity > isize::MAX as usize {
            // Practically never happens, but asserting this here helps avoid a
            // safety check later.
            return None;
        }

        let buffer: &'a mut [MaybeUninit<u8>] = allocator.try_alloc_uninit_slice(capacity, None)?;

        Some(LinearAllocator {
            backing_mem_lifetime_holder: PhantomData,
            backing_mem_ptr: buffer.as_mut_ptr() as *mut c_void,
            backing_mem_size: buffer.len(),
            allocated: AtomicUsize::new(0),
        })
    }

    /// Creates  a new [`LinearAllocator`] with as many bytes of backing memory
    /// as there are in the given slice.
    ///
    /// Only the first [`isize::MAX`] bytes of the slice are used if it's longer
    /// than that.
    ///
    /// ### Safety
    ///
    /// The `backing_slice` pointer must not be shared, nor the memory behind
    /// it, and it must live for as long as this allocator (and any allocations
    /// from it) live. Consider this function as taking ownership of the memory
    /// pointed to by it for 'static.
    pub const unsafe fn from_raw_slice(backing_slice: *mut [u8]) -> LinearAllocator<'static> {
        LinearAllocator {
            backing_mem_lifetime_holder: PhantomData,
            backing_mem_ptr: (*backing_slice).as_mut_ptr() as *mut c_void,
            backing_mem_size: if backing_slice.len() > isize::MAX as usize {
                isize::MAX as usize
            } else {
                backing_slice.len()
            },
            allocated: AtomicUsize::new(0),
        }
    }

    /// Returns an estimate of the amount of allocated memory currently, in
    /// bytes.
    ///
    /// An "estimate" since the value returned is from an [`Ordering::Relaxed`]
    /// atomic operation, which technically may return the wrong value even when
    /// using the allocator on a single thread due to funky out-of-order
    /// computing details. Still, the value can be considered accurate for some
    /// point in time.
    pub fn allocated(&self) -> usize {
        self.allocated
            .load(Ordering::Relaxed)
            .min(self.backing_mem_size)
    }

    /// Returns the total (free and allocated) amount of memory owned by this
    /// allocator, in bytes.
    pub fn total(&self) -> usize {
        self.backing_mem_size
    }

    /// Allocates memory for a `T` and returns a boxed version of it.
    pub fn try_alloc_box<T>(&'static self, value: T) -> Option<Box<T>> {
        let slice = self.try_alloc_uninit_slice(1, None)?;
        let (allocation, _) = slice.split_first_mut().unwrap();
        let allocation = allocation.write(value);
        Some(Box::from_mut(allocation))
    }

    /// Allocates memory for a `[T]` with `len` elements, zeroes it out, and
    /// returns a boxed version of it.
    pub fn try_alloc_boxed_slice_zeroed<T: Zeroable>(
        &'static self,
        len: usize,
    ) -> Option<Box<[T]>> {
        let slice = self.try_alloc_uninit_slice::<T>(len, None)?;
        fill_zeroes(slice);
        // Safety: the whole slice is initialized by the fill_zeroes above.
        let slice = unsafe { transmute::<&mut [MaybeUninit<T>], &mut [T]>(slice) };
        Some(Box::from_mut(slice))
    }

    /// Allocates memory for a `[T]` with `len` elements, fills it by calling
    /// `init`, and returns a boxed version of it.
    ///
    /// If `init` returns None for any invocation, this also returns None. Note
    /// that the already allocated memory isn't freed up in this case (due to
    /// [`LinearAllocator`] being strictly growing for thread-safety reasons).
    pub fn try_alloc_boxed_slice_with<T, F: FnMut() -> Option<T>>(
        &'static self,
        mut init: F,
        len: usize,
    ) -> Option<Box<[T]>> {
        let slice = self.try_alloc_uninit_slice::<T>(len, None)?;
        for uninit in &mut *slice {
            uninit.write(init()?);
        }
        // Safety: the whole slice is initialized by the loop above.
        let slice = unsafe { transmute::<&mut [MaybeUninit<T>], &mut [T]>(slice) };
        Some(Box::from_mut(slice))
    }

    /// Allocates memory for a slice of `MaybeUninit<T>`, leaving the contents
    /// of the slice uninitialized, returning None if there's not enough free
    /// memory.
    ///
    /// Note regardless of if the allocation is successful, `len` bytes are
    /// "allocated" from the allocation offset. This means that once this
    /// returns `None`, subsequent allocations will always fail until
    /// [`LinearAllocator::reset`].
    ///
    /// If `alignment` is Some, it will be used for alignment instead of `T`'s
    /// alignment. If the resulting alignment would result in `T` being
    /// unaligned, this function will panic.
    pub fn try_alloc_uninit_slice<'a, T>(
        &'a self,
        len: usize,
        alignment: Option<usize>,
    ) -> Option<&'a mut [MaybeUninit<T>]> {
        let alignment = if let Some(alignment) = alignment {
            assert_eq!(
                0,
                alignment % align_of::<T>(),
                "invalid manual alignment for allocation",
            );
            alignment
        } else {
            align_of::<T>()
        };

        let reserved_bytes = len * size_of::<T>() + alignment - 1;
        // This is a relaxed fetch_add since we don't really care about the
        // order of allocations, we don't have any other atomic operations to
        // order, all we care about is that we get distinct allocation offsets
        // between different calls to try_alloc_uninit_slice. `self.allocated`
        // may overflow, but that's simply taken as a signal that the allocator
        // is full.
        let allocation_unaligned_offset =
            self.allocated.fetch_add(reserved_bytes, Ordering::Relaxed);

        // Make sure the entire allocation fits in the backing memory.
        if allocation_unaligned_offset + reserved_bytes > self.backing_mem_size {
            return None;
        }

        // Safety:
        // - Due to the check above, we know the offset is less than
        //   `self.backing_mem_size`, which in turn is clamped to `isize::MAX`
        //   in the allocator constructor.
        // - Due to the same check above, we know the offset version of the
        //   pointer is still within the bounds of the allocated object.
        let unaligned_allocation_ptr =
            unsafe { self.backing_mem_ptr.byte_add(allocation_unaligned_offset) };

        // Figure out the properly aligned offset of the new allocation.
        let extra_offset_for_alignment = unaligned_allocation_ptr.align_offset(alignment);
        let allocation_aligned_offset =
            allocation_unaligned_offset.saturating_add(extra_offset_for_alignment);

        // Make sure the *aligned* allocation fits in the backing memory.
        if allocation_aligned_offset + len * size_of::<T>() > self.backing_mem_size {
            return None;
        }

        // Safety: exactly the same pattern and reasoning used for
        // `unaligned_allocation_ptr`, see the safety explanation for that. As a
        // slight addendum, note how the bounds check above takes into account
        // `the aligned offset + length * the size of T`, as that is the area of
        // memory we'll be creating a reference to.
        let aligned_allocation_ptr =
            unsafe { unaligned_allocation_ptr.byte_add(extra_offset_for_alignment) };

        let uninit_t_ptr = aligned_allocation_ptr as *mut MaybeUninit<T>;

        // Safety:
        // - `uninit_t_ptr` is non-null and valid for both reads and writes
        //   (which in turn have to follow MaybeUninit semantics, so we're
        //   "passing the unsafety" to the user of the slice).
        //   - The entire memory range of the slice is contained within a single
        //     allocated object, the malloc'd area of memory from the
        //     constructor.
        //   - `uninit_ptr` is non-null and aligned regardless of slice length
        //     of the size of T.
        // - `uninit_ptr` does point to `len` consecutive properly initialized
        //   values of type `MaybeUninit<T>`, because uninitialized values are
        //   valid for the type.
        // - The memory referenced by this slice is not accessed through any
        //   other pointer for the duration of lifetime 'a, since this pointer
        //   is derived from `self.allocated`, which has been bumped past the
        //   bounds of this slice, and is not reset until self is mutably
        //   borrowable again (i.e. after this slice has been dropped).
        // - `len * size_of::<MaybeUninit<T>>()` is not larger than
        //   `isize::MAX`, because it is not larger than `self.backing_mem_size`
        //   as checked above, and that in turn is checked to be no larger than
        //   `isize::MAX` in the constructor.
        let uninit_t_slice: &'a mut [MaybeUninit<T>] =
            unsafe { slice::from_raw_parts_mut(uninit_t_ptr, len) };

        Some(uninit_t_slice)
    }

    /// Resets the linear allocator, reclaiming all of the backing memory for
    /// future allocations.
    pub fn reset(&mut self) {
        // Safety: though this is not an unsafe operation, pretty much all the
        // unsafety in this file relies on `self.backing_mem_ptr +
        // self.allocated` to not point into memory which is already being
        // borrowed. Here's why we're not: We have a mutable borrow of self. =>
        // There's no other borrows of self. => There's no pointers to the
        // backing memory. (All previous allocations have lifetimes that cannot
        // outlive the related immutable borrow of this allocator.)
        //
        // Additionally, any atomic shenanigans between threads don't need to be
        // accounted for because we have an exclusive borrow of self, thus self
        // can't be shared between threads currently.
        self.allocated.store(0, Ordering::Release);
    }
}
