use core::{marker::PhantomData, mem::transmute};

use crate::{
    channel::{Receiver, Sender},
    Box,
};

#[derive(Debug)]
pub struct TaskHandle<T: 'static> {
    thread_index: usize,
    task_position: u64,
    _type_holder: PhantomData<&'static T>,
}

struct TaskInFlight {
    /// Whether or not [`Task::run`] has been run for this task.
    finished: bool,
    /// Extracted from: `Box<T>`.
    data: *mut (),
    /// Cast from: `fn(&mut T)`.
    func: *const (),
    /// Pass `self.func` and `self.data` in here to call the function with the
    /// right types.
    func_proxy: fn(func: *const (), data: *mut ()),
}

impl TaskInFlight {
    fn run(&mut self) {
        (self.func_proxy)(self.func, self.data);
    }

    /// ### Safety
    /// The type parameter `T` must match the original type parameter `T` of
    /// [`ThreadPool::spawn_task`] exactly.
    unsafe fn join<T>(mut self, run_if_not_finished: bool) -> Box<T> {
        if !self.finished && run_if_not_finished {
            self.run();
        }

        // Safety: the *mut c_void was originally casted from a *mut T which in
        // turn was from a Box<T>, so this pointer has already been guaranteed
        // to live long enough. It is also not shared anywhere outside of this
        // struct, so this is definitely a unique reference to the memory.
        unsafe { Box::from_ptr(self.data as *mut T) }
    }
}

// Safety: the only non-Sync field, the data pointer, points to the T of a
// Box<T: Sync>.
unsafe impl Sync for TaskInFlight {}

struct ThreadState {
    /// For sending tasks to the thread.
    sender: Sender<TaskInFlight>,
    /// For getting tasks results back from the thread.
    receiver: Receiver<TaskInFlight>,
    /// The amount of tasks sent via `sender`. (Used for picking
    /// [`TaskHandle::task_position`] for send).
    sent_count: u64,
    /// The amount of tasks received via `receiver`. (Used for checking
    /// [`TaskHandle::task_position`] on recv).
    recv_count: u64,
}

/// Thread pool for running compute-intensive tasks in parallel.
///
/// Note that the tasks are run in submission order (on multiple threads, if
/// available), so a task that e.g. blocks on a file read will prevent other
/// tasks from running.
pub struct ThreadPool {
    next_thread_index: usize,
    threads: &'static mut [ThreadState],
}

impl ThreadPool {
    pub fn thread_count(&mut self) -> usize {
        self.threads.len()
    }

    pub fn spawn_task<T>(
        &mut self,
        data: Box<T>,
        func: fn(&mut T),
    ) -> Result<TaskHandle<T>, Box<T>> {
        if self.threads.is_empty() {
            return Err(data);
        }

        let thread_index = self.next_thread_index;
        self.next_thread_index = (thread_index + 1) % self.thread_count();

        let task_position = self.threads[thread_index].sent_count;
        self.threads[thread_index].sent_count = task_position
            .checked_add(1)
            .expect("sent task count should not overflow a u64. well done!");

        let func = func as *const ();

        let data: *mut T = data.into_ptr();
        let data = data as *mut (); // type erase for run()

        fn proxy<T>(func: *const (), data: *mut ()) {
            // Safety: this pointer is cast from the destination type `fn(&mut
            // T)` above, and transmuting pointers to fn pointers is ok
            // according to the [fn
            // docs](https://doc.rust-lang.org/core/primitive.fn.html#casting-to-and-from-integers).
            let func = unsafe { transmute::<*const (), fn(&mut T)>(func) };
            // Safety: this pointer is the same one created above from a Box<T>
            // (which had unique access to this memory), and it's safe to create
            // a mutable borrow of it, as this is the only function that will do
            // anything with the pointer, and this function is only ever called
            // once for any particular task.
            let data: &mut T = unsafe { &mut *(data as *mut T) };
            func(data);
        }

        let task = TaskInFlight {
            finished: false,
            data,
            func,
            func_proxy: proxy::<T>,
        };

        self.threads[thread_index]
            .sender
            .send(task)
            // Safety: T is definitely correct, we just created this task with
            // the same type parameter.
            .map_err(|task| unsafe { task.join::<T>(false) })?;

        Ok(TaskHandle {
            thread_index,
            task_position,
            _type_holder: PhantomData,
        })
    }

    /// Blocks on and returns the task passed into [`ThreadPool::spawn_task`],
    /// if it's next in the queue for the thread it's running on.
    ///
    /// The `Err` variant signifies that there's some other task that should be
    /// joined before this one. When spawning and joining tasks in FIFO order,
    /// this never returns an `Err`.
    ///
    /// Depending on the platform, this could either call the function, or wait
    /// until another thread has finished calling it.
    pub fn join_task<T>(&mut self, handle: TaskHandle<T>) -> Result<Box<T>, TaskHandle<T>> {
        let current_recv_count = self.threads[handle.thread_index].recv_count;
        if handle.task_position == current_recv_count {
            loop {
                if let Some(task) = self.threads[handle.thread_index].receiver.try_recv() {
                    // Safety: the TaskHandle returned from the spawn function
                    // has the correct T for this, and since we've already
                    // checked the thread index and task position, we know this
                    // matches the original spawn call (and thus its type
                    // parameter) for this data.
                    let data = unsafe { task.join::<T>(true) };
                    self.threads[handle.thread_index].recv_count += 1;
                    return Ok(data);
                }
            }
        }
        Err(handle)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::boxed::Box;

    use crate::{
        self as pal,
        channel::leak_channel,
        thread_pool::{TaskInFlight, ThreadState},
        ThreadPool,
    };

    #[derive(Debug)]
    struct ExampleData(u32);

    #[test]
    fn single_threaded_pool_works() {
        // Generally you'd create two channels for thread<->thread
        // communication, but in a single-threaded situation, the channel works
        // as a simple work queue.
        let (tx, rx) = leak_channel::<TaskInFlight>(1);
        let mut thread_pool: ThreadPool = ThreadPool {
            next_thread_index: 0,
            threads: Box::leak(Box::new([ThreadState {
                sender: tx,
                receiver: rx,
                sent_count: 0,
                recv_count: 0,
            }])),
        };

        let mut data = ExampleData(0);
        {
            // Safety: `data` is dropped after this scope, and this Box does not
            // leave this scope, so `data` outlives this Box.
            let data_boxed: pal::Box<ExampleData> = unsafe { pal::Box::from_ptr(&raw mut data) };
            assert_eq!(0, data_boxed.0);

            let handle = thread_pool.spawn_task(data_boxed, |n| n.0 = 1).unwrap();
            let data_boxed = thread_pool.join_task(handle).unwrap();
            assert_eq!(1, data_boxed.0);
        }
        #[allow(clippy::drop_non_drop)]
        drop(data); // `data` lives at least until here, at which point the unsafe box has been dropped
    }
}
