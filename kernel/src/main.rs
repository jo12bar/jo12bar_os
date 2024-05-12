#![no_std]
#![no_main]

use core::panic::PanicInfo;

use embedded_graphics::{
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Circle, PrimitiveStyleBuilder, Rectangle, Sector},
};

mod framebuffer;

bootloader_api::entry_point!(kernel_main);

/// Kernel entry point.
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    if boot_info.framebuffer.as_ref().is_none() {
        panic!("could not access framebuffer");
    }

    let mut display: framebuffer::Display = boot_info.framebuffer.as_mut().unwrap().into();

    // Create styles used by the drawing operations.
    let sector_style = PrimitiveStyleBuilder::new()
        .stroke_color(Rgb888::BLACK)
        .stroke_width(2)
        .fill_color(Rgb888::YELLOW)
        .build();
    let eye_style = PrimitiveStyleBuilder::new()
        .stroke_color(Rgb888::BLACK)
        .stroke_width(1)
        .fill_color(Rgb888::BLACK)
        .build();
    let bg_style = PrimitiveStyleBuilder::new()
        .fill_color(Rgb888::WHITE)
        .build();

    const STEPS: i32 = 10;
    let mut progress: i32 = 0;

    display.clear(Rgb888::WHITE).unwrap();

    // #[allow(clippy::empty_loop)]
    loop {
        let p = (progress - STEPS).abs();

        // Draw a Sector as the main Pacman feature.
        Sector::new(
            Point::new(2, 2),
            61,
            Angle::from_degrees((p * 30 / STEPS) as f32),
            Angle::from_degrees((360 - 2 * p * 30 / STEPS) as f32),
        )
        .into_styled(sector_style)
        .draw(&mut display)
        .unwrap();

        // Draw a Circle as the eye.
        Circle::new(Point::new(36, 16), 5)
            .into_styled(eye_style)
            .draw(&mut display)
            .unwrap();

        progress = (progress + 1) % (2 * STEPS + 1);

        // Clear
        Rectangle::new(Point::new(1, 1), Size::new(63, 63))
            .into_styled(bg_style)
            .draw(&mut display)
            .unwrap();
    }
}

/// Called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
