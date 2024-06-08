//! Framebuffer-based graphics code for the kernel.
//!
//! Heavily based on [Wasabi375/WasabiOS](https://github.com/Wasabi375/WasabiOS/blob/main/wasabi-kernel/src/graphics/mod.rs),
//! but modified to hook into the [`embedded_graphics`] ecosystem.

use core::slice;

use embedded_graphics::{mono_font::ascii::FONT_8X13, prelude::*};

use crate::{graphics::tty::color, logger::LOGGER, prelude::*};

use self::{
    canvas::CanvasWriter,
    framebuffer::{
        startup::{take_boot_framebuffer, HARDWARE_FRAMEBUFFER_START_INFO},
        Framebuffer, HARDWARE_FRAMEBUFFER,
    },
};

pub mod canvas;
pub mod framebuffer;
pub mod tty;

/// A point in 2D space.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
#[repr(C)]
#[allow(missing_docs)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

impl Point {
    /// Create a new 2D point.
    pub const fn new(x: u32, y: u32) -> Self {
        Point { x, y }
    }
}

impl AsRef<[u32]> for Point {
    fn as_ref(&self) -> &[u32] {
        unsafe { slice::from_raw_parts(&self.x as *const u32, 2) }
    }
}

impl AsMut<[u32]> for Point {
    fn as_mut(&mut self) -> &mut [u32] {
        unsafe { slice::from_raw_parts_mut(&mut self.x as *mut u32, 2) }
    }
}

/// Initialize graphics.
///
/// # Safety
/// - Must only be called once during allocation
/// - Requires logging and heap access
pub unsafe fn init(framebuffer_logger: bool) {
    let fb: Framebuffer = unsafe {
        take_boot_framebuffer()
            .expect("No hardware framebuffer found")
            .into()
    };

    unsafe {
        HARDWARE_FRAMEBUFFER_START_INFO = Some((fb.start, fb.info));
    }

    *HARDWARE_FRAMEBUFFER.lock() = Some(fb);

    if framebuffer_logger {
        init_framebuffer_logger();
    }
}

/// Initialize logging to the screen.
///
/// # Safety
/// - Requires heap access
fn init_framebuffer_logger() {
    let mut fb = if let Some(fb) = HARDWARE_FRAMEBUFFER.lock().take() {
        fb
    } else {
        log::warn!("Framebuffer already taken. Framebuffer logger will not be created.");
        return;
    };

    let bg_color = color::DEFAULT_BACKGROUND;

    fb.clear(bg_color).unwrap();

    // TODO: Finish this

    let canvas_writer: CanvasWriter<_> = CanvasWriter::builder()
        .font(FONT_8X13)
        .canvas(fb)
        .margin_left(10)
        .margin_right(10)
        .margin_top(10)
        .margin_bottom(10)
        .background_color(bg_color)
        .log_errors(true)
        .build()
        .expect("Canvas writer should be fully initialized");

    // let canvas_lock = TicketLock::new_non_preemtable(canvas_writer);

    // let mut fb_logger: OwnLogger<CanvasWriter<Framebuffer>, _> = OwnLogger::new(canvas_lock);
    // setup_logger_module_rename(&mut fb_logger);

    // if let Some(dispatch_logger) = unsafe { LOGGER.as_ref() } {
    //     let logger = TargetLogger::new_secondary_boxed("framebuffer", Box::from(fb_logger));

    //     dispatch_logger.with_logger(logger)
    // } else {
    //     panic!("No global logger found to register the framebuffer logger");
    // }

    if let Some(hacky_logger) = unsafe { LOGGER.as_ref() } {
        hacky_logger.set_canvas_writer(Some(canvas_writer));
    } else {
        panic!("No global logger found to register the framebuffer logger.");
    }

    log::info!("Framebuffer logger initialized.");
}
