//! Memory allocation.

use core::ops;
use mem_util::KiB;
use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

use crate::prelude::*;

pub mod bump;
pub mod linked_list;

#[global_allocator]
static ALLOCATOR: LockedAllocator<linked_list::LinkedListAllocator> =
    LockedAllocator::new(linked_list::LinkedListAllocator::new());

/// Start (virtual) address of the kernel's heap
pub const HEAP_START: VirtAddr = VirtAddr::new(0x4444_4444_0000);
/// Size of the kernel's heap
pub const HEAP_SIZE: u64 = KiB!(100);

struct LockedAllocator<A> {
    inner: TicketLock<A>,
}

impl<A> LockedAllocator<A> {
    pub const fn new(inner: A) -> Self {
        Self {
            inner: TicketLock::new_non_preemtable(inner),
        }
    }
}

impl<A> ops::Deref for LockedAllocator<A> {
    type Target = TicketLock<A>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<A> ops::DerefMut for LockedAllocator<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
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
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}
