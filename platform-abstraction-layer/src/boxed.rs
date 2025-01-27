use core::{
    fmt::{Debug, Display},
    ops::{Deref, DerefMut},
};

/// Owned pointer to a `T`.
///
/// Intended for similar use cases as the standard library `Box`, but this one's
/// simpler and does not add a dependency on `alloc`. Using `alloc::boxed::Box`
/// would've been possible otherwise, but the allocator API is still unstable.
pub struct Box<T: 'static + ?Sized> {
    inner: *mut T,
}

impl<T: ?Sized> Box<T> {
    /// Creates a [`Box`] from a leaked borrow of the boxed value.
    pub fn from_mut(value: &'static mut T) -> Box<T> {
        Box { inner: value }
    }

    /// Creates a [`Box`] from a raw pointer to the boxed value.
    ///
    /// ### Safety
    ///
    /// The caller must ensure that the pointer meets all requirements to be
    /// turned into a static mutable borrow, that memory behind the pointer is
    /// never freed while this `Box` exists, and that the T pointed to by this
    /// pointer is never accessed after this call, because [`Box`]'s Drop
    /// implementation drops `T`.
    pub unsafe fn from_ptr(ptr: *mut T) -> Box<T> {
        Box { inner: ptr }
    }

    /// Deconstructs into the internal pointer.
    pub fn into_ptr(self) -> *mut T {
        self.inner
    }
}

impl<T: ?Sized> Drop for Box<T> {
    fn drop(&mut self) {
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
