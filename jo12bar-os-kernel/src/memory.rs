//! Memory setup, mapping, and allocation.

use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use mem_util::KiB;
use x86_64::{
    structures::paging::{
        frame, FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PhysFrame, Size4KiB,
        Translate,
    },
    PhysAddr, VirtAddr,
};

/// Initialize a new [`OffsetPageTable`].
///
/// # Safety
/// - The caller must guarantee that the complete physical memory is mapped to
///   virtual memory at the passed `physical_memory_offset`.
/// - This function must only be called once to avoid aliasing `&mut` references
///   (which is undefined behaviour).
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = unsafe {
        // Safety:
        // - The caller needs to verify that physical_memory_offset is valid
        // - The caller needs to make sure the function is called once and only once
        active_level_4_table(physical_memory_offset)
    };

    // Safety:
    // - The caller needs to verify that physical_memory_offset is valid
    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}

/// Returns a mutable reference to the active level 4 table.
///
/// # Safety
/// - The caller must guarantee that the complete physical memory is mapped to
///   virtual memory at the passed `physical_memory_offset`.
/// - This function must only be called once to avoid aliasing `&mut` references
///   (which is undefined behaviour).
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    // Safety:
    // - The caller needs to verify that physical_memory_offset is valid
    // - The caller needs to make sure the function is called once and only once
    unsafe { &mut *page_table_ptr }
}

/// A [`FrameAllocator`] that always returns `None`.
pub struct EmptyFrameAllocator;

// Safety: `allocate_frame()` never actually returns or allocates frames, so we
// don't need to worry about only returning unique unused frames.
unsafe impl FrameAllocator<Size4KiB> for EmptyFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        None
    }
}

/// A [`FrameAllocator`] that returns usable frames from the bootloader's memory map.
pub struct BootInfoFrameAllocator {
    memory_regions: &'static MemoryRegions,
    next: usize,
}

impl<'a> BootInfoFrameAllocator {
    /// Create a [`FrameAllocator`] from the passed memory map.
    ///
    /// # Safety
    /// - The caller must guarantee that the passed memory map is valid. The main
    ///   requirement is that all frames marked as `USABLE` in it are _actually_
    ///   unused.
    pub unsafe fn init(memory_regions: &'static MemoryRegions) -> Self {
        Self {
            memory_regions,
            next: 0,
        }
    }

    /// Returns an iterator over the usable frames specified in the memory map.
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        let regions = self.memory_regions.iter();
        let usable_regions = regions.filter(|r| r.kind == MemoryRegionKind::Usable);

        // map each region to its address range
        let addr_ranges = usable_regions.map(|r| r.start..r.end);

        // transform to iterator of frame start addresses
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(KiB!(4)));

        // create `PhysFrame` types from the start addresses
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

/// Safety: As long as the caller upholds the safety contraints of
/// [`BootInfoFrameAllocator::init()`] this trait implementation will be safe,
/// as it will iterate through a valid list of unused frames.
unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}
