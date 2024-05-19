//! # jo12bar_os `kernel` -- The kernel component of jo12bar_os.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

use core::panic::PanicInfo;

use jo12bar_os_kernel::{init, init_logger, DISPLAY};

bootloader_api::entry_point!(kernel_main);

/// Kernel entry point.
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    if boot_info.framebuffer.as_ref().is_none() {
        panic!("could not access framebuffer");
    }

    init(boot_info);
    init_logger();

    log::trace!("Testing logging");
    log::debug!("Testing logging");
    log::info!("Testing logging");
    log::warn!("Testing logging");
    log::error!("Testing logging");

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

/// Loop endlessly, executing the x86 `hlt` instruction.
pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
