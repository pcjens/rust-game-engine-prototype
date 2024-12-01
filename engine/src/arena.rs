mod vec;

use core::{cell::Cell, ffi::c_void, mem::MaybeUninit, slice};

use bytemuck::Zeroable;
use pal::Pal;

pub use vec::FixedVec;

/// A stack allocator with a constant capacity.
///
/// NOTE: The allocation functions return mutable borrows to the allocated
/// values, and [Arena::reset] does not drop those values. This means that the
/// values allocated from this arena are not dropped, unless mem::swap is
/// employed to extract the value out of the allocation, and then dropped.
///
/// However, you can allocate containers like [FixedVec] using the arena, and
/// those containers in turn may drop the values they contain, see their
/// documentation for details.
pub struct Arena<'platform> {
    backing_mem_ptr: *mut c_void,
    backing_mem_size: usize,
    platform: &'platform dyn Pal,

    allocated: Cell<usize>,
}

impl Drop for Arena<'_> {
    fn drop(&mut self) {
        // Safety: backing_mem_ptr is a private field, so the only way to use it
        // is via Arena's API, which only deals out borrows which cannot outlive
        // the Arena itself. So since we're in the Drop impl, there must be no
        // such borrows, i.e. nobody is using the memory anymore.
        unsafe {
            self.platform
                .free(self.backing_mem_ptr, self.backing_mem_size)
        };
    }
}

impl Arena<'_> {
    /// Creates a new [Arena] with `capacity` bytes of backing memory. Returns
    /// None if allocating the memory fails or if `capacity` overflows `isize`.
    pub fn new(platform: &dyn Pal, capacity: usize) -> Option<Arena> {
        if capacity > isize::MAX as usize {
            // Practically never happens, but asserting this here helps avoid a
            // safety check later.
            return None;
        }

        let backing_mem_ptr = platform.malloc(capacity);
        if backing_mem_ptr.is_null() {
            return None;
        }

        Some(Arena {
            backing_mem_ptr,
            backing_mem_size: capacity,
            platform,

            allocated: Cell::new(0),
        })
    }

    /// Allocates one `T` zeroed out. Returns None if there's not enough free
    /// memory left.
    pub fn alloc_zeroed<T: Zeroable>(&self) -> Option<&mut T> {
        self.alloc_with_initializer(initialize_memory_to_zero::<T>)
    }

    /// Allocates one `T` with its default value. Returns None if there's not
    /// enough free memory left.
    pub fn alloc_default<T: Default>(&self) -> Option<&mut T> {
        self.alloc_with_initializer(initialize_memory_to_default::<T>)
    }

    /// Allocates one `T` and moves `value` there. Returns None if there's not
    /// enough free memory left.
    pub fn alloc<T>(&self, value: T) -> Option<&mut T> {
        self.alloc_with_initializer(initialize_memory_by_move(value))
    }

    /// Allocates memory for a `T` and initializes the memory with
    /// `initialize_memory`. **Internal use only** because `initialize_memory`
    /// *must* initialize the memory given to it to contain a valid `T`,
    /// otherwise we've got UB.
    ///
    /// ## Safety
    ///
    /// - `initialize_memory` must initialize the memory where the given pointer
    ///   points to, to be a valid value of `T`.
    #[inline(always)]
    fn alloc_with_initializer<'a, T, F: FnOnce(*mut T)>(
        &'a self,
        initialize_memory: F,
    ) -> Option<&'a mut T> {
        // Figure out the properly aligned offset of the new allocation.
        let offset = self.allocated.get().next_multiple_of(align_of::<T>());
        if offset + size_of::<T>() > self.backing_mem_size {
            // Bail if the allocation would not fit.
            return None;
        }

        // Advance the `allocated` offset by the size. Note that `allocated` is
        // in a Cell, which guarantees that nobody else is reading `allocated`
        // in between the `get()` above and the `set()` here.
        //
        // NOTE: `allocated` is also set in `Arena::reset`, but that function
        // requires a mutable borrow to the Arena, which in turn means that
        // there are no immutable borrows of Arena anymore, i.e. all previous
        // return values of this function have been dropped (the &mut T cannot
        // outlive the &self).
        self.allocated.set(offset + size_of::<T>());

        // Safety:
        // - The computed offset does not overflow `isize`: `offset` (+ `size`)
        //   is guaranteed to be less or equal to `self.backing_mem_size`, which
        //   in turn is guaranteed to be less or equal to `isize::MAX` in
        //   `Arena::new`.
        // - `self.backing_mem_ptr` is a pointer to an allocated object (it's
        //   from a successful `malloc`), and `offset` is less than the amount
        //   of memory we asked for (checked above). So the memory range between
        //   `self.backing_mem_ptr` and the result is within the bounds of the
        //   allocated object.
        let allocated_void_ptr = unsafe { self.backing_mem_ptr.byte_add(offset) };

        let t_ptr = allocated_void_ptr as *mut T;
        initialize_memory(t_ptr);

        // Safety:
        // - The pointer is properly aligned for T, as the base pointer is from
        //   `malloc`, which always produces a pointer that is aligned enough
        //   for anything, and the offset to that base pointer (the `offset`
        //   variable) is also a multiple of T's alignment.
        // - The pointer is non-null because the pointer we offset it from is
        //   also non-null (checked in Arena::new).
        // - The pointer is *dereferenceable*: all of the memory between `t_ptr`
        //   and `t_ptr + size_of::<T>()` falls within the memory allocated with
        //   one `malloc` call and pointed to by `self.backing_mem_ptr` (a
        //   single allocated object).
        // - The `initialize_memory` function call above initializes the memory
        //   at `t_ptr` to a valid value of T.
        // - Rust's aliasing rules are enforced:
        //   - While this reference `&'a mut T` exists, this Arena is immutably
        //     borrowed (`&'a self`). The only way the memory pointed to by this
        //     reference can be accessed (except via this `&'a mut T`) is via
        //     another allocation *after the Arena has been reset,* but
        //     `Arena::reset` requires a mutable borrow of Arena. This is
        //     enforced by only creating new borrows based on the value of
        //     `self.allocated`, which is carefully bumped to never overlap
        //     regions of memory between allocations, see the relevant comments
        //     above.
        //
        // Bullet points from:
        // https://doc.rust-lang.org/core/ptr/index.html#pointer-to-reference-conversion
        let t_borrow: &'a mut T = unsafe { &mut *t_ptr };

        Some(t_borrow)
    }

    /// Allocates memory for a slice of `MaybeUninit<T>`, leaving the contents
    /// of the slice uninitialized.
    #[inline(always)]
    fn try_alloc_uninit_slice<'a, T>(&'a self, len: usize) -> Option<&'a mut [MaybeUninit<T>]> {
        // NOTE: This first part is the same as alloc_with_initializer, except
        // that the size is multiplied by `len`.
        let offset = self.allocated.get().next_multiple_of(align_of::<T>());
        if offset + len * size_of::<T>() > self.backing_mem_size {
            return None;
        }
        self.allocated.set(offset + len * size_of::<T>());
        // Safety: see alloc_with_initializer, it's the same up to this point.
        let allocated_void_ptr = unsafe { self.backing_mem_ptr.byte_add(offset) };
        let uninit_t_ptr = allocated_void_ptr as *mut MaybeUninit<T>;

        // NOTE: Here starts the differing section. Instead of initializing the
        // memory and making a borrow out of the pointer, this function just
        // makes a slice out of the pointer and the length we reserved after
        // that pointer:

        // Safety:
        // - `uninit_t_ptr` is non-null and valid for both reads and writes
        //   (which in turn have to follow MaybeUninit semantics, so we're
        //   "passing the unsafety" to the user of the slice).
        //   - The entire memory range of the slice is contained within a single
        //     allocated object, the malloc'd area of memory from `Arena::new`.
        //   - `uninit_ptr` is non-null and aligned regardless of slice length
        //     of the size of T.
        // - `uninit_ptr` does point to `len` consecutive properly initialized
        //   values of type `MaybeUninit<T>`, because uninitialized values are
        //   valid for the type.
        // - The memory referenced by this slice is not accessed through any
        //   other pointer for the duration of lifetime 'a, since this pointer
        //   is derived from `self.allocated`, which has been bumped past the
        //   bounds of this slice, and is not reset until Arena is mutably
        //   borrowable again (i.e. after this slice has been dropped).
        // - `len * size_of::<MaybeUninit<T>>()` is not larger than isize::MAX,
        //   because it is not larger than `self.backing_mem_size` as checked
        //   above, and that in turn is checked to be no larger than isize::MAX
        //   in `Arena::new`.
        let uninit_t_slice: &'a mut [MaybeUninit<T>] =
            unsafe { slice::from_raw_parts_mut(uninit_t_ptr, len) };

        Some(uninit_t_slice)
    }

    /// Resets the arena memory, reclaiming all of the backing memory for future
    /// allocation. **NOTE:** While the memory is reclaimed, the values are not
    /// dropped, i.e. they are leaked.
    pub fn reset(&mut self) {
        // Safety: though this is not an unsafe operation, pretty much all the
        // unsafety in this file relies on `self.backing_mem_ptr +
        // self.allocated` to not point in memory referenced by the mutable
        // borrows dealt out by Arena. So, the safety of this assignment: we
        // have a mutable borrow of self, which means that all the mutable
        // borrows we've dealt out no longer exist, since those borrows cannot
        // outlive the immutable borrow of Arena required by the allocation
        // functions.
        self.allocated = Cell::new(0);
    }
}

