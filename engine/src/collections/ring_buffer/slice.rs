// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::ops::{Deref, DerefMut};

use platform::Box;

use super::RingAllocationMetadata;

#[allow(unused_imports)] // used in docs
use super::RingBuffer;

/// Owned slice of a [`RingBuffer`]. [`RingBuffer::free`] instead of [`drop`]!
#[derive(Debug)]
pub struct RingSlice<T: 'static> {
    pub(super) slice: Box<[T]>,
    pub(super) metadata: RingAllocationMetadata,
}

impl<T> RingSlice<T> {
    /// Splits this [`RingSlice`] into its raw parts. Can be combined back with
    /// [`RingSlice::from_parts`].
    ///
    /// Useful for cases where some API is expecting a [`Box`] and a deref isn't
    /// enough.
    pub fn into_parts(self) -> (Box<[T]>, RingAllocationMetadata) {
        (self.slice, self.metadata)
    }

    /// ### Safety
    ///
    /// The parts passed in must be a pair returned by an earlier
    /// [`RingSlice::into_parts`] call. Mixing up metadatas and slices is not
    /// allowed, because it will result in aliased mutable borrows, so
    /// definitely very Undefined-Behavior.
    pub unsafe fn from_parts(slice: Box<[T]>, metadata: RingAllocationMetadata) -> RingSlice<T> {
        RingSlice { slice, metadata }
    }
}

impl<T> Deref for RingSlice<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.slice
    }
}

impl<T> DerefMut for RingSlice<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.slice
    }
}
