// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Thread pool for running tasks on other threads.
//!
//! [`ThreadPool`] implements a FIFO task queue where the tasks are executed on
//! other threads, the amount of threads depending on how the [`ThreadPool`] is
//! constructed. As a fallback, single-threaded platforms are supported by
//! simply running the task in [`ThreadPool::join_task`].
//!
//! This module doesn't do any allocation, and isn't very usable on its own,
//! it's intended to be used  alongsideplatform-provided threading functions, by
//! the engine, to construct multithreading utilities.

use core::{marker::PhantomData, mem::transmute};

use crate::{
    channel::{Receiver, Sender},
    Box,
};

#[allow(unused_imports)] // used by docs
use crate::Pal;

/// Handle to a running or waiting task on a [`ThreadPool`].
///
/// These should be passed into [`ThreadPool::join_task`] in the same order as
/// they were created with [`ThreadPool::spawn_task`].
#[derive(Debug)]
pub struct TaskHandle<T: 'static> {
    thread_index: usize,
    task_position: u64,
    _type_holder: PhantomData<&'static T>,
}

/// Packets sent between threads to coordinate a [`ThreadPool`].
pub struct TaskInFlight {
    /// Whether or not [`Task::run`] has been run for this task.
    finished: bool,
    /// Extracted from: `Box<T>`.
    data: *mut (),
    /// Cast from: `fn(&mut T)`.
    func: *const (),
    /// Pass `self.func` and `self.data` in here to call the function with the
    /// right types.
    func_proxy: fn(func: *const (), data: *mut ()),
    /// Can be set to true by the processing thread to signal that the thread
    /// panicked. This will cause the join function to panic with "a thread in
    /// the thread pool panicked" when joining this task.
    thread_panicked: bool,
}

impl TaskInFlight {
    /// Process the task in this container. Returns false if the task has
    /// already been ran, in which case this function does nothing.
    pub fn run(&mut self) -> bool {
        if !self.finished {
            (self.func_proxy)(self.func, self.data);
            self.finished = true;
            true
        } else {
            false
        }
    }

    /// Signals the thread pool that the thread responsible for running this
    /// task panicked. This can be used to propagate the panic to the main
    /// thread.
    pub fn signal_panic(&mut self) {
        self.thread_panicked = true;
    }

