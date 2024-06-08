//! CPU utilities and initialization procedures.

pub use instructions::*;

mod instructions {
    use x86_64::{
        instructions::{self, interrupts},
        registers::model_specific::Msr,
    };

    /// [MSR][Msr] for the active GS segment base.
    static mut IA32_GS_BASE: Msr = Msr::new(0xc0000101);

    /// [MSR][Msr] for the active FS segment base.
    static mut IA32_FS_BASE: Msr = Msr::new(0xc0000100);

    /// Issues a single halt instruction
    #[inline]
    pub fn halt_single() {
        instructions::hlt();
    }

    /// Issues the halt instruction in a loop.
    #[inline]
    pub fn halt() -> ! {
        loop {
            halt_single();
        }
    }

    /// Disables interrupts.
    ///
    /// When possible `locals!().disable_interrupts()` should be used instead.
    ///
    /// ## See
    /// [crate::core_local::CoreLocals]
    /// [crate::locals]
    ///
    /// # Safety
    /// Caller must ensure that disabling interrupts won't violate any safety guarantees.
    #[inline(always)]
    pub unsafe fn disable_interrupts() {
        interrupts::disable();
    }

    /// Enables interrupts.
    ///
    /// When possible `locals!().enable_interrupts()` should be used instead.
    ///
    /// ## See
    /// [crate::core_local::CoreLocals]
    /// [crate::locals]
    ///
    /// # Safety
    /// Caller must ensure that enabling interrupts won't violate any safety guarantees.
    #[inline(always)]
    pub unsafe fn enable_interrupts() {
        interrupts::enable();
    }

    /// Get the current GS segment base.
    #[inline]
    pub fn get_gs_base() -> u64 {
        // Safety: Reading from the GS segment is safe, as no side-effects are possible.
        unsafe { IA32_GS_BASE.read() }
    }

    /// Set the GS segment base to something.
    #[inline]
    pub fn set_gs_base(base: u64) {
        // Safety: Writing to the GS segment is safe, as no side-effects are possible.
        unsafe { IA32_GS_BASE.write(base) };
    }

    /// Get the current FS segment base.
    #[inline]
    pub fn get_fs_base() -> u64 {
        // Safety: Reading from the FS segment is safe, as no side-effects are possible.
        unsafe { IA32_FS_BASE.read() }
    }

    /// Set the FS segment base to something.
    #[inline]
    pub fn set_fs_base(base: u64) {
        // Safety: Writing to the FS segment is safe, as no side-effects are possible.
        unsafe { IA32_FS_BASE.write(base) };
    }
}
