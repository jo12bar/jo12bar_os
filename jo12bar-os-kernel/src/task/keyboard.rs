//! A background task that handles incoming scancodes from the keyboard interrupt handler.

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::{task::AtomicWaker, Stream, StreamExt};
use log::{trace, warn};
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();

static WAKER: AtomicWaker = AtomicWaker::new();

/// Print keypresses to the log.
///
/// This future will never terminate, so you should run it as a background task via `spawn()`.
///
/// Panics if called more than once.
pub async fn print_keypresses() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    );

    // the scancode stream never ends, so this will never terminate
    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode('\x1B') => {
                        trace!("received keyboard interrupt, char=<ESC>")
                    }
                    DecodedKey::Unicode(character) => {
                        trace!("received keyboard interrupt, char={character}")
                    }
                    DecodedKey::RawKey(key) => {
                        trace!("received keyboard interrupt, key={key:?}")
                    }
                }
            }
        }
    }
}

/// Called by the keyboard interrupt handler.
///
/// Must not block or allocate.
pub(crate) fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if queue.push(scancode).is_err() {
            warn!("scancode queue full; dropping user input");
        } else {
            WAKER.wake();
        }
    } else {
        warn!("scancode queue uninitialized");
    }
}

/// An async stream of keyboard scancodes.
pub struct ScancodeStream {
    _private: (),
}

impl ScancodeStream {
    /// Initialize the keyboard scancode stream.
    ///
    /// Panics if called more than once.
    pub fn new() -> Self {
        SCANCODE_QUEUE
            .try_init_once(|| ArrayQueue::new(100))
            .expect("ScancodeStream::new() should be called only once");
        Self { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let queue = SCANCODE_QUEUE.try_get().expect("not initialized");

        // fast path - there's a scancode immediately available!
        if let Some(scancode) = queue.pop() {
            return Poll::Ready(Some(scancode));
        }

        // nothing available, get woken up later.
        WAKER.register(cx.waker());

        // the interrupt handler might've added something since the last check,
        // so check again just to be sure
        match queue.pop() {
            Some(scancode) => {
                WAKER.take();
                Poll::Ready(Some(scancode))
            }
            None => Poll::Pending,
        }
    }
}

impl Default for ScancodeStream {
    fn default() -> Self {
        Self::new()
    }
}
