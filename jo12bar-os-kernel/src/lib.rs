//! # `jo12bar-os-kernel` -- The kernel component of jo12bar_os.

#![no_std]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

use conquer_once::spin::OnceCell;
use embedded_graphics::{pixelcolor::Rgb888, prelude::*};
use mem_util::KiB;
use x86_64::structures::paging::{PageSize, Size4KiB};

pub mod framebuffer;
pub mod gdt;
pub mod interrupts;
pub mod memory;

/// A global instance of [`framebuffer::Display`], locked behind a spinlock.
///
/// This is totally a hack until I figure out something better.
pub static DISPLAY: OnceCell<framebuffer::LockedDisplay> = OnceCell::uninit();

/// Initialize the kernel.
pub fn init(boot_info: &'static mut bootloader_api::BootInfo) {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    let framebuffer = boot_info.framebuffer.as_mut().unwrap();

    let display = DISPLAY.get_or_init(|| framebuffer::LockedDisplay::new(framebuffer.into()));
    display.lock().clear(Rgb888::BLACK).unwrap();
}

/// Initialize logging to the global [`framebuffer::Display`] instance.
pub fn init_logger() {
    let display = DISPLAY.get().unwrap();
    log::set_logger(display).expect("Logger has already been set");
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("Hello, kernel mode!");
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
