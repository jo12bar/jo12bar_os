//! Interrupt setup and handlers.

use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spinning_top::Spinlock;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::{gdt, hlt_loop, serial_print};

/// Interrupt vector number offset for the primary Programmable Interrupt Controller.
pub const PIC_1_OFFSET: u8 = 32;
/// Interrupt vector number offset for the secondary Programmable Interrupt Controller.
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Two chained Programmable Interrupt Controllers.
pub static PICS: Spinlock<ChainedPics> =
    Spinlock::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Interrupt indexes in the Interrupt Descriptor Table, past the first 32 pre-defined CPU indices.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }

    // fn as_usize(self) -> usize {
    //     usize::from(self.as_u8())
    // }
}

lazy_static! {
    /// The interrupt descriptor table, which lives for the entire time the kernel is running.
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt[InterruptIndex::Timer.as_u8()]
            .set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_u8()]
            .set_handler_fn(keyboard_interrupt_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);

        idt
    };
}

/// Initialize the [`InterruptDescriptorTable`].
pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    log::info!("EXCEPTION: BREAKPOINT\n{stack_frame:#?}");
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT (error_code={error_code})\n{stack_frame:#?}");
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use core::fmt::Write;

    unsafe {
        if let Some(Some(l)) = core::ptr::addr_of!(crate::logger::LOGGER).as_ref() {
            if let Some(mut canvas_writer_lock) = l.try_lock() {
                if let Some(canvas_writer) = canvas_writer_lock.as_mut() {
                    write!(canvas_writer, ".").unwrap();
                }
            }
        }
    }

    serial_print!(".");

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
    use x86_64::instructions::port::Port;

    static KEYBOARD: Spinlock<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Spinlock::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore,
        ));

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);

    let scancode: u8 = unsafe { port.read() };
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode('\x1B') => {
                    log::trace!("received keyboard interrupt, char=<ESC>")
                }
                DecodedKey::Unicode(character) => {
                    log::trace!("received keyboard interrupt, char={character}")
                }
                DecodedKey::RawKey(key) => log::trace!("received keyboard interrupt, key={key:?}"),
            }
        }
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    log::warn!(
        "EXCEPTION: Page fault\n    \
        Accessed address: {:?}\n    \
        Error code: {error_code:?}\n\
        {stack_frame:#?}",
        Cr2::read(),
    );

    hlt_loop();
}
