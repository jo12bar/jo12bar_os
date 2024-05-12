#![no_std]
#![no_main]

use core::panic::PanicInfo;

mod framebuffer;

bootloader_api::entry_point!(kernel_main);

/// Kernel entry point.
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let color = framebuffer::Color::rgb(0, 0, 255);
        for x in 0..100 {
            for y in 0..100 {
                let position = framebuffer::Position::new(20 + x, 100 + y);
                framebuffer::set_pixel_in(framebuffer, position, color)
            }
        }
    }

    #[allow(clippy::empty_loop)]
    loop {}
}

/// Called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
