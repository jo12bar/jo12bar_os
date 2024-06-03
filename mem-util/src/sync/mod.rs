//! Synchronization primitives.
//!
//! Mostly based on the implementation in [WasabiOS](https://github.com/Wasabi375/WasabiOS),
//! with some minor tweaks.

use crate::types::CoreId;

pub mod lock_cell;
pub mod ticket_lock;

/// Trait that allows access to OS-level constructs defining interrupt state,
/// exception state, unique core IDs, and enter/exit lock (for interrupt
/// disabling and enabling) primitives.
pub trait InterruptState: 'static {
    /// Returns `true` if we're currently in an interrupt.
    fn in_interrupt() -> bool;

    /// Returns `true` if we're currently in an exception.
    ///
    /// This indicates that a lock cannot be held as we may have preempted a
    /// non-preemptable lock.
    fn in_exception() -> bool;

    /// Get the ID of the running core.
    ///
    /// This core ID *must* be unique to the core.
    fn core_id() -> CoreId;

    /// Signal the kernel that a critical section was entered (e.g. a lock was taken).
    ///
    /// If `disable_interrupts` is true, the lock does not support being interrupted
    /// and interrupts must therefore be disabled. This is also a prequisite for
    /// a lock to be taken within an interrupt.
    ///
    /// # Safety
    /// - Caller must call [`InterruptState::exit_critical_section()`] exactly once with the
    ///   same parameter passed for `enable_interrupts`.
    /// - If `disable_interrupts` is true, the caller must ensure that interrupts
    ///   can be disabled safely.
    unsafe fn enter_critical_section(disable_interrupts: bool);

    /// Signal the kernel that a critical section was exited (e.g. a lock was released).
    ///
    /// If `enable_interrupts` is true, the kernel will reenable interrupts if
    /// possible.
    ///
    /// # Safety
    /// - The caller must ensure that this function is called exactly once per
    ///   invocation of [`InterruptState::enter_critical_section()`] with the
    ///   same parameter as passed to this function.
    unsafe fn exit_critical_section(enable_interrupts: bool);
}
