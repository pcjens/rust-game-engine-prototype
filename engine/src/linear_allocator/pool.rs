use core::{
    cell::RefCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use super::{FixedVec, LinearAllocator};

/// A container for `T`. Think of `Box`, but allocated from a [`Pool`]. Frees up
/// memory for a new [`Pool::insert`] on drop.
///
/// ## Lifetime notes
///
/// `'a` is the backing [`Pool`]'s lifetime parameter (the `'a` in `Pool<'a,
/// T>`).
///
/// `'b` is the lifetime of the [`Pool::insert`] borrow.
///
/// For example, if you create a pool using the frame allocator, `'a` would be
/// (bounded by) `'frm`, and `'b` would generally be some anonymous lifetime
/// that prevents the Pool from being dropped while this box exists.
#[derive(Debug)]
pub struct PoolBox<'a, 'b, T> {
    /// Contains a mutable borrow of the thing this references. Always Some
    /// while in use, gets take()n in the Drop impl so that it can be moved out
    /// of self.
    inner: Option<&'a mut Option<T>>,
    free_list: &'b RefCell<FixedVec<'a, &'a mut Option<T>>>,
}

impl<T> Deref for PoolBox<'_, '_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self.inner.as_ref().unwrap() {
            Some(value) => value,
            None => unreachable!(),
        }
    }
}

impl<T> DerefMut for PoolBox<'_, '_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self.inner.as_mut().unwrap() {
            Some(value) => value,
            None => unreachable!(),
        }
    }
}

impl<T> Drop for PoolBox<'_, '_, T> {
    fn drop(&mut self) {
        let mut free_list = self.free_list.borrow_mut();

        let allocation = self.inner.take().unwrap();

        let allocated_element = allocation.take().unwrap();
        drop(allocated_element);

        let Ok(_) = free_list.push(allocation) else {
            panic!("the pool free list should not be able to overflow");
        };
    }
}

/// An object pool for objects of type `T`.
///
/// Can allocate instances of `T` very fast. Individual allocations are returned
/// as [`PoolBox`]es, which can be dropped to free up memory for new allocations.
///
/// Never frees up the backing memory, all allocated "slots" are just added to
/// the free list to be reused, so the pool reserves enough memory for its peak
/// usage until it's dropped.
///
/// Uses a [`LinearAllocator`] for backing memory, which cannot be reset for the
/// lifetime of the pool.
#[derive(Debug)]
pub struct Pool<'a, T> {
    allocator: &'a LinearAllocator<'a>,
    free_list: RefCell<FixedVec<'a, &'a mut Option<T>>>,
}

impl<'a, T> Pool<'a, T> {
    /// Creates a new pool with `capacity` possible allocations at the same
    /// time, and allocates the free list for it.
    ///
    /// If `allocator` runs out of memory before `capacity` allocations have
    /// been made, then that's the bounding factor. In either case,
    /// [`Pool::insert`] will start returning None until existing [`PoolBox`]es
    /// are dropped.
    ///
    /// The free list is a simple list of pointers, so it takes up `capacity *
    /// size_of::<usize>()` of memory and any required padding for the
    /// list to have pointer alignment.
    pub fn new(allocator: &'a LinearAllocator, capacity: usize) -> Option<Pool<'a, T>> {
        Some(Pool {
            allocator,
            free_list: RefCell::new(FixedVec::new(allocator, capacity)?),
        })
    }

    /// Stores the value in a [`PoolBox`], reusing previous allocations that
    /// have since been freed, or if none are available, by allocating from the
    /// [`LinearAllocator`] passed into the constructor of the pool. If neither
    /// is possible, the value is returned back wrapped in a [`Result::Err`].
    ///
    /// If `T` doesn't implement [`Debug`] and you want to unwrap the result,
    /// use [`Result::ok`] and then unwrap.
    pub fn insert(&self, value: T) -> Result<PoolBox<'a, '_, T>, T> {
        'use_a_free_slot: {
            let mut free_list = self.free_list.borrow_mut();
            let Some(dst_slot) = free_list.pop() else {
                break 'use_a_free_slot;
            };

            let _ = dst_slot.insert(value);

            return Ok(PoolBox {
                inner: Some(dst_slot),
                free_list: &self.free_list,
            });
        }

        'allocate_new_slot: {
            let Some(new_slot) = self
                .allocator
                .try_alloc_uninit_slice::<Option<T>>(1)
                .and_then(|slice| slice.first_mut())
            else {
                break 'allocate_new_slot;
            };

            // MaybeUninit leaking note: this borrow is stored in PoolBox, which
            // extracts the value and drops it in its Drop implementation.
            let initialized = new_slot.write(Some(value));

            return Ok(PoolBox {
                inner: Some(initialized),
                free_list: &self.free_list,
            });
        }

        Err(value)
    }
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicI32, Ordering};

    use crate::{test_platform::TestPlatform, LinearAllocator, Pool};

    use super::PoolBox;

    /// Something high for allocator-bottlenecked tests.
    const HIGH_CAP: usize = 10;
    /// Memory required for the pool's free list with [`HIGH_CAP`] capacity.
    const HIGH_CAP_BASELINE: usize = HIGH_CAP * size_of::<usize>();

    #[test]
    fn does_not_allocate_more_than_peak_usage() {
        type Element = u8;
        const EXPECTED_ALLOC: usize = size_of::<Option<Element>>() * 3;

        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, HIGH_CAP_BASELINE + EXPECTED_ALLOC).unwrap();
        let pool: Pool<Element> = Pool::new(&alloc, HIGH_CAP).unwrap();

        let a = pool.insert(0).unwrap(); // no free slots space, allocate
        let _b = pool.insert(0).unwrap(); // no free slots space, allocate
        drop(a);
        let _c = pool.insert(0).unwrap(); // this should go in a's original memory
        let _d = pool.insert(0).unwrap(); // no free slots space again, allocate

        assert_eq!(
            EXPECTED_ALLOC,
            alloc.allocated() - HIGH_CAP_BASELINE,
            "pool should reuse previous allocations once in these four inserts"
        );
    }

    #[test]
    fn handles_allocator_oom_gracefully() {
        type Element = u8;
        const ALLOC_SIZE: usize = size_of::<Option<Element>>();

        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, HIGH_CAP_BASELINE + ALLOC_SIZE).unwrap();
        let pool: Pool<Element> = Pool::new(&alloc, HIGH_CAP).unwrap();

        let _a: PoolBox<Element> = pool.insert(0).unwrap();
        let _b: Element = pool.insert(0).unwrap_err(); // space for just one, should oom
    }

    #[test]
    fn does_not_leak_allocated_values() {
        static ELEMENT_COUNT: AtomicI32 = AtomicI32::new(0);

        struct Element {
            _foo: bool,
        }
        impl Element {
            pub fn create_and_count() -> Element {
                ELEMENT_COUNT.fetch_add(1, Ordering::Release);
                Element { _foo: true }
            }
        }
        impl Drop for Element {
            fn drop(&mut self) {
                ELEMENT_COUNT.fetch_add(-1, Ordering::Release);
            }
        }

        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, 1000).unwrap();
        let pool: Pool<Element> = Pool::new(&alloc, 1).unwrap();

        assert_eq!(0, ELEMENT_COUNT.load(Ordering::Acquire), "test's haunted");
        let allocated_thing = pool.insert(Element::create_and_count()).ok().unwrap();
        assert_eq!(1, ELEMENT_COUNT.load(Ordering::Acquire), "dropped early?");
        drop(allocated_thing);
        assert_eq!(0, ELEMENT_COUNT.load(Ordering::Acquire), "value leaked!");
    }
}
