use core::num::NonZeroU32;

use bytemuck::Zeroable;

use crate::{allocators::LinearAllocator, collections::FixedVec};

/// Sparse array of `T`.
///
/// Can operate with less memory than a [`FixedVec`] of the same length, since
/// the array length and the backing `T` storage have separate capacities.
/// Useful for cases where stable indices are important, but having the entire
/// array in-memory is infeasible.
pub struct SparseArray<'eng, T> {
    /// The indirection array: contains indexes to `loaded_elements` for the
    /// array elements which are currently loaded.
    index_map: FixedVec<'eng, OptionalU32>,
    /// Reusable indexes to `loaded_elements`.
    free_indices: FixedVec<'eng, u32>,
    /// The currently loaded instances of `T`.
    loaded_elements: FixedVec<'eng, T>,
}

impl<T> SparseArray<'_, T> {
    /// Creates a new sparse array of `T` with length `array_len`, allowing for
    /// `loaded_len` elements to be loaded at a time.
    pub fn new<'a>(
        allocator: &'a LinearAllocator,
        array_len: u32,
        loaded_len: u32,
    ) -> Option<SparseArray<'a, T>> {
        let mut index_map = FixedVec::new(allocator, array_len as usize)?;
        index_map.fill_with_zeroes();

        Some(SparseArray {
            index_map,
            free_indices: FixedVec::new(allocator, loaded_len as usize)?,
            loaded_elements: FixedVec::new(allocator, loaded_len as usize)?,
        })
    }

    /// Removes the value from the index, freeing space for another
    /// value to be inserted anywhere.
    pub fn unload(&mut self, index: u32) {
        if let Some(resident_index) = self
            .index_map
            .get_mut(index as usize)
            .and_then(OptionalU32::take)
        {
            self.free_indices.push(resident_index).unwrap();
        }
    }

    /// Allocates space for the index if there's space, returning a mutable
    /// borrow to fill it with.
    ///
    /// If reuse is not possible and a new `T` needs to be created, `init_fn` is
    /// used. `init_fn` can fail by returning None, in which case nothing else
    /// happens and None is returned.
    pub fn insert(&mut self, index: u32, init_fn: impl FnOnce() -> Option<T>) -> Option<&mut T> {
        let now_resident_index = if let Some(reused_resident_index) = self.free_indices.pop() {
            reused_resident_index
        } else {
            let new_data = init_fn()?;
            let new_resident_index = self.loaded_elements.len() as u32;
            self.loaded_elements.push(new_data).ok()?;
            new_resident_index
        };
        self.index_map[index as usize].set(now_resident_index);
        Some(&mut self.loaded_elements[now_resident_index as usize])
    }

    /// Returns the value at the index if it's loaded.
    pub fn get(&self, index: u32) -> Option<&T> {
        let resident_index = self.index_map[index as usize].get()?;
        Some(&self.loaded_elements[resident_index as usize])
    }

    /// Returns the length of the whole array (not the amount of loaded
    /// elements).
    pub fn array_len(&self) -> usize {
        self.index_map.len()
    }
}

/// `Option<u32>` but Zeroable and u32-sized.
///
/// But can't contain `0xFFFFFFFF`.
#[derive(Zeroable, Clone, Copy)]
struct OptionalU32 {
    index_plus_one: Option<NonZeroU32>,
}

impl OptionalU32 {
    pub fn set(&mut self, index: u32) {
        self.index_plus_one = Some(NonZeroU32::new(index + 1).unwrap());
    }

    pub fn get(self) -> Option<u32> {
        self.index_plus_one
            .map(|index_plus_one| index_plus_one.get() - 1)
    }

    pub fn take(&mut self) -> Option<u32> {
        let result = self.get();
        self.index_plus_one = None;
        result
    }
}
