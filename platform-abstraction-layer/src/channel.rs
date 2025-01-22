//! Static memory based single-producer single-consumer channel for
//! communication between threads.

use core::sync::atomic::{AtomicUsize, Ordering};

pub use sync_unsafe_cell::SyncUnsafeCell;

struct SharedChannelState<T: 'static + Sync> {
    /// The slice containing the actual elements.
    queue: &'static [SyncUnsafeCell<Option<T>>],
    /// The index to `queue` where the oldest pushed element is. Only mutated by
    /// [`Receiver::try_recv`]. If `read_offset == write_offset`, the queue
    /// should be considered empty.
    read_offset: &'static AtomicUsize,
    /// The index to `queue` where the next element is pushed. Only mutated by
    /// [`Sender::send`]. If `write_offset + 1 == read_offset`, the queue is
    /// considered full. Since writes need to happen before reads, this happens
    /// when the writes wrap around to the start and reach the read offset.
    write_offset: &'static AtomicUsize,
}

/// Creates a new channel from its raw parts.
///
/// The queue should be one longer than the actual bound of the queue, so a
/// channel with room for 3 buffered elements should have a slice of length 4 as
/// the `queue`.
pub fn channel_from_parts<T: Sync>(
    queue: &'static mut [SyncUnsafeCell<Option<T>>],
    write_offset: &'static mut AtomicUsize,
    read_offset: &'static mut AtomicUsize,
) -> (Sender<T>, Receiver<T>) {
    read_offset.store(0, Ordering::Release);
    write_offset.store(0, Ordering::Release);
    let sender = Sender {
        ch: SharedChannelState {
            queue,
            read_offset,
            write_offset,
        },
    };
    let receiver = Receiver {
        ch: SharedChannelState {
            queue,
            read_offset,
            write_offset,
        },
    };
    (sender, receiver)
}

pub struct Sender<T: 'static + Sync> {
    ch: SharedChannelState<T>,
}

impl<T: Sync> Sender<T> {
    /// Sends the value into the channel if there's room.
    pub fn send(&mut self, value: T) -> Result<(), T> {
        if self.ch.queue.len() <= 1 {
            // This channel does not have any capacity, always fail.
            return Err(value);
        }

        // 1. Acquire-load the read offset, so we know the offset is either what
        //    we get here or something higher (if the other side `pop`s during
        //    this function). In either case, we're good as long as we don't
        //    write past the offset we read here.
        let read_offset = self.ch.read_offset.load(Ordering::Acquire);

        // 2. Since this is a single-producer channel (and thus we have a mut
        //    self), we can acquire-load here and release-store later in this
        //    function and rest assured that the value of write_offset does not
        //    change in between.
        let write_offset = self.ch.write_offset.load(Ordering::Acquire);

        let next_write_offset = (write_offset + 1) % self.ch.queue.len();

        // See if the queue is full. Note that the comparison isn't ">=" because
        // that would cause issues with wrapping. This is still equivalent to
        // ">=" if we didn't wrap (which is what we want), because:
        // 1. The read offset is either the value stored in read_offset or
        //    greater. This means that the value of read_offset never goes
        //    *down* between push_back calls.
        // 2. The write offset is always only incremented by 1 per push_back.
        // => next_write_offset must reach read_offset before becoming greater
        //    than it. Since write_offset is not incremented in this branch,
        //    next_write_offset will never actually go past read_offset.
        if next_write_offset == read_offset {
            // The queue is full.
            return Err(value);
        }

        // 3. Write the value.
        {
            let slot_ptr = self.ch.queue[write_offset].get();
            // Safety: this specific index of the queue is not currently being
            // read by the Receiver nor written by the Sender:
            // - The Receiver does not read if read_offset == write_offset, and
            //   since only Sender mutates write_offset and we have a mutable
            //   borrow of self, we know Receiver is definitely not reading from
            //   this index at this time. (After step 4 below, it can read this
            //   value. Note that we drop this borrow before that step.)
            // - We have a mutable borrow of this Sender, and there can only be
            //   one Sender per write_offset, so we're definitely the only
            //   Sender trying to access this queue.
            let slot = unsafe { &mut *slot_ptr };
            assert!(slot.is_none(), "slot should not be populated since the write offset should never go past the read offset");
            *slot = Some(value);
        }

        // 4. Update the write offset, making the written value visible to the
        //    receiver.
        self.ch
            .write_offset
            .store(next_write_offset, Ordering::Release);

        Ok(())
    }
}

pub struct Receiver<T: 'static + Sync> {
    ch: SharedChannelState<T>,
}

