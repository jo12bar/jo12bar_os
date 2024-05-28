//! # `jo12bar-os-kernel` -- The kernel component of jo12bar_os.

#![no_std]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use core::{
    ptr,
    sync::atomic::{self, AtomicU8},
};

use bootloader_api::BootInfo;
use mem_util::{types::CoreId, KiB};
use memory::BootInfoFrameAllocator;
use x86_64::{
    structures::paging::{PageSize, Size4KiB},
    VirtAddr,
};

pub mod allocator;
pub mod gdt;
pub mod graphics;
pub mod interrupts;
pub mod logger;
pub mod memory;
pub mod serial;

/// A counter used to sign an ID for each core.
///
/// Each core called [AtomicU8::fetch_add] to get its ID and automatically
/// increment it for the next core ensuring IDs are unique.
///
/// As a side-effect, this is also the number of cores that have been started.
///
/// TODO: Implement actually booting more than one core :)
static CORE_ID_COUNTER: AtomicU8 = AtomicU8::new(0);

/// Contains the [BootInfo] provided by the Bootloader
///
/// TODO: this breaks rust's uniquness guarantee, and is super racy overall. Need to figure out some
/// form of locking, but that's hard to do since the boot info needs to be stored here
/// before memory allocation can be set up (so no Arc's, so no Sync). We can't use a OnceCell
/// because some things (like the framebuffer) require mutability. As long as we're single-core
/// this doesn't *really* matter, but it makes me itchy.
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

/// Returns the [BootInfo] provided by the bootloader.
///
/// # Safety
/// - The caller must guarantee unique access.
/// - Must be called after [`init()`], or you'll get a null pointer.
pub unsafe fn boot_info() -> &'static mut BootInfo {
    unsafe { &mut *BOOT_INFO }
}

/// Initialize the kernel.
pub fn init(boot_info: &'static mut bootloader_api::BootInfo) {
    // Safety: TODO: This is not safe at all. But we're single-core, so synchronized
    // access doesn't matter yet.
    unsafe {
        BOOT_INFO = boot_info;
    }

    let core_id: CoreId = CORE_ID_COUNTER
        .fetch_add(1, atomic::Ordering::AcqRel)
        .into();

    serial_print!("booting core {core_id}... ");

    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    serial_println!("done");
    serial_print!("initializing logger... ");

    // Safety: Serial should be initialized by now
    unsafe {
        logger::init();
    }

    serial_println!("done");
    serial_print!("initializing BootInfoFrameAllocator... ");

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset.into_option().unwrap());
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_regions) };

    serial_println!("done");
    serial_print!("initializing kernel heap... ");

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    serial_println!("done");
    serial_print!("initializing graphics stack... ");

    // Safety: logging and alloc should be working
    unsafe {
        graphics::init(true);
    }

    serial_println!("done");
}

/// Loop endlessly, executing the x86 `hlt` instruction.
pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

/// Default kernel stack size (80 KiB)
pub const DEFAULT_STACK_SIZE: u64 = KiB!(80);
// assert that the stack size is a multiple of page size
static_assertions::const_assert!(DEFAULT_STACK_SIZE & 0xfff == 0);

/// The default number of 4KiB pages used for the kernel's stack.
///
/// Calculated from [`DEFAULT_STACK_SIZE`].
pub const DEFAULT_STACK_PAGE_COUNT: u64 = DEFAULT_STACK_SIZE / Size4KiB::SIZE;

/// Fills in bootloader configuration shared between normal and test mode kernels.
pub const fn bootloader_config_common(
    mut config: bootloader_api::BootloaderConfig,
) -> bootloader_api::BootloaderConfig {
    config.mappings.physical_memory = Some(bootloader_api::config::Mapping::Dynamic);
    config.kernel_stack_size = DEFAULT_STACK_SIZE;
    config
}

/// Codes to be written to the I/O port specified by the `iobase` argument to QEMU,
/// allowing QEMU to exit with exit status `(value << 1) | 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
#[allow(missing_docs)]
pub enum QemuExitCode {
    Success = 0x10,
    Failure = 0x11,
}

/// Exit QEMU with an `exit_code`.
///
/// # Safety
/// - The kernel must be running inside of QEMU
/// - The `isa-debug-exit` device must be enabled via port-mapped I/O, with a
///   port size of `0x04` and an address of `0xf4`. Starting QEMU with the arguments
///   `-device isa-debug-exit,iobase=0xf4,iosize=0x04` should be sufficient.
pub unsafe fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    // Safety: see above
    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}
