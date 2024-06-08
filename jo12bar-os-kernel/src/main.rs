//! # jo12bar_os `kernel` -- The kernel component of jo12bar_os.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use core::panic::PanicInfo;

use jo12bar_os_kernel::{
    bootloader_config_common, core_locals::CoreInterruptState, cpu::halt, dbg, graphics, init,
    logger::LOGGER,
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

    // Allocate a number on the heap
    let heap_value = Box::new(41);
    log::info!("heap_value at {heap_value:p}");

    // Create a dynamically-sized vector
    let mut vec = Vec::new();
    for i in 0..500 {
        vec.push(i);
    }
    log::info!("vec at {:p}", vec.as_slice());

    // Create a reference-counted vector -> will be freed when count reaches 0
    let rc_vec = Rc::new(vec![1, 2, 3]);
    log::info!("current reference count is {}", Rc::strong_count(&rc_vec));
    let cloned = Rc::clone(&rc_vec);
    log::info!("current reference count is {}", Rc::strong_count(&rc_vec));
    core::mem::drop(cloned);
    log::info!("current reference count is {}", Rc::strong_count(&rc_vec));

    log::info!("Kernel initialized");

    dbg!();
    dbg!(&graphics::framebuffer::HARDWARE_FRAMEBUFFER);
    dbg!(&CoreInterruptState);

    log::trace!("Test trace log");
    log::debug!("Test debug log");
    log::info!("Test info log");
    log::warn!("Test warn log");
    log::error!("Test error log");

    halt();
}

/// Called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        if let Some(l) = LOGGER.as_ref() {
            l.force_unlock();
        }
    }
    // unsafe { jo12bar_os_kernel::exit_qemu(jo12bar_os_kernel::QemuExitCode::Failure) };
    log::error!("{}", info);
    halt();
}
