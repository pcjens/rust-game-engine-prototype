use core::{
    cell::RefCell,
    fmt::Debug,
    mem::replace,
    ops::{Deref, DerefMut},
};

use super::LinearAllocator;

/// A container for `T`. Think of `Box`, but allocated from a [Pool].
#[derive(Debug)]
pub struct PoolBox<'pool, T> {
    /// Contains a mutable borrow of the thing this references. Always Some
    /// while in use, gets take()n in the Drop impl.
    inner: Option<&'pool mut PoolElement<'pool, T>>,
    pool: &'pool Pool<'pool, T>,
}

impl<T> Deref for PoolBox<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self.inner.as_ref().unwrap() {
            PoolElement::Allocated(value) => value,
            PoolElement::Free { .. } => unreachable!(),
        }
    }
}

impl<T> DerefMut for PoolBox<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self.inner.as_mut().unwrap() {
            PoolElement::Allocated(value) => value,
            PoolElement::Free { .. } => unreachable!(),
        }
    }
}

impl<T> Drop for PoolBox<'_, T> {
    fn drop(&mut self) {
        // Create the new head of the free list (which will be stored where the
        // contents of this box used to be).
        let mut pool_next_free = self.pool.next_free.borrow_mut();
        let next_free = pool_next_free.take();
        let new_next_free = PoolElement::Free { next_free };

        // Get the allocated value (to drop) and put the free slot in its place.
        let allocation = self.inner.take().unwrap();
        let allocated_element = replace(allocation, new_next_free);

        // MaybeUninit leaking note: this is where values allocated by Pool are
        // dropped.
        drop(allocated_element);

        // Assign the (new) head of the free list back to the pool.
        let _ = pool_next_free.insert(allocation);
    }
}

#[derive(Debug)]
enum PoolElement<'allocation, T> {
    Free {
        next_free: Option<&'allocation mut PoolElement<'allocation, T>>,
    },
    Allocated(T),
}

/// An object pool for objects of type `T`.
///
/// Can allocate instances of `T` very fast, and the allocated memory gets
/// fragmented only in the sense that subsequent allocations may be far from
/// each other. No memory is wasted since this only allocates fixed size chunks,
/// which are always reused. Individual allocations are returned as [PoolBox]es,
/// which can be dropped to free up memory for new allocations. Uses a
/// [LinearAllocator] for backing memory, which cannot be reset for the lifetime
/// of the pool.
#[derive(Debug)]
pub struct Pool<'allocation, T> {
    allocator: &'allocation LinearAllocator<'allocation>,
    next_free: RefCell<Option<&'allocation mut PoolElement<'allocation, T>>>,
}

impl<'allocation, T> Pool<'allocation, T> {
    pub fn new(allocator: &'allocation LinearAllocator) -> Option<Pool<'allocation, T>> {
        Some(Pool {
            allocator,
            next_free: RefCell::new(None),
        })
    }

    pub fn insert(&'allocation self, value: T) -> Result<PoolBox<'allocation, T>, T> {
        'use_a_free_slot: {
            let mut next_free = self.next_free.borrow_mut();

            let Some(dst_slot) = next_free.take() else {
                break 'use_a_free_slot;
            };

            // Put the given value into the free slot.
            let old_free_list_head = replace(dst_slot, PoolElement::Allocated(value));

            // Pop the head off the free list (it's now owned by this function,
            // so it's not really a free slot in the backing memory anymore).
            match old_free_list_head {
                PoolElement::Allocated(_) => unreachable!(),
                PoolElement::Free {
                    next_free: new_free_list_head,
                } => {
                    *next_free = new_free_list_head;
                }
            }

            return Ok(PoolBox {
                inner: Some(dst_slot),
                pool: self,
            });
        }

        'allocate_new_slot: {
            let Some(new_slot) = self
                .allocator
                .try_alloc_uninit_slice::<PoolElement<'allocation, T>>(1)
                .and_then(|slice| slice.first_mut())
            else {
                break 'allocate_new_slot;
            };

            // MaybeUninit leaking note: this borrow is stored in PoolBox, which
            // extracts the value and drops it in its Drop implementation.
            let initialized = new_slot.write(PoolElement::Allocated(value));

            return Ok(PoolBox {
                inner: Some(initialized),
                pool: self,
            });
        }

        Err(value)
    }
}

// TODO: Unit test to ensure that OOM is handled properly, and that free slots
// are reused as expected.
