mod scatter_gather;

use platform_abstraction_layer::{thread_pool::ThreadPool, Pal, TaskChannel, ThreadState};

use crate::{allocators::LinearAllocator, collections::channel::channel};

pub use scatter_gather::*;

#[allow(unused_imports)] // used in docs
use platform_abstraction_layer::channel::CachePadded;

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
