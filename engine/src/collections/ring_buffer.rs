use core::{
    mem::{transmute, MaybeUninit},
    ops::{Deref, DerefMut},
    slice,
    sync::atomic::{AtomicUsize, Ordering},
};

use bytemuck::fill_zeroes;
use platform_abstraction_layer::Box;

use crate::allocators::StaticAllocator;

/// Metadata related to a specific [`RingSlice`].
#[derive(Debug)]
pub struct RingSliceMetadata {
    offset: usize,
    buffer_identifier: usize,
}

/// Owned slice of a [`RingBuffer`]. [`RingBuffer::free`] instead of [`drop`]!
#[derive(Debug)]
pub struct RingSlice {
    slice: Box<[u8]>,
    metadata: RingSliceMetadata,
}

impl RingSlice {
    pub fn into_parts(self) -> (Box<[u8]>, RingSliceMetadata) {
        (self.slice, self.metadata)
    }

    /// ### Safety
    ///
    /// The parts passed in must be a pair returned by an earlier
    /// [`RingSlice::into_parts`] call. Mixing up metadatas and slices is not
    /// allowed, because it will result in aliased mutable borrows, so
    /// definitely very Undefined-Behavior.
    pub unsafe fn from_parts(slice: Box<[u8]>, metadata: RingSliceMetadata) -> RingSlice {
        RingSlice { slice, metadata }
    }
}

impl Deref for RingSlice {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.slice
    }
}

impl DerefMut for RingSlice {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.slice
    }
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
pub struct RingBuffer {
    buffer_ptr: *mut u8,
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

impl RingBuffer {
    /// Allocates and zeroes out a new ring buffer with the given capacity.
    pub fn new(allocator: &'static StaticAllocator, capacity: usize) -> Option<RingBuffer> {
        let buffer = allocator.try_alloc_uninit_slice(capacity)?;
        fill_zeroes(buffer);
        // Safety: `fill_zeroes` initializes the whole slice, and transmuting a
        // `&mut [MaybeUninit<u8>]` to `&mut [u8]` is safe if it's initialized.
        let buffer = unsafe { transmute::<&mut [MaybeUninit<u8>], &mut [u8]>(buffer) };
        Some(RingBuffer {
            buffer_ptr: buffer.as_mut_ptr(),
            buffer_len: buffer.len(),
            allocated_offset: 0,
            allocated_len: 0,
            buffer_identifier: make_buffer_id(),
        })
    }

    /// Allocates a slice of the given length if there's enough contiguous free
    /// space.
    ///
    /// Note that the slice may have been used previously, in which case the
    /// contents may not be zeroed.
    pub fn allocate(&mut self, len: usize) -> Option<RingSlice> {
        let allocated_end = self.allocated_offset + self.allocated_len;
        let padding_to_end = self.buffer_len - (allocated_end % self.buffer_len);
        let offset = if allocated_end + len <= self.buffer_len {
            // The allocation fits between the current allocated slice's end and
            // the end of the buffer
            self.allocated_len += len;
            allocated_end
        } else if self.allocated_len + padding_to_end + len <= self.buffer_len {
            // The slice fits even with padding added to the end so that the
            // allocated slice starts at the beginning
            self.allocated_len += padding_to_end + len;
            0
        } else {
            return None;
        };

        // Safety: The offset is smaller than the length of the backing slice,
        // so it's definitely safe to offset by.
        let ptr = unsafe { self.buffer_ptr.byte_add(offset) };
        // Safety: The above allocation logic ensures that we create distinct
        // slices, so the slice created here does not alias with any other
        // slice. The pointer is not null since it's from a slice in the
        // constructor. The borrow is also valid for &'static since the buffer
        // is specifically allocated for 'static in the constructor.
        let slice: &'static mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        Some(RingSlice {
            slice: Box::from_mut(slice),
            metadata: RingSliceMetadata {
                offset,
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
    pub fn free(&mut self, slice: RingSlice) -> Result<(), RingSlice> {
        assert_eq!(
            self.buffer_identifier, slice.metadata.buffer_identifier,
            "the given ring slice was not allocated from this ring buffer",
        );
        if slice.metadata.offset == self.allocated_offset {
            let freed_len = slice.len();
            self.allocated_offset += freed_len;
            self.allocated_len -= freed_len;
            Ok(())
        } else {
            Err(slice)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{allocators::StaticAllocator, static_allocator};

    use super::RingBuffer;

    #[test]
    fn works_at_all() {
        static ALLOC: &StaticAllocator = static_allocator!(1);
        let mut ring = RingBuffer::new(ALLOC, 1).unwrap();
        let mut slice = ring.allocate(1).unwrap();
        slice[0] = 123;
        ring.free(slice).unwrap();
    }

    #[test]
    fn wraps_when_full() {
        static ALLOC: &StaticAllocator = static_allocator!(10);
        let mut ring = RingBuffer::new(ALLOC, 10).unwrap();

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
        static ALLOC_0: &StaticAllocator = static_allocator!(1);
        static ALLOC_1: &StaticAllocator = static_allocator!(1);

        let mut ring0 = RingBuffer::new(ALLOC_0, 1).unwrap();
        let mut ring1 = RingBuffer::new(ALLOC_1, 1).unwrap();

        let foo0 = ring0.allocate(1).unwrap();
        let _ = ring1.free(foo0); // should panic
    }
}
