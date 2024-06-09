//! Provides [BumpAllocator], a basic kernel bump allocator.

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use log::trace;

use super::LockedAllocator;
use crate::prelude::*;

/// A simple bump allocator for the kernel's use.
///
/// Allocates memory linearly and only keeps track of the numebr of allocated
/// bytes and number of allocations. Can only free all of its memory at once.
#[derive(Debug)]
pub struct BumpAllocator {
    /// A pointer to the first byte of our memory chunk.
    start: *mut u8,
    /// A pointer to the last byte of our memory chunk.
    end: *mut u8,
    /// The bump pointer.
    ///
    /// # Safety
    /// At all times, we must ensure that `start <= ptr <= end`.
    ptr: *mut u8,
    /// Current number of allocations.
    allocations: usize,
}

// Safety: We're just using the pointers in BumpAllocator for bookkeeping - we
// don't actually own anything. So this struct is still Sendable, though Rust
// gets scared an auto-marks it as non-Send.
unsafe impl Send for BumpAllocator {}

impl BumpAllocator {
    /// Create a new, empty bump allocator.
    pub const fn new() -> Self {
        BumpAllocator {
            start: ptr::null_mut(),
            end: ptr::null_mut(),
            ptr: ptr::null_mut(),
            allocations: 0,
        }
    }

    /// Initialize the bump allocator with the given heap bounds.
    ///
    /// This method is unsafe because the caller must ensure that the given
    /// memory range is unused. Also, this method must be called only once.
    ///
    /// # Safety
    /// Must only be called once.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        trace!(
            "Initializing bump allocator, heap_start=0x{heap_start:x}, heap_end=0x{:x}, heap_size=0x{heap_size:x}",
            heap_start + heap_size
        );
        self.start = heap_start as *mut u8;
        self.end = (heap_start + heap_size) as *mut u8;
        self.ptr = self.end;
    }

    #[inline]
    unsafe fn is_last_allocation(&self, ptr: NonNull<u8>) -> bool {
        self.ptr == ptr.as_ptr()
    }
}

unsafe impl GlobalAlloc for LockedAllocator<BumpAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut bump = self.lock();

        let size = layout.size();
        let align = layout.align();

        debug_assert!(align > 0);
        debug_assert!(align.is_power_of_two());

        let ptr = bump.ptr as usize;

        let new_ptr = match ptr.checked_sub(size) {
            None => return ptr::null_mut(),
            Some(p) => p,
        };

        // Round down to the requested alignment.
        let new_ptr = new_ptr & !(align - 1);

        let start = bump.start as usize;
        if new_ptr < start {
            // out of memory!
            return ptr::null_mut();
        }

        bump.ptr = new_ptr as *mut u8;

        bump.allocations += 1;
        bump.ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut bump = self.lock();

        debug_assert!(!ptr.is_null());
        let ptr = unsafe { NonNull::new_unchecked(ptr) };

        bump.allocations -= 1;

        // If the pointer is the last allocation we made we can reuse the bytes.
        // Otherwise, leak until all allocations are released.
        if unsafe { bump.is_last_allocation(ptr) } {
            bump.ptr = unsafe { ptr.as_ptr().add(layout.size()) };
        } else if bump.allocations == 0 {
            bump.ptr = bump.end;
        }
    }
}

impl Default for BumpAllocator {
    fn default() -> Self {
        Self::new()
    }
}
