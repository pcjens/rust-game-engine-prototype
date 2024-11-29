use core::{
    mem::{needs_drop, transmute, MaybeUninit},
    ops::{Deref, DerefMut},
};

use super::Arena;

/// A fixed-capacity contiguous growable array type. Named like Vec since it's
/// used similarly, but this type does *not* allocate more memory as needed, it
/// **panics** if full. Allocates memory from an [Arena], so this is very cheap
/// to create.
pub struct FixedVec<'arena, T> {
    uninit_slice: &'arena mut [MaybeUninit<T>],
    initialized_len: usize,
}

impl<T> FixedVec<'_, T> {
    pub fn new<'arena>(arena: &'arena Arena, capacity: usize) -> FixedVec<'arena, T> {
        let uninit_slice: &'arena mut [MaybeUninit<T>] = arena.alloc_uninit_slice::<T>(capacity);
        FixedVec {
            uninit_slice,
            initialized_len: 0,
        }
    }

    /// Appends the value to the back of the array, panicing if there's no
    /// capacity left.
    pub fn push(&mut self, value: T) {
        if !self.try_push(value) {
            panic!("should not push values into a full FixedVec");
        }
    }

    /// Appends the value to the back of the array, returning `false` if there's
    /// no capacity left.
    #[must_use]
    pub fn try_push(&mut self, value: T) -> bool {
        // Pick index, check it fits:
        let i = self.initialized_len;
        let Some(uninit) = self.uninit_slice.get_mut(i) else {
            return false;
        };

        // Write the value at the index:
        uninit.write(value);
        // Notes:
        // - The "existing value" (`uninit`, uninitialized memory) does not get
        //   dropped here. Dropping uninitialized memory would be bad.
        //   - To avoid leaks, we should be sure that `uninit` here is actually
        //     uninitialized: if `uninit` is actually initialized here, it will
        //     never be dropped, i.e. it will be leaked. `uninit` is definitely
        //     uninitialized the first time we use any specific index, since we
        //     specifically allocate a slice of uninitialized MaybeUninits. On
        //     the subsequent times, we rely on the fact that all the functions
        //     that remove values from the array also drop the values.

        // The value at `i` is now initialized, update length:
        self.initialized_len = i + 1;

        true
    }

    /// Empties out the array, dropping the currently contained values.
    pub fn clear(&mut self) {
        self.truncate(0);
    }

    /// Shortens the array to be the given length if it's currently longer. Any
    /// values past the new length are dropped.
    pub fn truncate(&mut self, new_len: usize) {
        if new_len >= self.initialized_len {
            return;
        }

        if needs_drop::<T>() {
            for initialized_value in &mut self.uninit_slice[new_len..self.initialized_len] {
                // Safety: since we're iterating only up to
                // `self.initialized_len`, which only gets incremented as the
                // values are initialized, all of these [MaybeUninit]s must be
                // initialized.
                unsafe { initialized_value.assume_init_drop() };
            }
        }

        self.initialized_len = new_len;
    }
}

impl<T> Deref for FixedVec<'_, T> {
    type Target = [T];

    fn deref<'a>(&'a self) -> &'a Self::Target {
        let initialized_slice = &self.uninit_slice[..self.initialized_len];
        // Safety: `MaybeUninit<T>` is identical to `T` except that it might be
        // uninitialized, and all values up to `self.initialized_len` are
        // initialized.
        unsafe { transmute::<&'a [MaybeUninit<T>], &'a [T]>(initialized_slice) }
    }
}

impl<T> DerefMut for FixedVec<'_, T> {
    fn deref_mut<'a>(&'a mut self) -> &'a mut Self::Target {
        let initialized_slice = &mut self.uninit_slice[..self.initialized_len];
        // Safety: `MaybeUninit<T>` is identical to `T` except that it might be
        // uninitialized, and all values up to `self.initialized_len` are
        // initialized.
        unsafe { transmute::<&'a mut [MaybeUninit<T>], &'a mut [T]>(initialized_slice) }
    }
}
