//! Provides a simple fixed-size block allocator. This is the main allocator
//! used in the kernel.

use alloc::alloc::{GlobalAlloc, Layout};
use core::{
    fmt, mem,
    ptr::{self, NonNull},
};

use x86_64::VirtAddr;

use super::{linked_list::LinkedListAllocator, LockedAllocator};
use crate::prelude::*;

/// Represent a free block of memory.
#[derive(Debug)]
struct ListNode {
    next: Option<&'static mut ListNode>,
}

/// The block sizes to use.
///
/// The sizes must be power of 2 because they are also used as the block
/// alignment (alignments must always be powers of 2).
///
/// For allocations greater than the maximum block size in this list, we'll
/// fall back to a linked list allocator.
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

/// Choose an appropriate block size for the given layout.
///
/// Returns an index into the [`BLOCK_SIZES`] array.
fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

/// A simple fixed-size block allocator.
///
/// For allocations larger than 2048 bytes in size, this allocator will fall
/// back to [`linked_list_allocator`].
pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: super::linked_list::LinkedListAllocator,
}

impl FixedSizeBlockAllocator {
    /// Creates an empty [`FixedSizeBlockAllocator`].
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: LinkedListAllocator::new(),
        }
    }

    /// Initialize the allocator with the given heap bounds.
    ///
    /// # Safety
    /// - The caller must guarantee that the given heap bounds are valid and that
    ///   the heap is unused.
    /// - This method must only be called once.
    pub unsafe fn init(&mut self, heap_start: VirtAddr, heap_size: u64) {
        // Safety: see above
        unsafe {
            self.fallback_allocator.init(
                VirtAddr::from_ptr(heap_start.as_mut_ptr::<u8>()),
                heap_size as _,
            );
        }
    }

    /// Allocates using the fallback allocator.
    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.fallback_allocator.allocate_first_fit(layout) {
            Some(ptr) => ptr.as_ptr(),
            None => ptr::null_mut(),
        }
    }
}

unsafe impl GlobalAlloc for LockedAllocator<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                match allocator.list_heads[index].take() {
                    Some(node) => {
                        allocator.list_heads[index] = node.next.take();
                        node as *mut ListNode as *mut u8
                    }
                    None => {
                        // no block exists in list --> allocate new block
                        let block_size = BLOCK_SIZES[index];
                        // only works if all block sizes are a power of 2
                        let block_align = block_size;
                        // Safety: all block sizes are a power of 2!! So this should be totally fine.
                        let layout =
                            unsafe { Layout::from_size_align_unchecked(block_size, block_align) };
                        allocator.fallback_alloc(layout)
                    }
                }
            }
            None => allocator.fallback_alloc(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                let new_node = ListNode {
                    next: allocator.list_heads[index].take(),
                };
                // verify that block has size and alignment required for storing node
                debug_assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                debug_assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node_ptr = ptr as *mut ListNode;
                // Safety: we verified this is safe
                unsafe {
                    new_node_ptr.write(new_node);
                    allocator.list_heads[index] = Some(&mut *new_node_ptr);
                }
            }
            None => {
                let ptr = NonNull::new(ptr).unwrap();
                // Safety: This block is allocated by the linked list so this is fine
                unsafe {
                    allocator.fallback_allocator.deallocate(ptr, layout);
                }
            }
        }
    }
}

/// Write addresses of all fixed-size free blocks to a [writer][Write].
impl fmt::Debug for FixedSizeBlockAllocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixedSizeBlockAllocator")
            .field("list_heads", &self.list_heads)
            .finish_non_exhaustive()
    }
}

impl Default for FixedSizeBlockAllocator {
    fn default() -> Self {
        Self::new()
    }
}
