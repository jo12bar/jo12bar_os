//! A very simple, inefficient [Task] executor.

use alloc::collections::VecDeque;
use core::{
    ptr,
    task::{Context, RawWaker, RawWakerVTable, Waker},
};

use super::Task;

/// A very, VERY basic [Task] executor.
///
/// Note that this is very inefficient, as no effort is made to properly use
/// the [`Waker`] type. Tasks are just polled in a round-robin fashion.
pub struct SimpleExecutor {
    task_queue: VecDeque<Task>,
}

impl SimpleExecutor {
    /// Create a new basic [Task] executor.
    pub const fn new() -> SimpleExecutor {
        SimpleExecutor {
            task_queue: VecDeque::new(),
        }
    }

    /// Spawn a [Task] onto the simple executor.
    pub fn spawn(&mut self, task: Task) {
        self.task_queue.push_back(task)
    }

    /// Run the executor.
    pub fn run(&mut self) {
        while let Some(mut task) = self.task_queue.pop_front() {
            let waker = dummy_waker();
            let mut context = Context::from_waker(&waker);
            match task.poll(&mut context) {
                core::task::Poll::Ready(()) => {} // task done
                core::task::Poll::Pending => self.task_queue.push_back(task),
            }
        }
    }
}

impl Default for SimpleExecutor {
    fn default() -> Self {
        Self::new()
    }
}

fn dummy_raw_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        dummy_raw_waker()
    }

    let vtable = &RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(ptr::null(), vtable)
}

fn dummy_waker() -> Waker {
    // Safety: This waker doesn't actually do anything so this is fine.
    unsafe { Waker::from_raw(dummy_raw_waker()) }
}
