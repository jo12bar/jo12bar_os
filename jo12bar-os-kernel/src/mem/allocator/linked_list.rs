//! Provides [LinkedListAllocator], a basic kernel allocator that keeps track
//! of free regions with a linked list.

use alloc::alloc::{GlobalAlloc, Layout};
use core::{mem, ptr};

use x86_64::VirtAddr;

use super::LockedAllocator;
use crate::prelude::*;

#[derive(Debug)]
struct ListNode {
    size: u64,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new(size: u64) -> Self {
        Self { size, next: None }
    }

    fn start_addr(&self) -> VirtAddr {
        VirtAddr::from_ptr(self as *const Self)
    }

    fn end_addr(&self) -> VirtAddr {
        self.start_addr() + self.size
    }
}

/// A kernel allocator that keeps track of free regions using a linked list.
#[derive(Debug)]
pub struct LinkedListAllocator {
    /// The start of the "freelist" - a linked list of free regions of memory.
    head: ListNode,
}

impl LinkedListAllocator {
    /// Create an empty [`LinkedListAllocator`].
    ///
    /// Should be initialized with [`LinkedListAllocator::init()`] before use.
    pub const fn new() -> Self {
        Self {
            head: ListNode::new(0),
        }
    }

    /// Initialize the allocator with the given heap bounds.
    ///
    /// # Safety
    /// - Caller must guarantee that the given heap bounds are valid and that
    ///   the heap is unused.
    /// - This method must be called only once.
    pub unsafe fn init(&mut self, heap_start: VirtAddr, heap_size: u64) {
        // Safety: See above
        unsafe {
            self.add_free_region(heap_start, heap_size);
        }
    }

    /// Add the given memory region to the front of the freelist.
    ///
    /// # Safety
    /// - Caller must guarantee that the given start address and size are valid,
    ///   and that the memory region is unused.
    unsafe fn add_free_region(&mut self, addr: VirtAddr, size: u64) {
        // Ensure the free region is capable of holding a ListNode
        assert_eq!(addr.align_up(mem::align_of::<ListNode>() as u64), addr);
        assert!(size >= mem::size_of::<ListNode>() as _);

        // Create a new list node and append it at the start of the lsit
        let mut node = ListNode::new(size);
        node.next = self.head.next.take();
        let node_ptr = addr.as_mut_ptr::<ListNode>();
        // Safety: we've verified that the target pointer is aligned for a ListNode
        // and is big enough for a ListNode. The caller needs to make sure it's
        // actually free.
        unsafe {
            node_ptr.write(node);
            self.head.next = Some(&mut *node_ptr)
        }
    }

    /// Looks for a free region with the size and alignment required by `Layout`
    /// and removes it from the list.
    ///
    /// Returns a tuple of the list node and the start address of the allocation.
    ///
    /// Returns `None` if no suitable region could be found.
    fn find_region(&mut self, layout: &Layout) -> Option<(&'static mut ListNode, VirtAddr)> {
        // Reference to current list node, updated each iteration
        let mut current = &mut self.head;
        // Look for a large-enough memory region in the linked list
        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(region, layout) {
                // Region is suitable for allocation --> remove node from list
                let next = region.next.take();
                let ret = Some((current.next.take().unwrap(), alloc_start));
                current.next = next;
                return ret;
            } else {
                // Region not suitable --> continue with next region
                current = current.next.as_mut().unwrap();
            }
        }

        // No suitable regions found
        None
    }

    /// Try to use the given region for an allocatio with a given [Layout].
    ///
    /// Returns the allocation start address on success.
    fn alloc_from_region(region: &ListNode, layout: &Layout) -> Result<VirtAddr, ()> {
        let size = layout.size();
        let align = layout.align();

        let alloc_start = region.start_addr().align_up(align as u64);
        let alloc_end = VirtAddr::new(alloc_start.as_u64().checked_add(size as _).ok_or(())?);

        if alloc_end > region.end_addr() {
            // Region too small
            return Err(());
        }

        let excess_size = region.end_addr() - alloc_end;
        if excess_size > 0 && excess_size < mem::size_of::<ListNode>() as u64 {
            // Rest of region too small too small to hold a ListNode (required
            // because the allocation splits the region into a used and a free
            // part).
            return Err(());
        }

        // Region suitable for allocation!
        Ok(alloc_start)
    }

    /// Adjust the given layout so that the resulting allocated memory region
    /// is also capable of storing a `ListNode`.
    ///
    /// Returns the adjusted size and alignment inside a new `Layout` struct.
    fn size_align(layout: &Layout) -> Layout {
        let layout = layout
            .align_to(mem::align_of::<ListNode>())
            .expect("adjusting alignment failed")
            .pad_to_align();
        let size = layout.size().max(mem::size_of::<ListNode>());

        // Safety:
        // - `align` cannot be zero and will be a multiple of two, as it comes
        //   from a valid pre-existing Layout
        // - `size`, when rounded up to the nearest multiple of ListNode's alignment,
        //   will not overflow isize because it will be a valid Layout size
        //   or a size of ListNode, whichever is larger.
        unsafe { Layout::from_size_align_unchecked(size, layout.align()) }
    }
}

unsafe impl GlobalAlloc for LockedAllocator<LinkedListAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Perform layout adjustments
        let layout = LinkedListAllocator::size_align(&layout);

        let mut allocator = self.lock();

        if let Some((region, alloc_start)) = allocator.find_region(&layout) {
            let alloc_end = VirtAddr::new(
                alloc_start
                    .as_u64()
                    .checked_add(layout.size() as _)
                    .expect("allocation end address overflow"),
            );

            let excess_size = region.end_addr() - alloc_end;
            if excess_size > 0 {
                // Safety: We've guaranteed the given start address and size
                // are valid. Caller must ensure the region is free (though it
                // should be as free as can be, due to our Self::find_region() call).
                unsafe {
                    allocator.add_free_region(alloc_end, excess_size);
                }
            }
            alloc_start.as_mut_ptr()
        } else {
            ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Perform layout adjustments
        let size = LinkedListAllocator::size_align(&layout).size();
        // Safety: We've guaranteed the given start address and size
        // are valid. The region is being freed.
        unsafe {
            self.lock()
                .add_free_region(VirtAddr::from_ptr(ptr), size as _);
        }
    }
}

impl Default for LinkedListAllocator {
    fn default() -> Self {
        Self::new()
    }
}
