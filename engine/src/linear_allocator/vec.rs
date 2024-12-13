use core::{
    mem::{needs_drop, transmute, MaybeUninit},
    ops::{Deref, DerefMut},
};

use super::LinearAllocator;

/// A fixed-capacity contiguous growable array type.
///
/// Named like Vec since it's used similarly, but this type does *not* allocate
/// more memory as needed. Very cheap to create and push to. Unlike
/// [arrayvec::ArrayVec], the capacity can be picked at runtime, and the backing
/// memory does not need to be initialized until it's actually used.
pub struct FixedVec<'a, T> {
    uninit_slice: &'a mut [MaybeUninit<T>],
    initialized_len: usize,
}

impl<T> FixedVec<'_, T> {
    /// Creates a new [FixedVec] with zero capacity, but also no need for an
    /// allocator.
    pub fn empty() -> FixedVec<'static, T> {
        FixedVec {
            uninit_slice: &mut [],
            initialized_len: 0,
        }
    }

    /// Creates a new [FixedVec] with enough space for `capacity` elements of
    /// type `T`. Returns None if the allocator does not have enough free space.
    pub fn new<'a>(allocator: &'a LinearAllocator, capacity: usize) -> Option<FixedVec<'a, T>> {
        let uninit_slice: &'a mut [MaybeUninit<T>] =
            allocator.try_alloc_uninit_slice::<T>(capacity)?;
        Some(FixedVec {
            uninit_slice,
            initialized_len: 0,
        })
    }

    /// Appends the value to the back of the array. Returns the given value back
    /// in an Err if there's no capacity left.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        // Pick index, check it fits:
        let i = self.initialized_len;
        let Some(uninit) = self.uninit_slice.get_mut(i) else {
            return Err(value);
        };

        // Write the value at the index:
        uninit.write(value);
        // Notes:
        // - The "existing value" (`uninit`, uninitialized memory) does not get
        //   dropped here. Dropping uninitialized memory would be bad.
        //   - To avoid leaks, we should be sure that `uninit` here is actually
        //     uninitialized: if `uninit` is actually initialized here, it will
        //     never be dropped, i.e. it will be leaked. `uninit` is definitely
        //     uninitialized the first time we use any specific index, since we
        //     specifically allocate a slice of uninitialized MaybeUninits. On
        //     the subsequent times, we rely on the fact that all the functions
        //     that remove values from the array also drop the values.

        // The value at `i` is now initialized, update length:
        self.initialized_len = i + 1;

        Ok(())
    }

    /// Empties out the array, dropping the currently contained values.
    pub fn clear(&mut self) {
        self.truncate(0);
    }

    /// Shortens the array to be the given length if it's currently longer. Any
    /// values past the new length are dropped.
    pub fn truncate(&mut self, new_len: usize) {
        if new_len >= self.initialized_len {
            return;
        }

        if needs_drop::<T>() {
            for initialized_value in &mut self.uninit_slice[new_len..self.initialized_len] {
                // Safety: since we're iterating only up to
                // `self.initialized_len`, which only gets incremented as the
                // values are initialized, all of these [MaybeUninit]s must be
                // initialized.
                unsafe { initialized_value.assume_init_drop() };
            }
        }

        self.initialized_len = new_len;
    }
}

impl<T> Drop for FixedVec<'_, T> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T> Deref for FixedVec<'_, T> {
    type Target = [T];

    fn deref<'a>(&'a self) -> &'a Self::Target {
        let initialized_slice = &self.uninit_slice[..self.initialized_len];
        // Safety: `MaybeUninit<T>` is identical to `T` except that it might be
        // uninitialized, and all values up to `self.initialized_len` are
        // initialized.
        unsafe { transmute::<&'a [MaybeUninit<T>], &'a [T]>(initialized_slice) }
    }
}

impl<T> DerefMut for FixedVec<'_, T> {
    fn deref_mut<'a>(&'a mut self) -> &'a mut Self::Target {
        let initialized_slice = &mut self.uninit_slice[..self.initialized_len];
        // Safety: `MaybeUninit<T>` is identical to `T` except that it might be
        // uninitialized, and all values up to `self.initialized_len` are
        // initialized.
        unsafe { transmute::<&'a mut [MaybeUninit<T>], &'a mut [T]>(initialized_slice) }
    }
}

#[cfg(test)]
mod tests {
    use core::{
        str::FromStr,
        sync::atomic::{AtomicI32, Ordering},
    };

    use arrayvec::ArrayString;

    use crate::{test_platform::TestPlatform, FixedVec, LinearAllocator};

    #[test]
    fn does_not_leak() {
        const COUNT: usize = 100;
        static ELEMENT_COUNT: AtomicI32 = AtomicI32::new(0);

        #[derive(Debug)]
        struct Element {
            _foo: bool,
            _bar: ArrayString<100>,
        }
        impl Element {
            pub fn create_and_count() -> Element {
                ELEMENT_COUNT.fetch_add(1, Ordering::Relaxed);
                Element {
                    _foo: true,
                    _bar: ArrayString::from_str("Bar").unwrap(),
                }
            }
        }
        impl Drop for Element {
            fn drop(&mut self) {
                ELEMENT_COUNT.fetch_add(-1, Ordering::Relaxed);
            }
        }

        let platform = TestPlatform::new();
        let alloc = LinearAllocator::new(&platform, size_of::<Element>() * COUNT).unwrap();
        let mut vec: FixedVec<Element> = FixedVec::new(&alloc, COUNT).unwrap();

        // Fill once:
        assert_eq!(0, ELEMENT_COUNT.load(Ordering::Relaxed));
        for _ in 0..COUNT / 2 {
            vec.push(Element::create_and_count()).unwrap();
        }
        assert_eq!(COUNT as i32 / 2, ELEMENT_COUNT.load(Ordering::Relaxed));

        // Clear:
        vec.clear();
        assert_eq!(0, ELEMENT_COUNT.load(Ordering::Relaxed));

        // Refill:
        for _ in 0..COUNT {
            vec.push(Element::create_and_count()).unwrap();
        }
        assert_eq!(COUNT as i32, ELEMENT_COUNT.load(Ordering::Relaxed));
        assert!(
            vec.push(Element::create_and_count()).is_err(),
            "vec should be full already"
        );
        assert_eq!(COUNT as i32, ELEMENT_COUNT.load(Ordering::Relaxed));

        // Drop:
        drop(vec);
        assert_eq!(0, ELEMENT_COUNT.load(Ordering::Relaxed));
    }
}
