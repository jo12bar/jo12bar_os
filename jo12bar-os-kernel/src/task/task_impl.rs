use alloc::boxed::Box;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// A pinned, heap-allocated, dynamically-dispatched [Future].
pub struct Task {
    future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Task {
    /// Create a new task from a future.
    pub fn new<F>(future: F) -> Task
    where
        F: Future<Output = ()> + 'static,
    {
        Task {
            future: Box::pin(future),
        }
    }

    pub(super) fn poll(&mut self, cx: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(cx)
    }
}
