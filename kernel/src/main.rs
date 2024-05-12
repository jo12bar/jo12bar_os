#![no_std]
#![no_main]

use core::panic::PanicInfo;

bootloader_api::entry_point!(kernel_main);

/// Kernel entry point.
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        for byte in framebuffer.buffer_mut() {
            *byte = 0x90;
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
