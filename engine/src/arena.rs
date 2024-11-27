use core::{cell::Cell, ffi::c_void};

use pal::Pal;

pub struct Arena {
    backing_mem_ptr: *mut u8,
    backing_mem_size: usize,
    /// It's not safe to free a pointer to memory which is in use.
    free_fn: unsafe fn(*mut c_void),

    allocated: Cell<usize>,
}

impl Drop for Arena {
    fn drop(&mut self) {
        // Safety: backing_mem_ptr is a private field, so the only way to
        // use it is via StackAllocator's API, which only deals out borrows
        // which cannot outlive the StackAllocator itself. So since we're in
        // the Drop impl, there must be no such borrows, i.e. nobody is
        // using the memory anymore.
        unsafe { (self.free_fn)(self.backing_mem_ptr as *mut c_void) };
    }
}

impl Arena {
    /// Creates a new [StackAllocator] with `capacity` bytes of backing memory.
    /// Returns None if allocating the memory fails or if `capacity` overflows
    /// `isize`.
    pub fn new<P: Pal>(capacity: usize) -> Option<Arena> {
        if capacity > isize::MAX as usize {
            // Practically never happens, but asserting this here helps avoid a
            // safety check later.
            return None;
        }
        let backing_mem_ptr = P::malloc(capacity) as *mut u8;
        if backing_mem_ptr.is_null() {
            return None;
        }
        Some(Arena {
            backing_mem_ptr,
            backing_mem_size: capacity,
            free_fn: P::free,

            allocated: Cell::new(0),
        })
    }

    pub fn alloc<T>(&self) -> Option<&mut T> {
        // Figure out the offset and size of the new allocation.
        let alignment = align_of::<T>();
        let size = size_of::<T>();
        let offset = self.allocated.get().next_multiple_of(alignment);
        if offset + size > self.backing_mem_size {
            // Bail if the allocation would not fit.
            return None;
        }

        // Advance the `allocated` offset by the size. Note that `allocated` is
        // in a Cell, which guarantees that nobody else is reading `allocated`
        // in between the `get()` above and the `set()` here.
        //
        // NOTE: `allocated` is also set in `StackAllocator::reset`, but that
        // function requires a mutable borrow to the StackAllocator, which in
        // turn means that there are no immutable borrows of StackAllocator
        // anymore, i.e. all previous return values of this function have been
        // dropped (the &mut T cannot outlive the &self).
        self.allocated.set(offset + size);

        // Safety:
        // - The computed offset does not overflow `isize`: `offset` (+ `size`)
        //   is guaranteed to be less or equal to `self.backing_mem_size`, which
        //   in turn is guaranteed (in `StackAllocator::new`) to be less or
        //   equal to `isize::MAX`. And the `size_of::<T>()` factor here is 1.
        // - `self.backing_mem_ptr` is a pointer to an allocated object (it's
        //   from a successful malloc), and `offset` is less than the amount of
        //   memory we asked for (checked above). So the entire memory range
        //   which should be in the bounds of the allocated object, is.
        let allocated_void_ptr = unsafe { self.backing_mem_ptr.add(offset) };

        let t_ptr = allocated_void_ptr as *mut T;
        // Safety:
        // - The pointer is properly aligned for T, as the base pointer is from
        //   `malloc`, which always produces a pointer that is aligned enough
        //   for anything, and the offset to that base pointer (the `offset`
        //   variable) is also a multiple of T's alignment.
        // - The pointer is non-null because the pointer we offset it from is
        //   also non-null (checked in StackAllocator::new).
        // - The pointer is *dereferenceable*: all of the memory between `t_ptr`
        //   and `t_ptr + size_of::<T>()` falls within the memory allocated with
        //   one `malloc` call and pointed to by `self.backing_mem_ptr` (a
        //   single allocated object).
        // - !! it is not a valid value of T, indeed. Time to  rethink a bit.
        //
        // Bullet points from:
        // https://doc.rust-lang.org/core/ptr/index.html#pointer-to-reference-conversion
        let t_borrow: &mut T = unsafe { &mut *t_ptr };

        Some(t_borrow)
    }

    pub fn reset(&mut self) {
        self.allocated.set(0);
    }
}
