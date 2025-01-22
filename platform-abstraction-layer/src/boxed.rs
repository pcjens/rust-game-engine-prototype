use core::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

/// Owned pointer to a `T`.
///
/// Intended for similar use cases as the standard library `Box`, but this one's
/// simpler and does not add a dependency on `alloc`. Using `alloc::boxed::Box`
/// would've been possible otherwise, but the allocator API is still unstable.
pub struct Box<T: 'static + ?Sized> {
    inner: &'static mut T,
}

impl<T: ?Sized> Box<T> {
    /// Creates a [`Box`] from a leaked borrow of the boxed value.
    pub fn from_mut(value: &'static mut T) -> Box<T> {
        Box { inner: value }
    }
}

impl<T: ?Sized> Deref for Box<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<T: ?Sized> DerefMut for Box<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.inner
    }
}

impl<T: ?Sized + Debug> Debug for Box<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Box").field(&self.inner).finish()
    }
}
