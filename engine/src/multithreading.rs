// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::mem::MaybeUninit;

use arrayvec::ArrayVec;
use platform::{
    thread_pool::{TaskHandle, ThreadPool},
    Platform, TaskChannel, ThreadState,
};

use crate::{
    allocators::LinearAllocator,
    collections::{channel, Queue, RingAllocationMetadata, RingBox, RingBuffer},
};

/// The maximum amount of threads which can be used by [`parallelize`].
/// [`create_thread_pool`] also caps the amount of threads it creates at this.
pub const MAX_THREADS: usize = 128;

/// Creates a thread pool, reserving space for buffering `task_queue_length`
/// tasks per thread.
///
/// The task queue lengths are relevant in that they limit how many
/// [`ThreadPool::spawn_task`] calls can be made before
/// [`ThreadPool::join_task`] needs to be called to free up space in the queue.
/// [`parallelize`] only requires 1, as it only allocates one task per
/// thread, and requires the thread pool to be passed in empty.
pub fn create_thread_pool(
    allocator: &'static LinearAllocator,
    platform: &dyn Platform,
    task_queue_length: usize,
) -> Option<ThreadPool> {
    let thread_count = platform.available_parallelism().min(MAX_THREADS);
    if thread_count > 1 {
        let init_thread_state = || {
            let task_channel: TaskChannel = channel(platform, allocator, task_queue_length)?;
            let result_channel: TaskChannel = channel(platform, allocator, task_queue_length)?;
            Some(platform.spawn_pool_thread([task_channel, result_channel]))
        };
        let threads = allocator.try_alloc_boxed_slice_with(init_thread_state, thread_count)?;
        Some(ThreadPool::new(threads).unwrap())
    } else {
        let init_thread_state = || {
            let (tx, rx): TaskChannel = channel(platform, allocator, task_queue_length)?;
            Some(ThreadState::new(tx, rx))
        };
        let threads = allocator.try_alloc_boxed_slice_with(init_thread_state, 1)?;
        Some(ThreadPool::new(threads).unwrap())
    }
}

