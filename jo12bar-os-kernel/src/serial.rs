//! Utilities for communication over serial ports (primarily logging).

use lazy_static::lazy_static;
use spinning_top::Spinlock;
use uart_16550::SerialPort;
use x86_64::instructions::interrupts;

lazy_static! {
    /// The global UART serial port protected by a spinlock.
    pub static ref SERIAL1: Spinlock<SerialPort> = {
        // Safety: 0x3F8 is the standard port number for the first serial interface on x86.
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Spinlock::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _serial_print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;

    interrupts::without_interrupts(|| {
        SERIAL1
            .lock()
            .write_fmt(args)
            .expect("printing to serial failed");
    });
}

/// Prints to the host through the serial interface.
///
/// If running in QEMU, make sure QEMU is started with the arguments `-serial stdio`.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_serial_print(format_args!($($arg)*));
    };
}

/// Prints to the host through the serial interface, appending a newline.
///
/// If running in QEMU, make sure QEMU is started with the arguments `-serial stdio`.
#[macro_export]
macro_rules! serial_println {
    () => {
        $crate::serial_print!("\n")
    };
    ($fmt:expr) => {
        $crate::serial_print!("{}\n", format_args!($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::serial_print!("{}\n", format_args!($fmt, $($arg)*))
    };
}
