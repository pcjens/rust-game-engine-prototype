use platform_abstraction_layer::{
    thread_pool::{TaskHandle, ThreadPool},
    Pal, TaskChannel, ThreadState,
};

use crate::{
    allocators::LinearAllocator,
    collections::{channel::channel, Queue, RingAllocationMetadata, RingBox, RingBuffer},
};

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
    platform: &dyn Pal,
    task_queue_length: usize,
) -> Option<ThreadPool> {
    let thread_count = platform.available_parallelism();
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
/// The return value is the size of the chunks the slice was split into. The
/// same slices can be acquired by calling `chunks` or `chunks_mut` on `data`
/// and passing it in as the chunk size.
///
/// Returns `None` if the thread pool already has pending tasks, or if the arena
/// doesn't have enough memory for the tasks. In these cases, the given function
/// is not called and the data is not touched.
#[track_caller]
#[must_use]
pub fn parallelize<T: 'static + Sync>(
    thread_pool: &mut ThreadPool,
    arena: &LinearAllocator,
    data: &mut [T],
    func: fn(&mut [T]),
) -> Option<usize> {
    struct Task<T: Sync> {
        data: *mut [T],
        func: fn(&mut [T]),
    }

    struct TaskProxy<T: 'static + Sync> {
        handle: TaskHandle<Task<T>>,
        metadata: RingAllocationMetadata,
    }

    if thread_pool.has_pending() {
        return None;
    }

    let max_tasks = thread_pool.thread_count();
    // Safety: task_buffer is only used to allocate within this function, and
    // while the allocations are passed to the thread pool, they are also
    // retrieved and RingBuffer::freed before returning.
    let mut task_buffer = unsafe { RingBuffer::new_non_static(arena, max_tasks) }?;
    let mut task_proxies = Queue::new(arena, max_tasks)?;

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
                (task.func)(data);
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

    Some(chunk_size)
}

#[cfg(test)]
mod tests {
    use crate::{
        allocators::{static_allocator, StaticAllocator},
        multithreading::{create_thread_pool, parallelize},
        test_platform::TestPlatform,
    };

    #[test]
    fn parallelize_works_singlethreaded() {
        static ARENA: &StaticAllocator = static_allocator!(10_000);
        let platform = TestPlatform::new(false);
        let mut thread_pool = create_thread_pool(ARENA, &platform, 1).unwrap();

        let mut data = [1, 2, 3, 4];
        parallelize(&mut thread_pool, ARENA, &mut data, |data| {
            for n in data {
                *n *= *n;
            }
        })
        .unwrap();
        assert_eq!([1, 4, 9, 16], data);
    }

    #[test]
    #[cfg(not(target_os = "emscripten"))]
    fn parallelize_works_multithreaded() {
        static ARENA: &StaticAllocator = static_allocator!(10_000);
        let platform = TestPlatform::new(true);
        let mut thread_pool = create_thread_pool(ARENA, &platform, 1).unwrap();

        let mut data = [1, 2, 3, 4];
        parallelize(&mut thread_pool, ARENA, &mut data, |data| {
            for n in data {
                *n *= *n;
            }
        })
        .unwrap();
        assert_eq!([1, 4, 9, 16], data);
    }

    #[test]
    #[ignore = "the emscripten target doesn't support multithreading"]
    #[cfg(target_os = "emscripten")]
    fn parallelize_works_multithreaded() {}
}
