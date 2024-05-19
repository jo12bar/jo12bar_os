//! # jo12bar_os `kernel` -- The kernel component of jo12bar_os.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

use core::panic::PanicInfo;

use conquer_once::spin::OnceCell;
use embedded_graphics::{pixelcolor::Rgb888, prelude::*};

mod framebuffer;
mod gdt;
mod interrupts;

bootloader_api::entry_point!(kernel_main);

pub(crate) static DISPLAY: OnceCell<framebuffer::LockedDisplay> = OnceCell::uninit();

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

fn init(boot_info: &'static mut bootloader_api::BootInfo) {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    let framebuffer = boot_info.framebuffer.as_mut().unwrap();

    let display = DISPLAY.get_or_init(|| framebuffer::LockedDisplay::new(framebuffer.into()));
    display.lock().clear(Rgb888::BLACK).unwrap();
}

pub(crate) fn init_logger() {
    let display = DISPLAY.get().unwrap();
    log::set_logger(display).expect("Logger has already been set");
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("Hello, kernel mode!");
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
