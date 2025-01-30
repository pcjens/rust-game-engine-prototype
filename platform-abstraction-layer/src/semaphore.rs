pub trait SemaphoreImpl: Sync {
    fn increment(&self);
    fn decrement(&self);
}

struct SingleThreadedSemaphore;
impl SemaphoreImpl for SingleThreadedSemaphore {
    fn increment(&self) {}
    fn decrement(&self) {}
}

/// Atomically counting semaphore for efficiently waiting on other threads,
/// intended to be created by the platform implementation, making use of OS
/// synchronization primitives.
///
/// On a single-threaded platform, these operations can be no-ops, because a
/// decrement without a preceding increment would be a deadlock anyway. Users of
/// the semaphore should expect this and probably panic if this happens.
#[derive(Clone)]
pub struct Semaphore {
    // TODO: replace with a *const dyn SemaphoreTrait + Sync or similar?
    semaphore_ptr: *const dyn SemaphoreImpl,
    drop_fn: Option<fn(*const dyn SemaphoreImpl)>,
}

// Safety: Semaphore::single_threaded makes sure this struct is inert,
// Semaphore::new has safety requirements to make sure this isn't an issue.
unsafe impl Sync for Semaphore {}

impl Semaphore {
    /// Creates a semaphore from very raw parts. Intended to be used in a
    /// platform implementation.
    ///
    /// `drop_fn` is called in Semaphore's drop implementation and
    /// `semaphore_ptr` is passed in. The `semaphore_ptr` isn't used after that.
    ///
    /// ### Safety
    ///
    /// `semaphore_ptr` should be valid for the whole lifetime of the semaphore
    /// (until drop).
    pub unsafe fn new(
        semaphore_ptr: *const dyn SemaphoreImpl,
        drop_fn: Option<fn(*const dyn SemaphoreImpl)>,
    ) -> Semaphore {
        Semaphore {
            semaphore_ptr,
            drop_fn,
        }
    }

    /// Creates a no-op semaphore. Fits single-threaded platforms — will cause
    /// panics if used in multi-threaded ones.
    pub fn single_threaded() -> Semaphore {
        Semaphore {
            semaphore_ptr: &SingleThreadedSemaphore,
            drop_fn: None,
        }
    }

    /// Increments the semaphore's count.
    pub fn increment(&self) {
        // Safety: the constructor requires the pointer to be valid to use for
        // the whole lifetime of the semaphore.
        unsafe { &(*self.semaphore_ptr) }.increment();
    }

    /// Waits until the count is positive, and then decrements the semaphore's
    /// count.
    ///
    /// Allowed to wake up without a matching increment if the alternative is
    /// deadlocking. So this being matched by an increment can't be depended on
    /// for unsafe operations. However, it's fine to panic in such a case,
    /// because it's a clear bug.
    pub fn decrement(&self) {
        // Safety: the constructor requires the pointer to be valid to use for
        // the whole lifetime of the semaphore.
        unsafe { &(*self.semaphore_ptr) }.decrement();
    }
}

impl Drop for Semaphore {
    fn drop(&mut self) {
        if let Some(drop_fn) = self.drop_fn {
            drop_fn(self.semaphore_ptr);
        }
    }
}
