// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::{
    num::NonZeroU32,
    sync::atomic::{AtomicU32, Ordering},
};

use bytemuck::Zeroable;

use crate::{allocators::LinearAllocator, collections::FixedVec};

struct LoadedElementInfo {
    age: AtomicU32,
    array_index: u32,
}

impl LoadedElementInfo {
    fn new(array_index: u32) -> LoadedElementInfo {
        LoadedElementInfo {
            age: AtomicU32::new(0),
            array_index,
        }
    }
}

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
    /// Ages of loaded elements, each element being the age of the loaded
    /// element in the same index in `loaded_elements`.
    ///
    /// The values are atomic to allow reading from the [`SparseArray`] from
    /// multiple threads at once, while still being able to maintain the ages of
    /// the loaded elements.
    loaded_element_infos: FixedVec<'eng, LoadedElementInfo>,
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
            loaded_element_infos: FixedVec::new(allocator, loaded_len as usize)?,
        })
    }

    /// Increments the age of each loaded element.
    ///
    /// These ages are used to determine which elements get discarded first when
    /// calling [`SparseArray::insert`]. [`SparseArray::get`] resets the age of
    /// the returned element.
    pub fn increment_ages(&mut self) {
        for info in &mut *self.loaded_element_infos {
            let age = info.age.get_mut();
            *age = age.saturating_add(1);
        }
    }

    /// Removes the value from the index, freeing space for another
    /// value to be inserted anywhere.
    pub fn unload(&mut self, index: u32) {
        if let Some(loaded_index) = self
            .index_map
            .get_mut(index as usize)
            .and_then(OptionalU32::take)
        {
            self.free_indices.push(loaded_index).unwrap();
        }
    }

    /// Allocates space for the index, returning a mutable borrow to fill it
    /// with.
    ///
    /// If reusing an old unloaded `T` is not possible, but there's and a new
    /// `T` needs to be created, `init_fn` is used. `init_fn` can fail by
    /// returning `None`, in which case nothing else happens and `None` is
    /// returned.
    ///
    /// If the backing memory is full, the least recently used `T` is assigned
    /// to this index and returned, implicitly "unloading the value at its old
    /// index."
    ///
    /// If the backing memory is full, and every `T` has been used since the
    /// last call to [`SparseArray::increment_ages`], this returns `None`.
    pub fn insert(&mut self, index: u32, init_fn: impl FnOnce() -> Option<T>) -> Option<&mut T> {
        let now_loaded_index = if let Some(unloaded_index) = self.free_indices.pop() {
            unloaded_index
        } else if self.loaded_elements.is_full() {
            let mut least_recent_age = 0;
            let mut least_recent_loaded_index = None;
            for (i, info) in self.loaded_element_infos.iter_mut().enumerate() {
                let age = *info.age.get_mut();
                if age > least_recent_age {
                    least_recent_age = age;
                    least_recent_loaded_index = Some(i as u32);
                }
            }
            let least_recent_loaded_index = least_recent_loaded_index?;

            let info = &mut self.loaded_element_infos[least_recent_loaded_index as usize];
            self.index_map[info.array_index as usize].take();
            *info = LoadedElementInfo::new(index);

            least_recent_loaded_index
        } else {
            let new_data = init_fn()?;
            let new_loaded_index = self.loaded_elements.len() as u32;
            self.loaded_element_infos
                .push(LoadedElementInfo::new(index))
                .ok()
                .unwrap();
            self.loaded_elements.push(new_data).ok().unwrap();
            new_loaded_index
        };
        self.index_map[index as usize].set(now_loaded_index);
        Some(&mut self.loaded_elements[now_loaded_index as usize])
    }

    /// Returns the value at the index if it's loaded.
    pub fn get(&self, index: u32) -> Option<&T> {
        let loaded_index = self.index_map[index as usize].get()? as usize;
        self.loaded_element_infos[loaded_index]
            .age
            .store(0, Ordering::Release);
        Some(&self.loaded_elements[loaded_index])
    }

    /// Returns the length of the whole array (not the amount of loaded
    /// elements).
    pub fn array_len(&self) -> usize {
        self.index_map.len()
    }
}

/// `Option<u32>` but Zeroable and u32-sized.
///
/// But can't hold the value `0xFFFFFFFF`.
#[derive(Clone, Copy)]
struct OptionalU32 {
    /// Contains the value represented by this struct, except that the inner
    /// value from [`NonZeroU32::get`] is 1 more than the value this struct
    /// represents. [`OptionalU32::set`] and [`OptionalU32::get`] handle
    /// applying this bias in both directions.
    biased_index: Option<NonZeroU32>,
}

// Safety: OptionalU32 is inhabited and all zeroes is a valid value for it (it'd
// have `index_plus_one: None`). For another perspective, Option<T> is Zeroable
// if T is PodInOption, and NonZeroU32 is PodInOption.
unsafe impl Zeroable for OptionalU32 {}

impl OptionalU32 {
    pub fn set(&mut self, index: u32) {
        self.biased_index = Some(NonZeroU32::new(index + 1).unwrap());
    }

    pub fn get(self) -> Option<u32> {
        self.biased_index
            .map(|index_plus_one| index_plus_one.get() - 1)
    }

    pub fn take(&mut self) -> Option<u32> {
        let result = self.get();
        self.biased_index = None;
        result
    }
}
