#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use conquer_once::spin::OnceCell;
use embedded_graphics::{pixelcolor::Rgb888, prelude::*};

mod framebuffer;
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

    // invoke a breakpoint exception
    x86_64::instructions::interrupts::int3();

    log::trace!("Testing logging");
    log::debug!("Testing logging");
    log::info!("Testing logging");
    log::warn!("Testing logging");
    log::error!("Testing logging");

    #[allow(clippy::empty_loop)]
    loop {}
}

fn init(boot_info: &'static mut bootloader_api::BootInfo) {
    interrupts::init_idt();

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
    loop {}
}
