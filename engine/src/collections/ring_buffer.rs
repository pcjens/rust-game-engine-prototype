use core::{
    mem::{transmute, MaybeUninit},
    sync::atomic::{AtomicUsize, Ordering},
};

use bytemuck::fill_zeroes;

use crate::allocators::LinearAllocator;

use super::Queue;

/// Owned slice of a [`RingBuffer`]. [`RingBuffer::free`] instead of [`drop`]!
#[derive(Debug)]
pub struct RingSlice {
    offset: usize,
    len: usize,
    buffer_identifier: usize,
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
pub struct RingBuffer<'a> {
    buffer: &'a mut [u8],
    allocated_offset: usize,
    allocated_len: usize,
    buffer_identifier: usize,
}

fn make_buffer_id() -> usize {
    static BUFFER_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
    BUFFER_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

impl RingBuffer<'_> {
    /// Creates a new ring buffer using the given buffer as the backing memory.
    /// Does not zero the contents.
    pub fn from_mut(buffer: &mut [u8]) -> RingBuffer {
        RingBuffer {
            allocated_offset: 0,
            allocated_len: 0,
            buffer_identifier: make_buffer_id(),
            buffer,
        }
    }

    /// Allocates and zeroes out a new ring buffer with the given capacity.
    pub fn new<'a>(allocator: &'a LinearAllocator, capacity: usize) -> Option<RingBuffer<'a>> {
        let buffer = allocator.try_alloc_uninit_slice(capacity)?;
        fill_zeroes(buffer);
        // Safety: `fill_zeroes` initializes the whole slice, and transmuting a
        // `&mut [MaybeUninit<u8>]` to `&mut [u8]` is safe if it's initialized.
        let buffer = unsafe { transmute::<&mut [MaybeUninit<u8>], &mut [u8]>(buffer) };

        Some(RingBuffer {
            allocated_offset: 0,
            allocated_len: 0,
            buffer_identifier: make_buffer_id(),
            buffer,
        })
    }

    /// Allocates a slice of the given length if there's enough contiguous free
    /// space. Note the slice may have been used previously, in which case the
    /// contents may not be zeroed/defaulted.
    pub fn allocate(&mut self, len: usize) -> Option<RingSlice> {
        let allocated_end = self.allocated_offset + self.allocated_len;
        let padding_to_end = self.buffer.len() - (allocated_end % self.buffer.len());
        if allocated_end + len <= self.buffer.len() {
            // The allocation fits between the current allocated slice's end and
            // the end of the buffer
            self.allocated_len += len;
            Some(RingSlice {
                offset: allocated_end,
                len,
                buffer_identifier: self.buffer_identifier,
            })
        } else if self.allocated_len + padding_to_end + len <= self.buffer.len() {
            // The slice fits even with padding added to the end so that the
            // allocated slice starts at the beginning
            self.allocated_len += padding_to_end + len;
            Some(RingSlice {
                offset: 0,
                len,
                buffer_identifier: self.buffer_identifier,
            })
        } else {
            None
        }
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
            self.buffer_identifier, slice.buffer_identifier,
            "the given ring slice was not allocated from this ring buffer",
        );
        if slice.offset == self.allocated_offset {
            self.allocated_offset += slice.len;
            self.allocated_len -= slice.len;
            Ok(())
        } else {
            Err(slice)
        }
    }

    /// Returns the slice represented by the [`RingSlice`].
    ///
    /// ### Panics
    ///
    /// Panics if the [`RingSlice`] was allocated from a different
    /// [`RingBuffer`].
    pub fn get_mut(&mut self, slice: &RingSlice) -> &mut [u8] {
        assert_eq!(
            self.buffer_identifier, slice.buffer_identifier,
            "the given ring slice was not allocated from this ring buffer",
        );
        &mut self.buffer[slice.offset..slice.offset + slice.len]
    }

    /// Returns true if `allocate(len)` would succeed if called after this.
    pub fn would_fit(&mut self, len: usize) -> bool {
        let fits_at_end = self.allocated_offset + self.allocated_len + len <= self.buffer.len();
        let fits_at_start = len <= self.allocated_offset;
        fits_at_start || fits_at_end
    }

    /// Pushes mutable slices represented by the [`RingSlice`]s into the
    /// [`Queue`].
    ///
    /// `accessors` will be sorted to be in splitting order.
    ///
    /// ### Panics
    ///
    /// Panics if any of the [`RingSlice`]s were allocated from a different
    /// [`RingBuffer`].
    pub fn get_many_mut<'a>(
        &'a mut self,
        accessors: &mut [&RingSlice],
        slices: &mut Queue<'_, &'a mut [u8]>,
    ) {
        assert!(
            accessors.len() <= slices.spare_capacity(),
            "the result queue cannot fit all of the slices this would split into",
        );

        for accessor in &*accessors {
            assert_eq!(
                self.buffer_identifier, accessor.buffer_identifier,
                "the given ring slice was not allocated from this ring buffer",
            );
        }

        accessors.sort_unstable_by_key(|slice| slice.offset);

        let mut splitting_buffer = &mut *self.buffer;
        let mut split_off_so_far = 0;
        for accessor in &*accessors {
            if split_off_so_far < accessor.offset {
                // `split_off_so_far..accessor.offset` isn't covered by the
                // given slices, so split that part off `splitting_buffer`.
                (_, splitting_buffer) =
                    splitting_buffer.split_at_mut(accessor.offset - split_off_so_far);
                split_off_so_far = accessor.offset;
            }

            let (slice, the_rest) = splitting_buffer.split_at_mut(accessor.len);
            splitting_buffer = the_rest;
            split_off_so_far += accessor.len;

            slices.push_back(slice).unwrap();
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
        let ring_from_alloc = RingBuffer::new(&alloc, 1).unwrap();

        let mut buffer = [0];
        let ring_from_buffer = RingBuffer::from_mut(&mut buffer);

        for mut ring in [ring_from_alloc, ring_from_buffer] {
            let foo = ring.allocate(1).unwrap();
            let slice = ring.get_mut(&foo);
            slice[0] = 123;
            ring.free(foo).unwrap();
        }
    }

    #[test]
    fn wraps_when_full() {
        let mut buffer = [0; 10];
        let mut ring = RingBuffer::from_mut(&mut buffer);

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
        let mut buffer0 = [0; 1];
        let mut buffer1 = [0; 1];
        let mut ring0 = RingBuffer::from_mut(&mut buffer0);
        let mut ring1 = RingBuffer::from_mut(&mut buffer1);

        let foo0 = ring0.allocate(1).unwrap();
        ring1.get_mut(&foo0); // should panic
    }

    #[test]
    #[should_panic]
    fn panics_on_wrong_buffer_identity_free() {
        let mut buffer0 = [0; 1];
        let mut buffer1 = [0; 1];
        let mut ring0 = RingBuffer::from_mut(&mut buffer0);
        let mut ring1 = RingBuffer::from_mut(&mut buffer1);

        let foo0 = ring0.allocate(1).unwrap();
        let _ = ring1.free(foo0); // should panic
    }
}