impl<T: Sync> Receiver<T> {
    /// Returns the oldest sent value on this channel if there are any.
    pub fn try_recv(&mut self) -> Option<T> {
        if self.ch.queue.len() <= 1 {
            // This channel does not have any capacity, nothing to receive.
            return None;
        }

        // 1. Acquire-load the write offset, so we know the offset is either
        //    what we get here or something higher (if the other side `push`es
        //    during this function). In either case, we're good as long as we
        //    only read elements before this offset. Also, if we're really
        //    racing against the writes, this should ensure that their write to
        //    the slot before this one (the "freshest" slot we might read) is
        //    visible to us as well, since they store this value with release
        //    ordering.
        let write_offset = self.ch.write_offset.load(Ordering::Acquire);

        // 2. Since this is a single-consumer channel (and thus we have a mut
        //    self), we can acquire-load here and release-store later in this
        //    function and rest assured that the value of read_offset does not
        //    change in between.
        let read_offset = self.ch.read_offset.load(Ordering::Acquire);

        // When the offsets match, the queue is considered empty. Otherwise the
        // read_offset is known to be "before" write_offset (i.e. if it wasn't
        // for the wrapping, it would be *less*), as read_offset only gets
        // incremented if it's not equal to write_offset, and write_offset only
        // gets incremented (if it weren't for the wrapping. But still, it's
        // always "after" or equal to read_offset).
        if read_offset == write_offset {
            return None;
        }

        // 3. Read the value.
        let value = {
            let slot_ptr = self.ch.queue[read_offset].get();
            // Safety: this specific index of the queue is not currently being
            // written by the Sender nor read by Receiver:
            // - We know the current write offset is the value of `write_offset`
            //   or greater (which may have wrapped), and that `read_offset` is
            //   before `write_offset`. Since the Sender checks that their write
            //   offset does not write at the read offset, and we know the read
            //   offset is the value of `read_offset` due to having a mutable
            //   borrow of this Receiver, we know Sender isn't writing into
            //   `read_offset`. This function allows writing into this index in
            //   step 4, after we're done with this borrow.
            // - We have a mutable borrow of the Receiver, and only one Receiver
            //   exists for a given channel, so there's definitely no other
            //   Receiver reading any index, including this one.
            let slot = unsafe { &mut *slot_ptr };
            slot.take()
                .expect("slot should be populated due to the write offset having passed this index")
        };

        // 4. Update the read offset, making room for the sender to push one
        //    more value into the queue. This also signals that the MaybeUninit
        //    value we read before should be interpreted as uninitialized.
        let next_read_offset = (read_offset + 1) % self.ch.queue.len();
        self.ch
            .read_offset
            .store(next_read_offset, Ordering::Release);

        Some(value)
    }
}

/// FIXME: Use core::cell::SyncUnsafeCell instead when it's stabilized. Tracked
/// in the rust-lang issue
/// [#95439](https://github.com/rust-lang/rust/issues/95439).
mod sync_unsafe_cell {
    #![allow(dead_code)]
    #[repr(transparent)]
    pub struct SyncUnsafeCell<T: ?Sized>(core::cell::UnsafeCell<T>);
    unsafe impl<T: ?Sized + Sync> Sync for SyncUnsafeCell<T> {}
    impl<T> SyncUnsafeCell<T> {
        #[inline]
        pub const fn new(value: T) -> Self {
            SyncUnsafeCell(core::cell::UnsafeCell::new(value))
        }
        #[inline]
        pub const fn get(&self) -> *mut T {
            self.0.get()
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::boxed::Box;
    use core::sync::atomic::AtomicUsize;

    use crate::channel::{channel_from_parts, sync_unsafe_cell::SyncUnsafeCell};

    /// A dummy function that matches the signature of `std::thread::spawn`.
    fn spawn<F, T>(f: F)
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        f();
    }

    #[test]
    fn sender_and_receiver_are_send() {
        let buf = Box::leak(Box::new([const { SyncUnsafeCell::new(None) }; 2]));
        let read_offset = Box::leak(Box::new(AtomicUsize::new(0)));
        let write_offset = Box::leak(Box::new(AtomicUsize::new(0)));

        let (mut tx, mut rx) = channel_from_parts::<u32>(buf, read_offset, write_offset);
        spawn(move || tx.send(123).unwrap());
        assert_eq!(123, rx.try_recv().unwrap());
    }

    #[test]
    fn handles_full_queue() {
        const CAP: usize = 6;
        let buf = Box::leak(Box::new([const { SyncUnsafeCell::new(None) }; CAP + 1]));
        let read_offset = Box::leak(Box::new(AtomicUsize::new(0)));
        let write_offset = Box::leak(Box::new(AtomicUsize::new(0)));

        let (mut tx, mut rx) = channel_from_parts::<usize>(buf, read_offset, write_offset);
        for i in 0..CAP {
            tx.send(123 + i).unwrap();
        }
        assert_eq!(Err(123), tx.send(123));
        for i in 0..CAP {
            assert_eq!(123 + i, rx.try_recv().unwrap());
        }
        assert_eq!(None, rx.try_recv());
    }

    #[test]
    fn wraps_around() {
        let buf = Box::leak(Box::new([const { SyncUnsafeCell::new(None) }; 3]));
        let read_offset = Box::leak(Box::new(AtomicUsize::new(0)));
        let write_offset = Box::leak(Box::new(AtomicUsize::new(0)));

        let (mut tx, mut rx) = channel_from_parts::<u32>(buf, read_offset, write_offset);
        tx.send(12).unwrap();
        assert_eq!(12, rx.try_recv().unwrap());
        tx.send(34).unwrap();
        assert_eq!(34, rx.try_recv().unwrap());
        tx.send(56).unwrap();
        assert_eq!(56, rx.try_recv().unwrap());
        tx.send(78).unwrap();
        assert_eq!(78, rx.try_recv().unwrap());

        tx.send(21).unwrap();
        tx.send(43).unwrap();
        assert_eq!(21, rx.try_recv().unwrap());
        assert_eq!(43, rx.try_recv().unwrap());
    }
}
