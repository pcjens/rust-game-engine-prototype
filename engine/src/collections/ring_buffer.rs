use core::{
    mem::{transmute, MaybeUninit},
    sync::atomic::{AtomicUsize, Ordering},
};

use bytemuck::{fill_zeroes, Zeroable};

use crate::allocators::LinearAllocator;

/// Owned slice of a [`RingBuffer`]. [`RingBuffer::free`] instead of [`drop`]!
#[derive(Debug)]
pub struct RingSlice {
    start: usize,
    end: usize,
    buffer_identifier: usize,
}

/// Ring buffer for allocating varying length slices in a sequential, FIFO
/// fashion.
///
/// Allocations are represented by [`RingSlice`]s, which are lifetimeless
/// handles to this buffer, and can be used to get a `&mut [T]` that will hold
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
    buffer: &'a mut [T],
    allocated_start: usize,
    allocated_end: usize,
    buffer_identifier: usize,
}

impl<T: Zeroable> RingBuffer<'_, T> {
    /// Allocates and zeroes out a new ring buffer with the given capacity.
    pub fn new<'a>(allocator: &'a LinearAllocator, capacity: usize) -> Option<RingBuffer<'a, T>> {
        let buffer = allocator.try_alloc_uninit_slice(capacity)?;
        fill_zeroes(buffer);
        // Safety: `fill_zeroes` initializes the whole slice, and transmuting a
        // `&mut [MaybeUninit<T>]` to `&mut [T]` is safe if it's initialized.
        let buffer = unsafe { transmute::<&mut [MaybeUninit<T>], &mut [T]>(buffer) };

        static BUFFER_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
        let buffer_identifier = BUFFER_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

        Some(RingBuffer {
            allocated_start: buffer.len(),
            allocated_end: 0,
            buffer_identifier,
            buffer,
        })
    }
}

impl<T> RingBuffer<'_, T> {
    /// Allocates a slice of the given length if there's enough contiguous free
    /// space. Note the slice may have been used previously, in which case the
    /// contents may not be zeroed/defaulted.
    pub fn allocate(&mut self, len: usize) -> Option<RingSlice> {
        if len > self.buffer.len() {
            return None;
        }

        let start = 'find_start: {
            let mut alloc_start = self.allocated_end;

            // Case A: the allocated span does not wrap across the end, consider (end..)
            if self.allocated_start < alloc_start {
                let free_space = self.buffer.len() - alloc_start;
                if len <= free_space {
                    break 'find_start alloc_start;
                } else {
                    // Not enough space: try with the start offset wrapped to the beginning of the slice
                    alloc_start = 0;
                }
            }

            // Case B: the allocated span does wrap across the end, consider (end..start)
            let free_space = self.allocated_start - alloc_start;
            if len <= free_space {
                break 'find_start alloc_start;
            }

            return None;
        };

        let end = start + len;
        self.allocated_end = end;
        Some(RingSlice {
            start,
            end,
            buffer_identifier: self.buffer_identifier,
        })
    }

    /// Reclaims the memory occupied by the given slice. Returns the slice back
    /// in an `Err` if the slice isn't the current head of the allocated span,
    /// and the memory is not reclaimed.
    ///
    /// # Panics
    ///
    /// Panics if the [`RingSlice`] was allocated from a different
    /// [`RingBuffer`].
    pub fn free(&mut self, slice: RingSlice) -> Result<(), RingSlice> {
        assert_eq!(
            self.buffer_identifier, slice.buffer_identifier,
            "the given ring slice was not allocated from this ring buffer",
        );
        if slice.start == self.allocated_start % self.buffer.len() {
            self.allocated_start = slice.end;
            Ok(())
        } else {
            Err(slice)
        }
    }

    /// Returns the actual slice represented by the [`RingSlice`].
    ///
    /// # Panics
    ///
    /// Panics if the [`RingSlice`] was allocated from a different
    /// [`RingBuffer`].
    pub fn get_mut(&mut self, slice: &RingSlice) -> &mut [T] {
        assert_eq!(
            self.buffer_identifier, slice.buffer_identifier,
            "the given ring slice was not allocated from this ring buffer",
        );
        &mut self.buffer[slice.start..slice.end]
    }

    /// Returns true if `allocate(len)` would succeed if called after this.
    pub fn would_fit(&mut self, len: usize) -> bool {
        if self.allocated_start < self.allocated_end {
            len < self.allocated_start || len < self.buffer.len() - self.allocated_end
        } else {
            len <= self.allocated_start - self.allocated_end
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{allocators::LinearAllocator, test_platform::TestPlatform};

    use super::RingBuffer;

    #[test]
    fn works_at_all() {
        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, 1).unwrap();
        let mut ring = RingBuffer::<u8>::new(&alloc, 1).unwrap();

        let foo = ring.allocate(1).unwrap();
        let slice = ring.get_mut(&foo);
        slice[0] = 123;
        ring.free(foo).unwrap();
    }

    #[test]
    fn wraps_when_full() {
        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, 10).unwrap();
        let mut ring = RingBuffer::<u8>::new(&alloc, 10).unwrap();

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
    fn panics_on_wrong_buffer_identity_get() {
        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, 1).unwrap();

        let mut ring0 = RingBuffer::<u8>::new(&alloc, 1).unwrap();
        let mut ring1 = RingBuffer::<u8>::new(&alloc, 1).unwrap();

        let foo0 = ring0.allocate(1).unwrap();
        ring1.get_mut(&foo0); // should panic
    }

    #[test]
    #[should_panic]
    fn panics_on_wrong_buffer_identity_free() {
        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, 1).unwrap();

        let mut ring0 = RingBuffer::<u8>::new(&alloc, 1).unwrap();
        let mut ring1 = RingBuffer::<u8>::new(&alloc, 1).unwrap();

        let foo0 = ring0.allocate(1).unwrap();
        let _ = ring1.free(foo0); // should panic
    }
}
