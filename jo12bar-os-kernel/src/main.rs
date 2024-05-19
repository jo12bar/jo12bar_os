//! # jo12bar_os `kernel` -- The kernel component of jo12bar_os.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use core::panic::PanicInfo;
use x86_64::VirtAddr;

use jo12bar_os_kernel::{
    allocator, bootloader_config_common, hlt_loop, init, init_logger,
    memory::{self, BootInfoFrameAllocator},
    DISPLAY,
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

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset.into_option().unwrap());
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_regions) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

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
