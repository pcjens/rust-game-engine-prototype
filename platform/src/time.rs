use core::{
    fmt::Debug,
    ops::{Add, Sub},
    time::Duration,
};

#[allow(unused_imports)] // used in docs
use crate::Platform;

/// Analogous to the standard library `Instant` type, representing a point in
/// time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(Duration);

impl Debug for Instant {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if *self >= Instant::reference() {
            f.debug_tuple("Instant")
                .field(&(self.duration_since(Instant::reference())))
                .field(&"after reference point")
                .finish()
        } else {
            f.debug_tuple("Instant")
                .field(&(Instant::reference().duration_since(*self)))
                .field(&"before reference point")
                .finish()
        }
    }
}

impl Instant {
    /// An arbitrary point in time to use as a starting point for other
    /// instances of [`Instant`].
    ///
    /// Generally intended to be used once in the platform implementation. Use
    /// [`Platform::now`] to get the current point in time.
    pub const fn reference() -> Instant {
        Instant(Duration::from_secs(u64::MAX / 2))
    }

    /// Returns the duration from `past_instant` to `self`.
    ///
    /// Returns None if `past_instant` is after `self`.
    pub fn duration_since(self, past_instant: Instant) -> Option<Duration> {
        if self.0 >= past_instant.0 {
            Some(self.0 - past_instant.0)
        } else {
            None
        }
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, rhs: Duration) -> Self::Output {
        Instant(self.0 - rhs)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, rhs: Duration) -> Self::Output {
        Instant(self.0 + rhs)
    }
}