/// Runs the function on multiple threads, splitting the data into one part for
/// each thread.
///
/// The function also gets the offset of the specific subslice it got, relative
/// to the start of `data`.
///
/// The return value is the size of the chunks `data` was split into. The same
/// slices can be acquired by calling `chunks` or `chunks_mut` on `data` and
/// passing it in as the chunk size. If the input slice is empty, 0 is returned.
///
/// ### Panics
///
/// If the thread pool already has pending tasks. This shouldn't ever be the
/// case when using the threadpool with just this function, as this function
/// always consumes all tasks it spawns.
#[track_caller]
pub fn parallelize<T, F>(thread_pool: &mut ThreadPool, data: &mut [T], func: F) -> usize
where
    T: 'static + Sync,
    // TODO: relax this lifetime requirement to allow borrowing from the calling function
    F: 'static + Fn(&mut [T], usize),
{
    struct Task<T: 'static + Sync, F: 'static + Fn(&mut [T], usize)> {
        data: *mut [T],
        func: *const F,
        data_offset: usize,
    }

    struct TaskProxy<T: 'static + Sync, F: 'static + Fn(&mut [T], usize)> {
        handle: TaskHandle<Task<T, F>>,
        metadata: RingAllocationMetadata,
    }

    if thread_pool.has_pending() {
        panic!("thread pool has pending tasks but was used in a parallellize() call");
    }

    if data.is_empty() {
        return 0;
    }

    let max_tasks = thread_pool.thread_count().min(MAX_THREADS);

    let mut backing_task_buffer = ArrayVec::<MaybeUninit<Task<T, F>>, MAX_THREADS>::new();
    let mut backing_task_proxies = ArrayVec::<MaybeUninit<TaskProxy<T, F>>, MAX_THREADS>::new();
    for _ in 0..max_tasks {
        backing_task_buffer.push(MaybeUninit::uninit());
        backing_task_proxies.push(MaybeUninit::uninit());
    }

    // Safety: all allocations from this buffer are passed into the thread pool,
    // from which all tasks are joined, and those buffers are freed right after.
    // So there are no leaked allocations.
    let mut task_buffer = unsafe { RingBuffer::from_mut(&mut backing_task_buffer) }.unwrap();
    let mut task_proxies = Queue::from_mut(&mut backing_task_proxies).unwrap();

    thread_pool.reset_thread_counter();

    // Shadow `func` to ensure that the value doesn't get dropped until the end
    // of this function, since this borrow is shared with the threads.
    let func = &func;

    let chunk_size = data.len().div_ceil(max_tasks);
    for (i, data_part) in data.chunks_mut(chunk_size).enumerate() {
        // Shouldn't ever trip, but if it does, we'd much rather crash here than
        // having half-spawned a task, which could be unsound.
        assert!(i < max_tasks);

        // Allocate the thread pool task.
        let (task, metadata) = task_buffer
            .allocate_box(Task {
                data: &raw mut *data_part,
                func,
                data_offset: i * chunk_size,
            })
            .ok()
            .unwrap() // does not panic: task_buffer is guaranteed to have capacity via the assert at the start of this loop body
            .into_parts();

        // Send off the task, using the proxy function from it to call the
        // user-provided one.
        let handle = thread_pool
            .spawn_task(task, |task| {
                // Safety:
                // - Pointer-validity-wise, this reference is ok to create as it
                //   was created from a valid mutable borrow via &raw mut in the
                //   first place.
                // - Lifetime-wise, creating this borrow is valid because its
                //   lifetime spans this function, and this function is run
                //   within the lifetime of the `parallelize` function call due
                //   to all tasks being joined before the end.
                // - Exclusive-access-wise, it's valid since the backing slice
                //   is only used to split it with chunks_mut, and those chunks
                //   are simply sent off to worker threads. Since this all
                //   happens during parallelize() (see lifetime point), there's
                //   definitely no others creating any kind of borrow of this
                //   particular chunk.
                let data = unsafe { &mut *task.data };
                // Safety: same logic as for the data, except that this
                // reference is shared, which is valid because it's a
                // const-pointer and we borrow it immutably.
                unsafe { (*task.func)(data, task.data_offset) };
            })
            .ok()
            .unwrap(); // does not panic: thread_pool is guaranteed to have capacity, it's empty and we're only spawning thread_count tasks

        // Add the task handle to the queue to be joined before returning.
        task_proxies
            .push_back(TaskProxy { handle, metadata })
            .ok()
            .unwrap(); // does not panic: task_proxies is guaranteed to have capacity via the assert at the start of this loop body
    }

    // Join tasks and free the buffers (doesn't free up space for anything, but
    // makes sure we're not leaking anything, which would violate the safety
    // requirements of the non-static RingBuffer).
    while let Some(proxy) = task_proxies.pop_front() {
        let task = thread_pool.join_task(proxy.handle).ok().unwrap(); // does not panic: we're joining tasks in FIFO order

        // Safety: the `Task` was allocated in the previous loop, with the
        // actual boxed task being sent onto a thread, and the metadata stored
        // in the proxy, alongside the handle for said task. Since `task` here
        // is the result of that task, it must be the same boxed task allocated
        // alongside this metadata.
        let boxed = unsafe { RingBox::from_parts(task, proxy.metadata) };
        task_buffer.free_box(boxed).ok().unwrap();
    }

    chunk_size
}

#[cfg(test)]
mod tests {
    use crate::{
        allocators::{static_allocator, LinearAllocator},
        multithreading::{create_thread_pool, parallelize},
        test_platform::TestPlatform,
    };

    #[test]
    fn parallelize_works_singlethreaded() {
        static ARENA: &LinearAllocator = static_allocator!(10_000);
        let platform = TestPlatform::new(false);
        let mut thread_pool = create_thread_pool(ARENA, &platform, 1).unwrap();

        let mut data = [1, 2, 3, 4];
        parallelize(&mut thread_pool, &mut data, |data, _| {
            for n in data {
                *n *= *n;
            }
        });
        assert_eq!([1, 4, 9, 16], data);
    }

    #[test]
    #[cfg(not(target_os = "emscripten"))]
    fn parallelize_works_multithreaded() {
        static ARENA: &LinearAllocator = static_allocator!(10_000);
        let platform = TestPlatform::new(true);
        let mut thread_pool = create_thread_pool(ARENA, &platform, 1).unwrap();

        let mut data = [1, 2, 3, 4];
        parallelize(&mut thread_pool, &mut data, |data, _| {
            for n in data {
                *n *= *n;
            }
        });
        assert_eq!([1, 4, 9, 16], data);
    }

    #[test]
    #[ignore = "the emscripten target doesn't support multithreading"]
    #[cfg(target_os = "emscripten")]
    fn parallelize_works_multithreaded() {}
}
