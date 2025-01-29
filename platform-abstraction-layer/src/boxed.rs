use core::{
    fmt::{Debug, Display},
    ops::{Deref, DerefMut},
};

/// Owned pointer to a `T`.
///
/// Intended for similar use cases as the standard library `Box`, but this one
/// does not free the memory on drop (though it does drop the `T`). Used
/// sparingly in cases where we really need to own a dynamically allocated `T`
/// instead of borrowing it, and using a static borrow would be too verbose.
pub struct Box<T: 'static + ?Sized> {
    inner: *mut T,
    should_drop: bool,
}

impl<T: ?Sized> Box<T> {
    /// Creates a [`Box`] from a leaked borrow of the boxed value.
    pub fn from_mut(value: &'static mut T) -> Box<T> {
        Box {
            inner: value,
            should_drop: true,
        }
    }

    /// Creates a [`Box`] from a raw pointer to the boxed value.
    ///
    /// ### Safety
    ///
    /// The caller must ensure that the memory behind the pointer is never read,
    /// written, or freed while this `Box` exists, and that the T pointed to by
    /// this pointer is never accessed after this call, unless it's after
    /// deconstructing this box with [`Box::into_ptr`].
    pub unsafe fn from_ptr(ptr: *mut T) -> Box<T> {
        Box {
            inner: ptr,
            should_drop: true,
        }
    }

    /// Consumes the [`Box<T>`] without dropping the internal value and returns
    /// the internal pointer.
    pub fn into_ptr(mut self) -> *mut T {
        self.should_drop = false; // avoid dropping the value
        self.inner
    }

    /// Forgets the type of the [`Box`].
    ///
    /// Useful in cases where we're only interested in ownership of the pointer
    /// rather than the value behind it.
    pub fn anonymize(self) -> Box<()> {
        Box {
            inner: self.inner as *mut (),
            should_drop: self.should_drop,
        }
    }
}

impl<T: ?Sized> Drop for Box<T> {
    fn drop(&mut self) {
        if self.should_drop {
            // Safety:
            // - self.inner is valid for reads and writes because the constructors
            //   require it.
            // - self.inner is properly aligned because the constructors require it.
            // - self.inner is nonnull because the constructors require it.
            // - self.inner should be valid for dropping, because it's exclusively
            //   owned by us, and it should be unsafe to leave T in an un-droppable
            //   state via the deref channels.
            // - the T pointed to by self.inner is exclusively accessible by the
            //   internals of this Box, and since we're in a drop impl, we have a
            //   mutable borrow of this Box, so this should indeed be the only way
            //   to access parts of the object.
            unsafe { self.inner.drop_in_place() };
        }
    }
}

impl<T: ?Sized> Deref for Box<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: the constructors ensure that the pointer is good to
        // turn into a reference.
        unsafe { &*self.inner }
    }
}

impl<T: ?Sized> DerefMut for Box<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: the constructors ensure that the pointer is good to
        // turn into a reference.
        unsafe { &mut *self.inner }
    }
}

impl<T: ?Sized + Debug> Debug for Box<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let inner: &T = self;
        f.debug_tuple("Box").field(&inner).finish()
    }
}

impl<T: ?Sized + Display> Display for Box<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let inner: &T = self;
        write!(f, "{}", inner)
    }
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicBool, Ordering};

    use crate::Box;

    #[test]
    fn dropping_a_box_drops_the_value() {
        static DROPPED: AtomicBool = AtomicBool::new(false);
        struct Example;
        impl Drop for Example {
            fn drop(&mut self) {
                DROPPED.store(true, Ordering::Release);
            }
        }

        let mut example = Example;
        let example_ptr = &raw mut example;
        {
            let example_box = unsafe { Box::from_ptr(example_ptr) };
            assert!(!DROPPED.load(Ordering::Acquire));
            drop(example_box);
            assert!(DROPPED.load(Ordering::Acquire));
        }
        core::mem::forget(example);
    }
}
