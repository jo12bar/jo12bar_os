//! Global Descriptor Table setup and configuration.

use lazy_static::lazy_static;
use mem_util::KiB;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;
use x86_64::{
    instructions::{
        segmentation::{Segment, CS},
        tables::load_tss,
    },
    registers::segmentation::SS,
};

/// Index of the double_fault interrupt handler's stack in the Interrupt Stack Table.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Index of the page_fault interrupt handler's stack in the Interrup Stack Table.
pub const PAGE_FAULT_IST_INDEX: u16 = 1;

lazy_static! {
    /// The task state segment, which holds the privlege stack table, interrupt
    /// stack table, and I/O map base address.
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = KiB!(20);
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            #[allow(unused_unsafe)] // TODO(jo12bar): rust started complaining about the unsafe block, even though it's required
            let stack_start = VirtAddr::from_ptr(unsafe { core::ptr::addr_of!(STACK) });
            stack_start + STACK_SIZE as _
        };

        tss.interrupt_stack_table[PAGE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = KiB!(20);
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            #[allow(unused_unsafe)] // TODO(jo12bar): rust started complaining about the unsafe block, even though it's required
            let stack_start = VirtAddr::from_ptr(unsafe { core::ptr::addr_of!(STACK) });
            stack_start + STACK_SIZE as _
        };

        tss
    };

    /// The global descriptor table and its segment selectors. Primarily used for setting up the [`TSS`].
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(&TSS));
        (gdt, Selectors { code_selector, data_selector, tss_selector })
    };
}

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

/// Initialize the Global Descriptor Table.
pub fn init() {
    GDT.0.load();

    unsafe {
        CS::set_reg(GDT.1.code_selector);
        SS::set_reg(GDT.1.data_selector);
        load_tss(GDT.1.tss_selector);
    }
}
