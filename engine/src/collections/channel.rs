use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicUsize, Ordering},
};

struct SharedChannelState<T: 'static> {
    /// The slice containing the actual elements.
    queue: &'static [UnsafeCell<MaybeUninit<T>>],
    /// The index to `queue` where the oldest pushed element is. Only mutated by
    /// the [`Receiver::pop_front`]. If `read_offset == write_offset`, the queue
    /// should be considered empty.
    read_offset: &'static AtomicUsize,
    /// The index to `queue` where the next element is pushed. Only mutated by
    /// [`Sender::push_back`].
    write_offset: &'static AtomicUsize,
}

/// Creates a single-producer single-consumer channel.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    static TEST: AtomicUsize = AtomicUsize::new(0);
    let queue = &[];
    let read_offset = &TEST;
    let write_offset = &TEST;
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

pub struct Sender<T: 'static> {
    ch: SharedChannelState<T>,
}

impl<T> Sender<T> {
    /// Sends the value into the channel if there's room.
    pub fn send(&mut self, value: T) -> Result<(), T> {
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
            // Leak safety: since we've already checked that write_offset hasn't
            // "lapped" the read_offset, we know this is not an initialized
            // value.
            slot.write(value);
        }

        // 4. Update the write offset, making the written value visible to the
        //    receiver.
        self.ch
            .write_offset
            .store(next_write_offset, Ordering::Release);

        Ok(())
    }
}

pub struct Receiver<T: 'static> {
    ch: SharedChannelState<T>,
}

impl<T> Receiver<T> {
    /// Returns the oldest sent value on this channel if there are any.
    pub fn try_recv(&mut self) -> Option<T> {
        // 1. Acquire-load the write offset, so we know the offset is either
        //    what we get here or something higher (if the other side `push`es
        //    during this function). In either case, we're good as long as we
        //    only read elements before this offset.
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
            // Safety:
            // - Why slot is definitely initialized: since read_offset !=
            //   write_offset, this index has been written to by
            //   Sender::push_back.
            // - Why this is not a double-read: this same MaybeUninit won't be
            //   read again since we have exclusive access to this value of
            //   read_offset (and thus this slot), and we increment it by one in
            //   step 4 right after this.
            unsafe { slot.assume_init_read() }
        };

        // 4. Update the read offset, making room for the sender to push one
        //    more value into the queue. This also signals that the MaybeUninit
        //    value we read before should be interpreted as uninitialized.
        let next_read_offset = read_offset + 1;
        self.ch
            .read_offset
            .store(next_read_offset, Ordering::Release);

        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::channel;

    fn spawn<F, T>(f: F)
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        f();
    }

    #[test]
    fn sender_and_receiver_are_send() {
        let (tx, mut rx) = channel::<u32>();
        spawn(move || tx.send(123));
        assert_eq!(123, rx.try_recv().unwrap());
    }
}
