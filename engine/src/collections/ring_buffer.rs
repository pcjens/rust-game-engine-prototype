// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod boxed;
mod slice;

use core::{
    marker::PhantomData,
    mem::{transmute, MaybeUninit},
    sync::atomic::{AtomicUsize, Ordering},
};

use bytemuck::{fill_zeroes, Zeroable};
use platform::Box;

use crate::allocators::LinearAllocator;

pub use boxed::*;
pub use slice::*;

/// Metadata related to a specific allocation from a [`RingBuffer`].
#[derive(Debug)]
pub struct RingAllocationMetadata {
    pub(super) offset: usize,
    pub(super) padding: usize,
    pub(super) buffer_identifier: usize,
}

/// Ring buffer for allocating varying length byte slices in a sequential, FIFO
/// fashion.
///
/// Allocations are represented by [`RingSlice`]s, which are lifetimeless
/// handles to this buffer, and can be used to get a `&mut [u8]` that will hold
/// the memory until the [`RingSlice`] is passed into [`RingBuffer::free`]. The
/// slices must be freed in the same order as they were allocated. As such,
/// dropping a [`RingSlice`] will cause the slice to never be reclaimed, which
/// will "clog" the ring buffer.
///
/// The sum of the lengths of the slices allocated from this buffer, when full,
/// may be less than the total capacity, since the individual slices are
/// contiguous and can't span across the end of the backing buffer. These gaps
/// could be prevented with memory mapping trickery in the future.
pub struct RingBuffer<'a, T> {
    buffer_lifetime: PhantomData<&'a mut [T]>,
    buffer_ptr: *mut MaybeUninit<T>,
    buffer_len: usize,
    allocated_offset: usize,
    allocated_len: usize,
    buffer_identifier: usize,
}

fn make_buffer_id() -> usize {
    static BUFFER_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let prev_id = BUFFER_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    prev_id.checked_add(1).unwrap()
}

impl<T> RingBuffer<'_, T> {
    /// Allocates a new ring buffer with the given capacity.
    pub fn new(
        allocator: &'static LinearAllocator,
        capacity: usize,
    ) -> Option<RingBuffer<'static, T>> {
        let buffer = allocator.try_alloc_uninit_slice(capacity)?;
        Some(RingBuffer {
            buffer_lifetime: PhantomData,
            buffer_ptr: buffer.as_mut_ptr(),
            buffer_len: buffer.len(),
            allocated_offset: 0,
            allocated_len: 0,
            buffer_identifier: make_buffer_id(),
        })
    }

    /// Creates a new ring buffer with the given buffer.
    ///
    /// ### Safety
    ///
    /// All allocations made from this [`RingBuffer`] must be passed back into
    /// [`RingBuffer::free`] before it is dropped, as the backing memory is only
    /// borrowed for 'a.
    #[allow(clippy::needless_lifetimes)]
    pub unsafe fn from_mut<'a>(buffer: &'a mut [MaybeUninit<T>]) -> Option<RingBuffer<'a, T>> {
        Some(RingBuffer {
            buffer_lifetime: PhantomData,
            buffer_ptr: buffer.as_mut_ptr(),
            buffer_len: buffer.len(),
            allocated_offset: 0,
            allocated_len: 0,
            buffer_identifier: make_buffer_id(),
        })
    }

    /// If it fits, allocates `len` contiguous bytes and returns the offset and
    /// padding of the allocation.
    fn allocate_offset(&mut self, len: usize) -> Option<(usize, usize)> {
        let allocated_end = self.allocated_offset + self.allocated_len;
        let padding_to_end = self.buffer_len - (allocated_end % self.buffer_len);
        if allocated_end + len <= self.buffer_len {
            // The allocation fits between the current allocated slice's end and
            // the end of the buffer
            self.allocated_len += len;
            Some((allocated_end, 0))
        } else if self.allocated_len + padding_to_end + len <= self.buffer_len {
            // The slice fits even with padding added to the end so that the
            // allocated slice starts at the beginning
            self.allocated_len += padding_to_end + len;
            Some((0, padding_to_end))
        } else {
            None
        }
    }
}