#[inline(always)]
fn initialize_memory_to_zero<T: Zeroable>(ptr: *mut T) {
    // Safety:
    // - `t_ptr` is valid for writes up the size of T, since it's offset from a
    //   valid pointer allocated by `malloc`, and we check above the `offset +
    //   size_of::<T>()` fits within the allocation.
    // - The pointer is properly aligned for the writes, since
    //   `self.backing_mem_ptr` is properly aligned for anything (due to being
    //   produced by `malloc`, which is apparently guaranteed to be aligned for
    //   any type Foo in a usage like `(Foo*)malloc(..)`), and the offset is
    //   aligned to `T`'s alignment.
    //
    // Not really related to write_bytes safety, but the pointer will point to a
    // valid value of T after this, since T: Zeroable and we write zeroes.
    unsafe { ptr.write_bytes(0, 1) };
}

#[inline(always)]
fn initialize_memory_to_default<T: Default>(ptr: *mut T) {
    // Safety: the logic is exactly the same as in initialize_memory_to_zero.
    // `write_bytes` has the same safety rules as `write`, it just writes zeroes
    // instead of some existing value of T.
    unsafe { ptr.write(T::default()) };
}

#[inline(always)]
fn initialize_memory_by_move<T>(value: T) -> impl FnOnce(*mut T) {
    move |ptr| {
        // Safety: the logic is exactly the same as in
        // initialize_memory_to_zero. `write_bytes` has the same safety rules as
        // `write`, it just writes zeroes instead of some existing value of T.
        unsafe { ptr.write(value) };
    }
}
