use core::{
    mem::{transmute, MaybeUninit},
    sync::atomic::AtomicUsize,
};

use platform_abstraction_layer::{
    channel::{channel_from_parts, CachePadded, Receiver, Sender, SyncUnsafeCell},
    Pal,
};

use crate::allocators::LinearAllocator;

/// Creates a single-producer single-consumer channel.
pub fn channel<T: Sync>(
    platform: &dyn Pal,
    allocator: &'static LinearAllocator,
    capacity: usize,
) -> Option<(Sender<T>, Receiver<T>)> {
    type ChannelSlot<T> = SyncUnsafeCell<CachePadded<Option<T>>>;

    // +1 to capacity since we're using the last slot as the difference between empty and full.
    let queue = allocator.try_alloc_uninit_slice::<ChannelSlot<T>>(capacity + 1)?;
    for slot in &mut *queue {
        slot.write(SyncUnsafeCell::new(CachePadded::new(None)));
    }
    // Safety: all the values are initialized above.
    let queue =
        unsafe { transmute::<&mut [MaybeUninit<ChannelSlot<T>>], &mut [ChannelSlot<T>]>(queue) };

    let offsets = allocator.try_alloc_uninit_slice::<AtomicUsize>(2)?;
    for offset in &mut *offsets {
        offset.write(AtomicUsize::new(0));
    }
    // Safety: all the values are initialized above.
    let offsets =
        unsafe { transmute::<&mut [MaybeUninit<AtomicUsize>], &mut [AtomicUsize]>(offsets) };
    let (read, write) = offsets.split_at_mut(1);

    let semaphore = allocator.try_alloc_uninit_slice(1)?;
    let semaphore = semaphore[0].write(platform.create_semaphore());

    Some(channel_from_parts(
        queue,
        &mut read[0],
        &mut write[0],
        semaphore,
    ))
}
