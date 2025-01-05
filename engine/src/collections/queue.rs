use core::mem::{transmute, MaybeUninit};

use crate::allocators::LinearAllocator;

/// Bounded FIFO queue of `T`.
pub struct Queue<'a, T> {
    /// Backing memory. Invariant: everything from index `init_offset`
    /// (inclusive) to `(init_offset + init_len) % uninit_slice.len()`
    /// (exclusive, possibly wrapping around the end of the slice) is
    /// initialized, and the rest is uninitialized.
    uninit_slice: &'a mut [MaybeUninit<T>],
    initialized_offset: usize,
    initialized_len: usize,
}

impl<T> Queue<'_, T> {
    pub fn new<'a>(allocator: &'a LinearAllocator, capacity: usize) -> Option<Queue<'a, T>> {
        let uninit_slice = allocator.try_alloc_uninit_slice(capacity)?;
        Some(Queue {
            initialized_offset: 0,
            initialized_len: 0,
            uninit_slice,
        })
    }

    pub fn push_back(&mut self, value: T) -> Result<(), T> {
        if self.initialized_len >= self.uninit_slice.len() {
            return Err(value);
        }

        // Since `init_len < self.uninit_slice.len()`, this will only "wrap
        // once" and won't reach the indices at the start of the initialized
        // indices.
        let i = (self.initialized_offset + self.initialized_len) % self.uninit_slice.len();

        // The value at `i` is uninitialized due to the invariant stated in the
        // doc comment of `self.uninit_slice`, so overwriting it does not leak
        // (in the drop sense) any value.
        self.uninit_slice[i].write(value);

        // Value at `i` is now initialized, bump up the length to maintain the
        // `self.uninit_slice` invariant.
        self.initialized_len += 1;

        Ok(())
    }

    pub fn pop_front(&mut self) -> Option<T> {
        if self.initialized_len == 0 {
            return None;
        }

        // Safety: due to the invariant these functions maintain, explained in
        // the documentation of `self.uninit_slice`, we know that the value at
        // `self.init_offset` is initialized. Duplicates caused by
        // `MaybeUninit::assume_init_read` are avoided by incrementing
        // `self.init_offset` after this.
        let value = unsafe { self.uninit_slice[self.initialized_offset].assume_init_read() };

        // Now that we have an owned version of the value at `self.init_offset`,
        // splice out the first index of the init slice.
        self.initialized_offset = (self.initialized_offset + 1) % self.uninit_slice.len();
        self.initialized_len -= 1;

        Some(value)
    }

    /// The amount of elements that could be pushed before the array is full.
    pub fn spare_capacity(&self) -> usize {
        self.uninit_slice.len() - self.initialized_len
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        let len = self.uninit_slice.len();

        let head = &self.uninit_slice
            [self.initialized_offset..(self.initialized_offset + self.initialized_len).min(len)];
        // Safety: the above indices are included in the span of initialized
        // elements of `self.uninit_slice`, and transmuting a fully initialized
        // `&[MaybeUninit<T>]` to `&[T]` is safe.
        let head = unsafe { transmute::<&[MaybeUninit<T>], &[T]>(head) };

        let tail = &self.uninit_slice
            [..(self.initialized_offset + self.initialized_len).saturating_sub(len)];
        // Safety: the above indices are included in the span of initialized
        // elements of `self.uninit_slice`, and transmuting a fully initialized
        // `&[MaybeUninit<T>]` to `&[T]` is safe.
        let tail = unsafe { transmute::<&[MaybeUninit<T>], &[T]>(tail) };

        head.iter().chain(tail.iter())
    }
}

#[cfg(test)]
mod tests {
    use crate::{allocators::LinearAllocator, test_platform::TestPlatform};

    use super::Queue;

    #[test]
    fn pushes_and_pops_in_fifo_order() {
        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, 2).unwrap();
        let mut queue = Queue::<u8>::new(&alloc, 2).unwrap();

        assert!(queue.push_back(0).is_ok());
        assert!(queue.push_back(1).is_ok());
        assert!(
            queue.push_back(2).is_err(),
            "pushed a third element into a queue with capacity for two?",
        );
        assert_eq!(Some(0), queue.pop_front());
        assert!(queue.push_back(2).is_ok());
        assert_eq!(Some(1), queue.pop_front());
        assert_eq!(Some(2), queue.pop_front());
        assert_eq!(
            None,
            queue.pop_front(),
            "popped a fourth element after only pushing three elements?",
        );
    }

    #[test]
    fn iter_works() {
        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, 3).unwrap();
        let mut queue = Queue::<u8>::new(&alloc, 3).unwrap();
        queue.push_back(0).unwrap();
        queue.push_back(1).unwrap();
        queue.push_back(2).unwrap();
        queue.pop_front().unwrap();
        queue.push_back(3).unwrap();

        let mut iter = queue.iter();
        assert_eq!(Some(&1), iter.next());
        assert_eq!(Some(&2), iter.next());
        assert_eq!(Some(&3), iter.next());
        assert_eq!(None, iter.next());
    }
}