impl<T: Zeroable> RingBuffer<'_, T> {
    /// Allocates and zeroes out a slice of the given length if there's enough
    /// contiguous free space.
    pub fn allocate(&mut self, len: usize) -> Option<RingSlice<T>> {
        let (offset, padding) = self.allocate_offset(len)?;

        // Safety: The offset is smaller than the length of the backing slice,
        // so it's definitely safe to offset by.
        let ptr = unsafe { self.buffer_ptr.add(offset) };

        // Safety: The offset allocation logic ensures that we create distinct
        // slices, so the slice created here does not alias with any other
        // slice. The pointer is not null since it's from a slice in the
        // constructor.
        let slice: &mut [MaybeUninit<T>] = unsafe { core::slice::from_raw_parts_mut(ptr, len) };

        fill_zeroes(slice);

        // Safety: fill_zeroes above initializes the whole slice.
        let slice = unsafe { transmute::<&mut [MaybeUninit<T>], &mut [T]>(slice) };

        // Safety: the constructors of the RingBuffer ensure that the memory is
        // not freed while this Box exists, and `RingBuffer::allocate_offset`
        // ensures that no aliasing allocations are created.
        let slice = unsafe { Box::from_ptr(&raw mut *slice) };

        Some(RingSlice {
            slice,
            metadata: RingAllocationMetadata {
                offset,
                padding,
                buffer_identifier: self.buffer_identifier,
            },
        })
    }

    /// Reclaims the memory occupied by the given slice. Returns the slice back
    /// in an `Err` if the slice isn't the current head of the allocated span,
    /// and the memory is not reclaimed.
    ///
    /// ### Panics
    ///
    /// Panics if the [`RingSlice`] was allocated from a different
    /// [`RingBuffer`].
    pub fn free(&mut self, slice: RingSlice<T>) -> Result<(), RingSlice<T>> {
        assert_eq!(
            self.buffer_identifier, slice.metadata.buffer_identifier,
            "this slice was not allocated from this ring buffer",
        );
        let allocated_offset_with_padding =
            (self.allocated_offset + slice.metadata.padding) % self.buffer_len;
        if slice.metadata.offset == allocated_offset_with_padding {
            let freed_len = slice.len();
            self.allocated_offset = (self.allocated_offset + freed_len) % self.buffer_len;
            self.allocated_len -= freed_len + slice.metadata.padding;
            if self.allocated_len == 0 {
                self.allocated_offset = 0;
            }
            Ok(())
        } else {
            Err(slice)
        }
    }
}

impl<T> RingBuffer<'_, T> {
    /// Allocates space for one T if there's free space, and boxes it.
    pub fn allocate_box(&mut self, value: T) -> Result<RingBox<T>, T> {
        let Some((offset, padding)) = self.allocate_offset(1) else {
            return Err(value);
        };

        // Safety: the offset is smaller than the length of the backing slice,
        // so it's definitely safe to offset by.
        let ptr = unsafe { self.buffer_ptr.add(offset) };

        // Safety: as established above, ptr points to a specific element in the
        // slice whose raw pointer `self.buffer_ptr` is, so this is a valid
        // reference.
        let uninit_mut: &mut MaybeUninit<T> = unsafe { &mut *ptr };

        let init_mut = uninit_mut.write(value);

        // Safety: the constructors of the RingBuffer ensure that the memory is
        // not freed while this Box exists, and `RingBuffer::allocate_offset`
        // ensures that no aliasing allocations are created.
        let boxed = unsafe { Box::from_ptr(&raw mut *init_mut) };

        Ok(RingBox {
            boxed,
            metadata: RingAllocationMetadata {
                offset,
                padding,
                buffer_identifier: self.buffer_identifier,
            },
        })
    }

    /// Reclaims the memory occupied by the given box. Returns the box back in
    /// an `Err` if the slice isn't the current head of the allocated span, and
    /// the memory is not reclaimed.
    ///
    /// ### Panics
    ///
    /// Panics if the [`RingBox`] was allocated from a different [`RingBuffer`].
    pub fn free_box(&mut self, boxed: RingBox<T>) -> Result<(), RingBox<T>> {
        assert_eq!(
            self.buffer_identifier, boxed.metadata.buffer_identifier,
            "this box was not allocated from this ring buffer",
        );
        if boxed.metadata.offset == self.allocated_offset {
            self.allocated_offset = (self.allocated_offset + 1) % self.buffer_len;
            self.allocated_len -= 1;
            Ok(())
        } else {
            Err(boxed)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::allocators::{static_allocator, LinearAllocator};

    use super::RingBuffer;

    #[test]
    fn works_at_all() {
        static ALLOC: &LinearAllocator = static_allocator!(1);
        let mut ring = RingBuffer::<u8>::new(ALLOC, 1).unwrap();
        let mut slice = ring.allocate(1).unwrap();
        slice[0] = 123;
        ring.free(slice).unwrap();
    }

    #[test]
    fn wraps_when_full() {
        static ALLOC: &LinearAllocator = static_allocator!(10);
        let mut ring = RingBuffer::<u8>::new(ALLOC, 10).unwrap();

        let first_half = ring.allocate(4).unwrap();
        let _second_half = ring.allocate(4).unwrap();
        assert!(ring.allocate(4).is_none(), "ring should be full");

        // Wrap:
        ring.free(first_half).unwrap();
        let _third_half = ring.allocate(4).unwrap();

        assert!(ring.allocate(4).is_none(), "ring should be full");
    }

    #[test]
    #[should_panic]
    fn panics_on_free_with_wrong_buffer_identity() {
        static ALLOC_0: &LinearAllocator = static_allocator!(1);
        static ALLOC_1: &LinearAllocator = static_allocator!(1);

        let mut ring0 = RingBuffer::<u8>::new(ALLOC_0, 1).unwrap();
        let mut ring1 = RingBuffer::<u8>::new(ALLOC_1, 1).unwrap();

        let foo0 = ring0.allocate(1).unwrap();
        let _ = ring1.free(foo0); // should panic
    }
}
