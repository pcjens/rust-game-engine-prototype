mod vec;

use core::{cell::Cell, ffi::c_void, mem::MaybeUninit, slice};

use pal::Pal;

pub use vec::FixedVec;

/// A linear allocator with a constant capacity. Can allocate memory regions
/// with any size or alignment very fast, but individual allocations can't be
/// freed, all of the allocations must be freed at once.
pub struct LinearAllocator<'platform> {
    backing_mem_ptr: *mut c_void,
    backing_mem_size: usize,
    platform: &'platform dyn Pal,

    allocated: Cell<usize>,
}

impl Drop for LinearAllocator<'_> {
    fn drop(&mut self) {
        self.reset();
        // Safety: reset "frees" everything, so we can be sure that there's no
        // pointers to the memory backed by this pointer anymore, so it's safe
        // to free. See further safety explanation in the reset implementation.
        unsafe {
            self.platform
                .free(self.backing_mem_ptr, self.backing_mem_size);
        }
    }
}

impl LinearAllocator<'_> {
    /// Creates a new [LinearAllocator] with `capacity` bytes of backing memory.
    /// Returns None if allocating the memory fails or if `capacity` overflows
    /// `isize`.
    pub fn new(platform: &dyn Pal, capacity: usize) -> Option<LinearAllocator> {
        if capacity > isize::MAX as usize {
            // Practically never happens, but asserting this here helps avoid a
            // safety check later.
            return None;
        }

        let backing_mem_ptr = platform.malloc(capacity);
        if backing_mem_ptr.is_null() {
            return None;
        }

        Some(LinearAllocator {
            backing_mem_ptr,
            backing_mem_size: capacity,
            platform,

            allocated: Cell::new(0),
        })
    }

    /// Allocates memory for a slice of `MaybeUninit<T>`, leaving the contents
    /// of the slice uninitialized, returning None if there's not enough free
    /// memory.
    pub fn try_alloc_uninit_slice<'a, T>(&'a self, len: usize) -> Option<&'a mut [MaybeUninit<T>]> {
        // Figure out the properly aligned offset of the new allocation.
        let offset = self.allocated.get().next_multiple_of(align_of::<T>());
        if offset + len * size_of::<T>() > self.backing_mem_size {
            // Bail if the allocation would not fit.
            return None;
        }

        // Advance the `allocated` offset by the size. Note that `allocated` is
        // in a Cell, which guarantees that nobody else is reading `allocated`
        // in between the `get()` above and the `set()` here. Also note that
        // this value only goes up, which ensures that allocations don't
        // overlap. The reset function does reset this, see the safety
        // explanation in its body.
        self.allocated.set(offset + len * size_of::<T>());

        // Safety:
        // - The computed offset does not overflow `isize`: `offset + len *
        //   size`) is guaranteed to not be larger than `self.backing_mem_size`,
        //   which in turn is guaranteed to not be larger than `isize::MAX` in
        //   the constructor.
        // - `self.backing_mem_ptr` is a pointer to an allocated object (it's
        //   from a successful `malloc`), and `offset` is less than the amount
        //   of memory we asked for (checked above). So the memory range between
        //   `self.backing_mem_ptr` and the result is within the bounds of the
        //   allocated object.
        let allocated_void_ptr = unsafe { self.backing_mem_ptr.byte_add(offset) };

        let uninit_t_ptr = allocated_void_ptr as *mut MaybeUninit<T>;

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
        self.allocated = Cell::new(0);
    }
}
