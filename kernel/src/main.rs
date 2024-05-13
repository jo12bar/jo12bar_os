#![no_std]
#![no_main]

use core::fmt::Write;
use core::panic::PanicInfo;

use conquer_once::spin::OnceCell;
use embedded_graphics::{pixelcolor::Rgb888, prelude::*};

mod framebuffer;

bootloader_api::entry_point!(kernel_main);

pub(crate) static DISPLAY: OnceCell<framebuffer::LockedDisplay> = OnceCell::uninit();

/// Kernel entry point.
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    if boot_info.framebuffer.as_ref().is_none() {
        panic!("could not access framebuffer");
    }

    let framebuffer = boot_info.framebuffer.as_mut().unwrap();

    let display = DISPLAY.get_or_init(|| framebuffer::LockedDisplay::new(framebuffer.into()));

    init_logger();

    {
        let mut d = display.lock();
        d.clear(Rgb888::BLUE).unwrap();

        for i in 0..=255 {
            write!(d, "{i:4}: ").unwrap();
            for j in 0..i {
                write!(d, "{}", j % 10).unwrap();
            }
            writeln!(d).unwrap();
        }
    }

    log::trace!("Testing logging");
    log::debug!("Testing logging");
    log::info!("Testing logging");
    log::warn!("Testing logging");
    log::error!("Testing logging");

    #[allow(clippy::empty_loop)]
    loop {}
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
