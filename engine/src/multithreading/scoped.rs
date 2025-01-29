#[cfg(feature = "std")]
extern crate std;

use core::{
    cell::RefCell,
    marker::PhantomData,
    mem::transmute,
    slice,
    sync::atomic::{AtomicU64, Ordering},
};

use platform_abstraction_layer::thread_pool::{TaskHandle, ThreadPool};

use crate::{
    allocators::LinearAllocator,
    collections::{Queue, RingAllocationMetadata, RingBox, RingBuffer},
};

fn make_scope_id() -> u64 {
    static SCOPE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
    let prev_id = SCOPE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    prev_id.checked_add(1).unwrap() // These ids really need to be unique.
}

/// Runs the function, inside which tasks can be parallellized using
/// [`ThreadPoolScope`].
///
/// Returns None if the arena does not have space for the task queues, or if the
/// thread pool has any pending tasks. In these cases, the function won't be run
/// at all.
///
/// Analogous to `std::thread::scope`, except that instead of spawning threads,
/// this handle allows sending work across the thread pool this was created
/// with.
///
/// Any tasks left unjoined are joined after the closure has finished running.
#[track_caller]
#[must_use]
pub fn thread_pool_scope<'env, F, T>(
    thread_pool: &'env mut ThreadPool,
    arena: &'env LinearAllocator,
    f: F,
) -> Option<T>
where
    F: for<'scope> FnOnce(&'scope ThreadPoolScope<'scope, 'env>) -> T,
{
    if thread_pool.has_pending() {
        return None;
    }

    let max_queued_tasks = thread_pool.queue_len() * thread_pool.thread_count();
    let scope = ThreadPoolScope {
        state: RefCell::new(ScopeState {
            thread_pool,
            // Safety: task_buffer is only used to allocate within
            // ThreadPoolScope's functions, all the allocations are stored in
            // the thread pool channels, which are joined from and the
            // allocations dropped in ThreadPoolScope::force_join at the latest,
            // which is within 'env.
            task_buffer: unsafe { RingBuffer::new_non_static(arena, max_queued_tasks) }?,
            task_proxies: Queue::new(arena, max_queued_tasks)?,
            scatter_count: 0,
            scope_id: make_scope_id(),
        }),
        scope: PhantomData,
        env: PhantomData,
    };

    // The hard parts are adapted from std::thread::scope. Finicky stuff.

    #[cfg(not(feature = "std"))]
    let result = f(&scope);
    #[cfg(feature = "std")]
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&scope)));

    scope.force_join();
    assert!(
        !scope.state.borrow().thread_pool.has_pending(),
        "thread pool should've finished all its tasks in scope.force_join!",
    );

    #[cfg(not(feature = "std"))]
    {
        Some(result)
    }
    #[cfg(feature = "std")]
    match result {
        Err(err) => std::panic::resume_unwind(err),
        Ok(result) => Some(result),
    }
}

pub struct ScatterHandle<T: 'static> {
    scatter_position: u64,
    scope_id: u64,
    _type_holder: PhantomData<&'static T>,
}

#[derive(Debug)]
struct GenericTask {
    data_ptr: *mut (),
    data_len: usize,
    func_ptr: *const (),
    func_proxy: fn(*const (), *mut (), usize),
}

// Safety: the type of the value pointed to by data_ptr is required to be Sync
// in ThreadPoolScope::scatter's type parameter.
unsafe impl Sync for GenericTask {}

#[derive(Debug)]
struct TaskProxy {
    handle: TaskHandle<GenericTask>,
    metadata: RingAllocationMetadata,
    scatter_position: u64,
}

struct ScopeState<'a> {
    thread_pool: &'a mut ThreadPool,
    task_buffer: RingBuffer<GenericTask>,
    task_proxies: Queue<'a, TaskProxy>,
    scatter_count: u64,
    scope_id: u64,
}

pub struct ThreadPoolScope<'scope, 'env: 'scope> {
    state: RefCell<ScopeState<'env>>,

    // These two match the PhantomDatas in a `std::thread::Scope`.
    scope: PhantomData<&'scope mut &'scope ()>,
    env: PhantomData<&'env mut &'env ()>,
}

