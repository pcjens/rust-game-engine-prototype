// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::ops::{Deref, DerefMut};

use platform::Box;

use super::RingAllocationMetadata;

/// Owned pointer into a [`RingBuffer`](super::RingBuffer).
/// [`RingBuffer::free_box`](super::RingBuffer::free_box) instead of [`drop`]!
#[derive(Debug)]
pub struct RingBox<T: 'static> {
    pub(super) boxed: Box<T>,
    pub(super) metadata: RingAllocationMetadata,
}

impl<T> RingBox<T> {
    /// Splits this [`RingBox`] into its raw parts. Can be combined back with
    /// [`RingBox::from_parts`].
    ///
    /// Useful for cases where some API is expecting a [`Box`] and a deref isn't
    /// enough.
    pub fn into_parts(self) -> (Box<T>, RingAllocationMetadata) {
        (self.boxed, self.metadata)
    }

    /// ### Safety
    ///
    /// The parts passed in must be a pair returned by an earlier
    /// [`RingBox::into_parts`] call. Mixing up metadatas and boxes is not
    /// allowed, because it will result in aliased mutable borrows, so
    /// definitely very Undefined-Behavior.
    pub unsafe fn from_parts(boxed: Box<T>, metadata: RingAllocationMetadata) -> RingBox<T> {
        RingBox { boxed, metadata }
    }
}

impl<T> Deref for RingBox<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.boxed
    }
}

impl<T> DerefMut for RingBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.boxed
    }
}
