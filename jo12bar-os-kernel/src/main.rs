//! # jo12bar_os `kernel` -- The kernel component of jo12bar_os.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

use core::panic::PanicInfo;

use jo12bar_os_kernel::{
    bootloader_config_common, hlt_loop, init, init_logger, memory::BootInfoFrameAllocator, DISPLAY,
};

/// Configuration for the bootloader.
const BOOTLOADER_CONFIG: bootloader_api::BootloaderConfig = {
    let config = bootloader_api::BootloaderConfig::new_default();
    bootloader_config_common(config)
};

bootloader_api::entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

/// Kernel entry point.
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    if boot_info.framebuffer.as_ref().is_none() {
        panic!("could not access framebuffer");
    }

    init(boot_info);
    init_logger();

    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_regions) };

    log::info!("Kernel initialized");

    hlt_loop();
}

/// Called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if let Some(d) = DISPLAY.get() {
        unsafe {
            d.force_unlock();
        }
    }
    log::error!("{}", info);
    hlt_loop();
}