impl<'scope> ThreadPoolScope<'scope, '_> {
    /// Splits `data` into parts and runs `func` on multiple threads, passing in
    /// one part to each thread's `func`. Returns None if the thread pool's task
    /// queue is full.
    pub fn scatter<T>(
        &self,
        mut data: &'scope mut [T],
        func: fn(&mut [T]),
    ) -> Option<ScatterHandle<T>>
    where
        // NOTE: this bound is needed because GenericTask is unsafe-impl'd to be
        // Sync and we store a the `data` slice's pointer in it.
        T: Sync,
    {
        let mut state = self.state.borrow_mut();

        // TODO: add a counter for currently-queued-up tasks, and return None if there's as many as thread pool's queue len

        let scatter_position = state.scatter_count;
        state.scatter_count = state.scatter_count.checked_add(1).unwrap();

        fn proxy<T>(func_ptr: *const (), data_ptr: *mut (), data_len: usize) {
            // Safety: this pointer is cast from the destination type `fn(&mut
            // [T])` above, and transmuting pointers to fn pointers is ok
            // according to the [fn
            // docs](https://doc.rust-lang.org/core/primitive.fn.html#casting-to-and-from-integers).
            let func = unsafe { transmute::<*const (), fn(&mut [T])>(func_ptr) };

            let data_ptr = data_ptr as *mut T;
            // Safety: the pointer and length are valid regarding alignment etc.
            // basic stuff since they were created from a valid slice in the
            // first place. The lifetime is valid as well since data does not
            // escape this function, and this function is run within 'scope,
            // which is the lifetime of the original slice this pointer and
            // length are from.
            let data = unsafe { slice::from_raw_parts_mut(data_ptr, data_len) };

            func(data);
        }

        let len_per_thread = data.len().div_ceil(state.thread_pool.thread_count());
        for _ in 0..state.thread_pool.thread_count() {
            if data.is_empty() {
                break;
            }

            let data_part;
            (data_part, data) = data.split_at_mut(len_per_thread.min(data.len()));

            // Allocate the GenericTask for 'scope.
            let (generic_task, metadata) = state
                .task_buffer
                .allocate_box(GenericTask {
                    data_ptr: data_part.as_mut_ptr() as *mut (),
                    data_len: data_part.len(),
                    func_ptr: func as *const (),
                    func_proxy: proxy::<T>,
                })
                .unwrap()
                .into_parts();

            // Send off the GenericTask, using the proxy function from it to
            // call the user-provided one.
            let handle = state
                .thread_pool
                .spawn_task(generic_task, |task| {
                    (task.func_proxy)(task.func_ptr, task.data_ptr, task.data_len);
                })
                .unwrap();

            // Add the actual task handle to our queue (so it can be drained
            // before 'scope ends even if never manually joined).
            state
                .task_proxies
                .push_back(TaskProxy {
                    handle,
                    metadata,
                    scatter_position,
                })
                .unwrap();
        }

        assert!(
            data.is_empty(),
            "part of the slice wasn't sent to any thread?",
        );

        Some(ScatterHandle {
            scatter_position,
            scope_id: state.scope_id,
            _type_holder: PhantomData,
        })
    }

    /// Waits until the work related to the [`ScatterHandle`] is done, and then
    /// returns the processed data back.
    ///
    /// Returns None if called in a different order as
    /// [`ThreadPoolScope::scatter`] was called — this is a FIFO queue of work.
    ///
    /// ### Panics
    ///
    /// If the handle is from a different [`ThreadPoolScope`].
    pub fn gather<T>(&self, handle: ScatterHandle<T>) -> Option<&'scope mut [T]> {
        let mut state = self.state.borrow_mut();

        assert_eq!(
            state.scope_id, handle.scope_id,
            "this scatter handle was not returned from this scope",
        );

        let mut slice_ptr = None;
        let mut slice_len = 0;

        while {
            if let Some(task_proxy) = state.task_proxies.peek_front() {
                task_proxy.scatter_position == handle.scatter_position
            } else {
                false
            }
        } {
            let proxy = state.task_proxies.pop_front().unwrap();
            let generic_task = state.thread_pool.join_task(proxy.handle).unwrap();

            if slice_ptr.is_none() {
                slice_ptr = Some(generic_task.data_ptr as *mut T);
            }
            slice_len += generic_task.data_len;

            // Safety: this TaskProxy was created from a matching box and metadata,
            // with the box "hidden" behind the task handle. Since result is the
            // same box we passed into spawn_task, which in turn was from the same
            // RingBox as the metadata, these two form a valid pair.
            let boxed = unsafe { RingBox::from_parts(generic_task, proxy.metadata) };
            state.task_buffer.free_box(boxed).unwrap();
        }

        let slice_ptr = slice_ptr?;

        // Safety: the pointer is from the first task that was spawned, so it's
        // also the base pointer of the slice passed into the original scatter
        // call. Similarly, since len is a sum of all the tasks's lengths, it
        // sums up to the length of the original slice. Since the original slice
        // was passed in for 'scope, the lifetime is valid as well.
        Some(unsafe { slice::from_raw_parts_mut(slice_ptr, slice_len) })
    }

    /// Joins the pending tasks on the thread pool, dropping the results.
    fn force_join(&self) {
        let mut state = self.state.borrow_mut();
        while let Some(proxy) = state.task_proxies.pop_front() {
            let result = state.thread_pool.join_task(proxy.handle).unwrap();
            // Safety: this TaskProxy was created from a matching box and
            // metadata, with the box "hidden" behind the task handle. Since
            // result is the same box we passed into spawn_task, which in turn
            // was from the same RingBox as the metadata, these two form a valid
            // pair.
            let ring_box = unsafe { RingBox::from_parts(result, proxy.metadata) };
            state.task_buffer.free_box(ring_box).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scatter_and_gather_work() {
        use super::thread_pool_scope;
        use crate::{allocators::StaticAllocator, static_allocator};
        use platform_abstraction_layer::thread_pool::leak_single_threaded_thread_pool;

        static ARENA: &StaticAllocator = static_allocator!(1000);
        let mut thread_pool = leak_single_threaded_thread_pool(1);

        let mut data = [1u32; 4];
        thread_pool_scope(&mut thread_pool, ARENA, |pool| {
            let handle = pool.scatter(&mut data, |data| data[0] = 123).unwrap();
            // data[0] = 5; // can't access here, good
            let data = pool.gather(handle).unwrap();
            assert_eq!([123, 1, 1, 1], data);
        })
        .unwrap();
        assert_eq!(123, data[0]);
    }

    #[test]
    fn tasks_are_joined_after_scope_ends() {
        use super::thread_pool_scope;
        use crate::{allocators::StaticAllocator, static_allocator};
        use platform_abstraction_layer::thread_pool::leak_single_threaded_thread_pool;

        static ARENA: &StaticAllocator = static_allocator!(1000);
        let mut thread_pool = leak_single_threaded_thread_pool(1);

        let mut data = [0u32; 4];
        thread_pool_scope(&mut thread_pool, ARENA, |pool| {
            pool.scatter(&mut data, |data| data[0] = 123);
        })
        .unwrap();
        assert_eq!(123, data[0]);
    }

    #[cfg(feature = "std")]
    #[test]
    fn tasks_are_joined_even_if_closure_panics() {
        extern crate std;

        use super::thread_pool_scope;
        use crate::{allocators::StaticAllocator, static_allocator};
        use core::panic::AssertUnwindSafe;
        use platform_abstraction_layer::thread_pool::leak_single_threaded_thread_pool;

        static ARENA: &StaticAllocator = static_allocator!(1000);
        let mut thread_pool = leak_single_threaded_thread_pool(1);

        let mut data = [0u32; 4];

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            thread_pool_scope(&mut thread_pool, ARENA, |pool| {
                pool.scatter(&mut data, |data| data[0] = 123);
                panic!("Oh no, a panic inside the scope function!");
            })
            .unwrap();
        }));
        assert!(result.is_err());

        assert_eq!(123, data[0]);
    }

    #[cfg(not(feature = "std"))]
    #[ignore = "panic handling requires the 'std' feature"]
    #[test]
    fn tasks_are_joined_even_if_closure_panics() {}
}
