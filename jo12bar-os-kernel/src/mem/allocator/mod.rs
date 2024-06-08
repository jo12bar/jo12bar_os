//! Memory allocation.

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr;
use linked_list_allocator::LockedHeap;
use mem_util::KiB;
use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Start (virtual) address of the kernel's heap
pub const HEAP_START: VirtAddr = VirtAddr::new(0x4444_4444_0000);
/// Size of the kernel's heap
pub const HEAP_SIZE: usize = KiB!(100);

/// A dummy allocator that always errors when [`Dummy::alloc()`] is called.
pub struct Dummy;

// Safety: This allocator always returns errors, and never actually allocates anything.
unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        panic!("dummy allocator does not support deallocation, and dealloc should never be called");
    }
}

/// Initialize the kernel's heap.
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_end = HEAP_START + HEAP_SIZE as _ - 1u64;
        let heap_start_page = Page::containing_address(HEAP_START);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START.as_mut_ptr(), HEAP_SIZE);
    }

    Ok(())
}
