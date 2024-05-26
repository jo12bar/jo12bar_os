//! A module containing logging and debug utilities.
//!
//! TODO: Implement a modular logger similar to that from WasabiOS. See:
//! <https://github.com/Wasabi375/WasabiOS/blob/2246c42cc2e296f9831b5daf5cb933fcead9ff3b/wasabi-kernel/src/logger.rs>
//!
//! TODO: Implement logging to serial interface.

use core::fmt::Write;

use log::{info, LevelFilter};
use spinning_top::{lock_api::MutexGuard, RawSpinlock, Spinlock};
use x86_64::instructions::interrupts::{self, without_interrupts};

use crate::{
    graphics::{canvas::CanvasWriter, framebuffer::Framebuffer},
    serial_println,
};

/// The static logger used by the [`log::log`] macro.
///
/// # Safety
/// This should never by modified outside of panics and [`init()`].
pub static mut LOGGER: Option<HackyLogger> = None;

/// A hacky, baseline logger that just outputs to the hardware framebuffer.
#[derive(Default)]
pub struct HackyLogger {
    canvas_writer: Spinlock<Option<CanvasWriter<'static, Framebuffer>>>,
}

impl HackyLogger {
    fn new() -> Self {
        Self::default()
    }

    /// Replace the current [`CanvasWriter`] with a new one.
    ///
    /// Can also be used to just remove the current [`CanvasWriter`] by passing `None`.
    /// Returns the old [`CanvasWriter`], or `None` if there wasn't one.
    pub fn set_canvas_writer(
        &self,
        new_writer: Option<CanvasWriter<'static, Framebuffer>>,
    ) -> Option<CanvasWriter<'static, Framebuffer>> {
        without_interrupts(|| {
            let mut cur_writer = self.canvas_writer.lock();
            let cur_writer_ref = &mut *cur_writer;
            core::mem::replace(cur_writer_ref, new_writer)
        })
    }

    /// Force-unlock the internal [`CanvasWriter`].
    ///
    /// # Safety
    /// Inherently unsafe. Only use in the global panic handler.
    pub unsafe fn force_unlock(&self) {
        // Safety: see above
        unsafe {
            self.canvas_writer.force_unlock();
        }
    }

    /// Try to aquire a lock on the internal [`CanvasWriter`] without blocking.
    ///
    /// See also [`Spinlock::try_lock`].
    #[inline]
    pub fn try_lock(
        &self,
    ) -> Option<MutexGuard<'_, RawSpinlock, Option<CanvasWriter<'static, Framebuffer>>>> {
        self.canvas_writer.try_lock()
    }
}

impl log::Log for HackyLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        const SGR_RESET: &str = "\x1b[0m";
        const SGR_BRBLACK: &str = "\x1b[90m";

        let sgr_color_escape = match record.level() {
            log::Level::Error => "\x1b[31m", // red
            log::Level::Warn => "\x1b[33m",  // yellow
            log::Level::Info => "\x1b[32m",  // green
            log::Level::Debug => "\x1b[34m", // blue
            log::Level::Trace => "\x1b[35m", // magenta
        };

        interrupts::without_interrupts(|| {
            let mut writer = self.canvas_writer.lock();
            serial_println!(
                "{SGR_RESET}{SGR_BRBLACK}[{sgr_color_escape}{:<5}{SGR_BRBLACK}]{SGR_RESET} {}",
                record.level(),
                record.args()
            );
            if let Some(writer) = &mut *writer {
                writeln!(
                    writer,
                    "{SGR_RESET}{SGR_BRBLACK}[{sgr_color_escape}{:<5}{SGR_BRBLACK}]{SGR_RESET} {}",
                    record.level(),
                    record.args()
                )
                .unwrap();
            }
        });
    }

    fn flush(&self) {}
}

/// Initializes the logger, piping all [log::log] calls into the first serial
/// port (TODO) and the framebuffer.
///
/// # Safety
/// Must only ever be called once at the start of the kernel boot process and after
/// serial is initialized.
pub unsafe fn init() {
    let hacky_logger = HackyLogger::new();

    // Safety: see above
    unsafe {
        LOGGER = Some(hacky_logger);

        let logger = LOGGER.as_mut().unwrap_unchecked();
        log::set_logger(logger).expect("logger has already been set");
    }

    log::set_max_level(LevelFilter::Trace);

    info!("Hacky logger initialized.");
}

/// A macro logging and returning the result of any expression.
/// The result of the expression is logged using the [log::debug] macro.
///
/// ```
/// assert_eq!(5, dbg!(5)); // also calls log::debug!(5)
/// ```
#[allow(unused_macros)]
#[macro_export]
macro_rules! dbg {
    () => {
        log::debug!(
            "[{}:{}:{}]",
            ::core::file!(),
            ::core::line!(),
            ::core::column!()
        )
    };
    ($val:expr) => {
        // Use of `match` here is intentional because it affects the lifetimes
        // of temporaries - https://stackoverflow.com/a/48732525/1063961
        match $val {
            tmp => {
                log::debug!(
                    "[{}:{}:{}] {} = {:#?}",
                    ::core::file!(),
                    ::core::line!(),
                    ::core::column!(),
                    ::core::stringify!($val),
                    &tmp
                );
                tmp
            }
        }
    };
}

/// Same as [todo!] but only calls a [log::warn] instead of [panic].
#[allow(unused_macros)]
#[macro_export]
macro_rules! todo_warn {
    () => {
        log::warn!(
            "[{}:{}:{}] not yet implemented",
            ::core::file!(),
            ::core::line!(),
            ::core::column!(),
        )
    };
    ($($arg:tt)+) => {
        log::warn!(
            "[{}:{}:{}] not yet implemented: {}",
            ::core::file!(),
            ::core::line!(),
            ::core::column!(),
            ::core::format_args!($($arg)+),
        )
    };
}

/// Same as [todo!] but only calls a [log::warn] instead of [panic].
#[allow(unused_macros)]
#[macro_export]
macro_rules! todo_error {
    () => {
        log::error!(
            "[{}:{}:{}] not yet implemented",
            ::core::file!(),
            ::core::line!(),
            ::core::column!(),
        )
    };
    ($($arg:tt)+) => {
        log::error!(
            "[{}:{}:{}] not yet implemented: {}",
            ::core::file!(),
            ::core::line!(),
            ::core::column!(),
            ::core::format_args!($($arg)+),
        )
    };
}
