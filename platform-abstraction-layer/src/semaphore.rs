use core::ptr;

/// Atomically counting semaphore for efficiently waiting on other threads,
/// intended to be created by the platform implementation, making use of OS
/// synchronization primitives.
///
/// On a single-threaded platform, these operations can be no-ops, because a
/// decrement without a preceding increment would be a deadlock anyway. Users of
/// the semaphore should expect this and probably panic if this happens.
#[derive(Clone)]
pub struct Semaphore {
    semaphore_ptr: *const (),
    increment_fn: Option<fn(*const ())>,
    decrement_fn: Option<fn(*const ())>,
    drop_fn: Option<fn(*const ())>,
}

// Safety: Semaphore::single_threaded makes sure this struct is inert,
// Semaphore::new has safety requirements to make sure this isn't an issue.
unsafe impl Sync for Semaphore {}

impl Semaphore {
    /// Creates a semaphore from very raw parts. Intended to be used in a
    /// platform implementation.
    ///
    /// `semaphore_ptr` represents the semaphore value (possibly pointing to
    /// some data) and the functions will get that pointer passed in when
    /// called.
    ///
    /// ### Safety
    ///
    /// `semaphore_ptr` should be safe to access from other threads, and the
    /// functions should expect to be called from different threads.
    pub unsafe fn new(
        semaphore_ptr: *const (),
        increment_fn: Option<fn(*const ())>,
        decrement_fn: Option<fn(*const ())>,
        drop_fn: Option<fn(*const ())>,
    ) -> Semaphore {
        Semaphore {
            semaphore_ptr,
            increment_fn,
            decrement_fn,
            drop_fn,
        }
    }

    pub fn single_threaded() -> Semaphore {
        Semaphore {
            semaphore_ptr: ptr::null(),
            increment_fn: None,
            decrement_fn: None,
            drop_fn: None,
        }
    }

    /// Increments the semaphore's count.
    pub fn increment(&self) {
        if let Some(increment) = self.increment_fn {
            increment(self.semaphore_ptr);
        }
    }

    /// Waits until the count is positive, and then decrements the semaphore's
    /// count.
    ///
    /// Allowed to wake up without a matching increment if the alternative is
    /// deadlocking. So this being matched by an increment can't be depended on
    /// for unsafe operations. However, it's fine to panic in such a case,
    /// because it's a clear bug.
    pub fn decrement(&self) {
        if let Some(decrement) = self.decrement_fn {
            decrement(self.semaphore_ptr);
        }
    }
}

impl Drop for Semaphore {
    fn drop(&mut self) {
        if let Some(drop_fn) = self.drop_fn {
            drop_fn(self.semaphore_ptr);
        }
    }
}