    /// Panics if the thread running this task has panicked, runs the task if
    /// the task hasn't been ran and it didn't panic, and finally, returns the
    /// data operated on by this task. Called by [`ThreadPool::join_task`].
    ///
    /// ### Safety
    /// The type parameter `T` must match the original type parameter `T` of
    /// [`ThreadPool::spawn_task`] exactly.
    unsafe fn join<T>(mut self, run_if_not_finished: bool) -> Box<T> {
        if self.thread_panicked {
            panic!("a thread in the thread pool panicked");
        }

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

/// The sending half of a [`TaskChannel`].
pub type TaskSender = Sender<TaskInFlight>;
/// The receiving half of a [`TaskChannel`].
pub type TaskReceiver = Receiver<TaskInFlight>;
/// Channel used by [`ThreadPool`] for communicating with the processing
/// threads.
///
/// Passed into [`Pal::spawn_pool_thread`].
pub type TaskChannel = (TaskSender, TaskReceiver);

/// State held by [`ThreadPool`] for sending and receiving [`TaskInFlight`]s
/// between it and a thread.
///
/// Returned from [`Pal::spawn_pool_thread`], multiple of these are used to
/// create a [`ThreadPool`].
pub struct ThreadState {
    /// For sending tasks to the thread.
    sender: TaskSender,
    /// For getting tasks results back from the thread.
    receiver: TaskReceiver,
    /// The amount of tasks sent via `sender`. (Used for picking
    /// [`TaskHandle::task_position`] for send).
    sent_count: u64,
    /// The amount of tasks received via `receiver`. (Used for checking
    /// [`TaskHandle::task_position`] on recv).
    recv_count: u64,
}

impl ThreadState {
    /// Creates a new [`ThreadState`] from the relevant channel endpoints.
    ///
    /// `sender_to_thread` is used to send tasks to the thread, while
    /// `receiver_from_thread` is used to receive finished tasks, so there
    /// should be two channels for each thread.
    ///
    /// To implement a simple single-threaded [`ThreadPool`], the sender and
    /// receiver of just one channel could be passed here, in which case
    /// [`ThreadPool`] will run the task when joining that task in
    /// [`ThreadPool::join_task`].
    pub fn new(sender_to_thread: TaskSender, receiver_from_thread: TaskReceiver) -> ThreadState {
        ThreadState {
            sender: sender_to_thread,
            receiver: receiver_from_thread,
            sent_count: 0,
            recv_count: 0,
        }
    }
}

/// Thread pool for running compute-intensive tasks in parallel.
///
/// Note that the tasks are run in submission order (on multiple threads, if
/// available), so a task that e.g. blocks on a file read will prevent other
/// tasks from running.
pub struct ThreadPool {
    next_thread_index: usize,
    threads: Box<[ThreadState]>,
}

impl ThreadPool {
    /// Creates a new [`ThreadPool`], returning None if the channels don't have
    /// matching capacities.
    pub fn new(threads: Box<[ThreadState]>) -> Option<ThreadPool> {
        // Check that each channel has the same capacity
        let mut capacity = None;
        for thread in threads.iter() {
            if let Some(capacity) = capacity {
                if thread.receiver.capacity() != capacity || thread.sender.capacity() != capacity {
                    return None;
                }
            } else if thread.receiver.capacity() != thread.sender.capacity() {
                return None;
            } else {
                capacity = Some(thread.receiver.capacity());
            }
        }

        Some(ThreadPool {
            next_thread_index: 0,
            threads,
        })
    }

    /// Returns the amount of threads in this thread pool.
    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }

    /// Returns the length of a task queue.
    ///
    /// In total, tasks can be spawned without joining up to this amount times
    /// the thread count.
    pub fn queue_len(&self) -> usize {
        if let Some(thread) = self.threads.first() {
            thread.receiver.capacity() // Checked in new() to match all other channels too
        } else {
            0
        }
    }

    /// Returns true if the thread pool has any pending tasks in the queues.
    pub fn has_pending(&self) -> bool {
        self.threads
            .iter()
            .any(|thread| thread.recv_count != thread.sent_count)
    }

    /// Resets the counter used to assign tasks to different threads.
    ///
    /// After calling this, the next [`ThreadPool::spawn_task`] is sent off to
    /// the first thread, instead of whichever value the counter is on now.
    pub fn reset_thread_counter(&mut self) {
        self.next_thread_index = 0;
    }

    /// Schedules the function to be ran on a thread in this pool, passing in
    /// the data as an argument, if they fit in the task queue.
    ///
    /// The function passed in is only ever ran once. In a single-threaded
    /// environment, it is ran when `join_task` is called for this task,
    /// otherwise it's ran whenever the thread gets to it.
    ///
    /// The threads are not load-balanced, the assigned thread is simply rotated
    /// on each call of this function.
    ///
    /// Tasks should be joined ([`ThreadPool::join_task`]) in the same order as
    /// they were spawned, as the results need to be received in sending order
    /// for each thread. However, this ordering requirement only applies
    /// per-thread, so [`ThreadPool::thread_count`] subsequent spawns can be
    /// joined in any order amongst themselves — whether this is useful or not,
    /// is up to the joiner.
    pub fn spawn_task<T>(
        &mut self,
        data: Box<T>,
        func: fn(&mut T),
    ) -> Result<TaskHandle<T>, Box<T>> {
        if self.threads.is_empty() {
            return Err(data);
        }

        let thread_index = self.next_thread_index;
        let task_position = self.threads[thread_index].sent_count;

        let func = func as *const (); // type erase for TaskInFlight

        let data: *mut T = data.into_ptr();
        let data = data as *mut (); // type erase for TaskInFlight

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
            thread_panicked: false,
        };

        (self.threads[thread_index].sender)
            .send(task)
            // Safety: T is definitely correct, we just created this task with
            // the same type parameter.
            .map_err(|task| unsafe { task.join::<T>(false) })?;

        self.threads[thread_index].sent_count = task_position
            .checked_add(1)
            .expect("thread pool sent_count should not overflow a u64");
        self.next_thread_index = (thread_index + 1) % self.thread_count();

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
    /// Depending on the [`ThreadState`]s passed into the constructor, this
    /// could either call the function (if it's a one-channel state), or wait
    /// until another thread has finished calling it (if it's a two-channel
    /// state that actually has a corresponding parallel thread).
    pub fn join_task<T>(&mut self, handle: TaskHandle<T>) -> Result<Box<T>, TaskHandle<T>> {
        let current_recv_count = self.threads[handle.thread_index].recv_count;

        if handle.task_position != current_recv_count {
            return Err(handle);
        }

        let task = self.threads[handle.thread_index].receiver.recv();
        // Safety: the TaskHandle returned from the spawn function
        // has the correct T for this, and since we've already
        // checked the thread index and task position, we know this
        // matches the original spawn call (and thus its type
        // parameter) for this data.
        let data = unsafe { task.join::<T>(true) };
        self.threads[handle.thread_index].recv_count += 1;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use crate::{self as pal, channel::leak_channel};
    use alloc::boxed::Box;

    use super::{TaskInFlight, ThreadPool, ThreadState};

    #[derive(Debug)]
    struct ExampleData(u32);

    #[test]
    fn single_threaded_pool_works() {
        // Generally you'd create two channels for thread<->thread
        // communication, but in a single-threaded situation, the channel works
        // as a simple work queue.
        let (tx, rx) = leak_channel::<TaskInFlight>(1);
        let thread_state = ThreadState::new(tx, rx);
        let threads = Box::leak(Box::new([thread_state]));
        let mut thread_pool = ThreadPool::new(pal::Box::from_mut(threads)).unwrap();

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
