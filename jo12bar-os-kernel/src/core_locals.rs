//! This provides the [locals!] macro as well as [CoreLocals] struct
//! which can be used to access per-core data.
//!
//! Largely based on [WasabiOS][wasabios_core_locals], with some of my own
//! modifications.
//!
//! [wasabios_core_locals]: https://github.com/Wasabi375/WasabiOS/blob/f4520ca20b0e0a2595f5c218701c32e01551820f/wasabi-kernel/src/core_local.rs

use core::{
    arch::asm,
    fmt,
    hint::spin_loop,
    ops,
    ptr::addr_of_mut,
    sync::atomic::{self, AtomicU64, AtomicU8},
};

use alloc::boxed::Box;
use mem_util::{sync::InterruptState, types::CoreId};
use x86_64::VirtAddr;

use crate::cpu;

/// A counter used to sign an ID for each core.
///
/// Each core called [AtomicU8::fetch_add] to get its ID and automatically
/// increment it for the next core ensuring IDs are unique.
///
/// As a side-effect, this is also the number of cores that have been started.
///
/// TODO: Implement actually booting more than one core :)
static CORE_ID_COUNTER: AtomicU8 = AtomicU8::new(0);

/// The number of cores that have finished booting.
static CORE_READY_COUNT: AtomicU8 = AtomicU8::new(0);

/// An array with [`VirtAddr`]s pointing to each core's instance of the [`CoreLocals`]
/// struct, indexed by the core ID.
static mut CORE_LOCALS_VADDRS: [VirtAddr; 255] = [VirtAddr::zero(); 255];

/// A [`CoreLocals`] instance used during the boot process.
static mut BOOT_CORE_LOCALS: CoreLocals = CoreLocals::new();

/// An atomic reference counter that automatically decrements its count when a
/// corresponding [`AutoRefCounterGuard`] is dropped.
///
/// Use [`AutoRefCount::increment()`] to get an [`AutoRefCounterGuard`], which
/// will decrement this struct's internal counter once dropped.
#[derive(Debug)]
pub struct AutoRefCounter(AtomicU64);

impl AutoRefCounter {
    /// Create a new [`AutoRefCounter`].
    pub const fn new(init: u64) -> Self {
        Self(AtomicU64::new(init))
    }

    /// Returns the current count.
    pub fn count(&self) -> u64 {
        self.0.load(atomic::Ordering::SeqCst)
    }

    /// Increment the count and return a [`AutoRefCounterGuard`], which increments
    /// _this_ struct's internal count when dropped.
    pub fn increment(&self) -> AutoRefCounterGuard<'_> {
        self.0.fetch_add(1, atomic::Ordering::SeqCst);
        AutoRefCounterGuard(self)
    }
}

impl Default for AutoRefCounter {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Guard struct which increments the count of the associated [`AutoRefCounter`]
/// when dropped.
pub struct AutoRefCounterGuard<'a>(&'a AutoRefCounter);

impl<'a> Drop for AutoRefCounterGuard<'a> {
    fn drop(&mut self) {
        (self.0).0.fetch_sub(1, atomic::Ordering::SeqCst);
    }
}

impl<'a> ops::Deref for AutoRefCounterGuard<'a> {
    type Target = &'a AutoRefCounter;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Data that is available globally, but is local to each initialized core.
///
/// There is exactly one instance of this per core that has successfully run
/// through the boot sequence (the core first called [`core_boot`], and later called [`init`]).
///
/// There is also an additional instance (`BOOT_CORE_LOCALS`) that is used after [`core_boot`] is
/// finished and before [`init`] is called.
#[derive(Debug)]
#[repr(C)]
pub struct CoreLocals {
    /// The virtual address of this struct.
    ///
    /// This is used in combination with the `GS` segment to access this struct.
    ///
    /// NOTE: This must be the first entry in the struct, as `unsafe` pointer
    /// math is used to load this struct from the `GS` segment.
    virt_addr: VirtAddr,

    /// A lock used to access the critical boot section.
    ///
    /// This lock is taken when [`core_boot`] is called and released at the end
    /// of [`init`].
    boot_lock: AtomicU8,

    /// This core's unique ID.
    ///
    /// Each core has sequential IDs starting from 0 and ending at [`get_started_core_count`].
    pub core_id: CoreId,

    // /// The core local apic id. This is assigned by the hardware and is not necessarially
    // /// equal to the core id.
    // pub apic_id: CoreId,
    /// Current depth of interrupts.
    ///
    /// This is incremented whenever an interrupt fires, and decremented once
    /// it's done. Therefore, `interrupt_depth.count() > 0` implies we're in an
    /// interrupt.
    interrupt_depth: AutoRefCounter,

    /// Current depth of exceptions.
    ///
    /// This is incremented whenever an exception fires, and decremented once
    /// it's done. Therefore, `exception_depth.count() > 0` implies we're in an
    /// exception.
    exception_depth: AutoRefCounter,

    /// A count of how many times [`CoreLocals::disable_interrupts()`] has been called.
    ///
    /// We only reenable interrupts once this hits 0. This is decremented in
    /// [`CoreLocals::enable_interrupts()`].
    interrupts_disable_count: AtomicU64,
    // /// A lock holding the local apic. This can be [None] if the apic has not been
    // /// initialized.
    // ///
    // /// # Safety:
    // ///
    // /// [cpu::apic::init] must be called before this can be used
    // pub apic: UnwrapTicketLock<Apic>,

    // /// Core locals used by tests
    // #[cfg(feature = "test")]
    // pub test_local: TestCoreLocals
}

impl CoreLocals {
    /// Create an empty [`CoreLocals`] struct.
    pub const fn new() -> Self {
        Self {
            virt_addr: VirtAddr::zero(),
            boot_lock: AtomicU8::new(0),
            core_id: CoreId(0),
            // apic_id: CoreId(0),
            interrupt_depth: AutoRefCounter::new(0),
            exception_depth: AutoRefCounter::new(0),

            // interrupts_disable_count is 1, because the boot section does not allow
            // for interrupts, after all we have not initialized them.
            interrupts_disable_count: AtomicU64::new(1),
            // apic: unsafe { UnwrapTicketLock::new_non_preemtable_uninit() },

            // #[cfg(feature = "test")]
            // test_local: TestCoreLocals::new(),
        }
    }

    /// Increment the [Self::interrupt_depth] and return a guard to decrement it again.
    pub fn inc_interrupt(&self) -> AutoRefCounterGuard<'_> {
        self.interrupt_depth.increment()
    }

    /// Increment the [Self::exception_depth] and return a guard to decrement it again.
    pub fn inc_exception(&self) -> AutoRefCounterGuard<'_> {
        self.exception_depth.increment()
    }

    /// Returns `true` if this core is currently in an interrupt
    pub fn in_interrupt(&self) -> bool {
        self.interrupt_depth.count() > 0
    }

    /// Returns `true` if this core is currently in an exception
    pub fn in_exception(&self) -> bool {
        self.exception_depth.count() > 0
    }

    /// Try to enable interrupts if possible.
    ///
    /// This will decrement [Self::interrupt_depth] and will only enable interrupts
    /// when `interrupt_depth == 0`.
    ///
    /// Also interrupts will never be enabled if we are currently inside an interrupt.
    /// In that case exiting the interrupt will reenable interrupts.
    ///
    /// # Safety
    /// - Must be called once for each call to [`CoreLocals::disable_interrupts()`].
    /// - Caller must ensure that the interrupts themselves are safe.
    pub unsafe fn enable_interrupts(&self) {
        let old_disable_count = self
            .interrupts_disable_count
            .fetch_sub(1, atomic::Ordering::SeqCst);

        // If we're not already in an interrupt, and we decremented the
        // interrupt outstanding to 0, we can actually enable interrupts.
        //
        // Since it's possible interrupts can be enabled when we enter an
        // interrupt, if we acquire a lock in an interrupt and release it it
        // may attempt to re-enable interrupts. Thus, we never allow enabling
        // interrupts from an interrupt handler. This means interrupts will
        // correctly get re-enabled in this case when the IRET loads the old
        // interrupt flag as part of the EFLAGS register.
        if old_disable_count == 1 && !self.in_interrupt() {
            // Safety: Not currently in an interrupt, and the outstanding interrupt count is 0
            unsafe {
                cpu::enable_interrupts();
            }
        }
    }

    /// Disable interrupts and increment [Self::interrupt_depth].
    ///
    /// # Safety
    /// - Caller must ensure that interrupts can be safely disabled.
    pub unsafe fn disable_interrupts(&self) {
        self.interrupts_disable_count
            .fetch_add(1, atomic::Ordering::SeqCst);
        // Safety: see above
        unsafe {
            cpu::disable_interrupts();
        }
    }

    /// Returns `true` if interrupts are currently enabled.
    ///
    /// This can break if [`cpu::disable_interrupts`] is used instead of the
    /// core local function [`CoreLocals::disable_interrupts`].
    ///
    /// This function uses a [relaxed load][atomic::Ordering::Relaxed], and
    /// should therefore only be used for diagnostics.
    pub fn interrupts_enabled(&self) -> bool {
        self.interrupts_disable_count
            .load(atomic::Ordering::Relaxed)
            == 0
    }

    /// Returns `true` if this core is used as the bootstrap processor.
    pub fn is_bsp(&self) -> bool {
        self.core_id.is_bsp()
    }
}

impl Default for CoreLocals {
    fn default() -> Self {
        Self::new()
    }
}

/// Start the core boot process, allowing the `locals!` macro to access the
/// `BOOT_CORE_LOCALS` region.
///
/// This will create a critical section that only 1 CPU can enter at a time,
/// which will end when [`init`] is called.
///
/// Returns the unique [`CoreId`] of the newly-booted core.
///
/// # Safety
/// - Must only be called once per CPU core, at the start of kernel execution.
pub unsafe fn core_boot() -> CoreId {
    let core_id: CoreId = CORE_ID_COUNTER
        .fetch_add(1, atomic::Ordering::AcqRel)
        .into();

    // Safety: This is only safe as long as we are the only core to access this.
    // Right now, we might not be. We'll ensure that we're the only core to
    // try and access this by taking CoreLocals::boot_lock. If we successfully
    // obtain CoreLocals::boot_lock, then accessing the entire rest of the
    // struct will be safe.
    let boot_core_locals = unsafe { &mut *addr_of_mut!(BOOT_CORE_LOCALS) };

    // Safety: We are still setting up all the interrupt handlers, locks, etc.
    // So we can disable this now until everything is set up.
    unsafe {
        cpu::disable_interrupts();
    }

    // Spin until we can lock BOOT_CORE_LOCALS. This is critical for safe access
    // to BOOT_CORE_LOCALS.
    while boot_core_locals.boot_lock.load(atomic::Ordering::SeqCst) != core_id.0 {
        spin_loop()
    }

    cpu::set_gs_base(boot_core_locals as *const CoreLocals as u64);

    // Set up locals region for booting the core.
    boot_core_locals.core_id = core_id;
    boot_core_locals.virt_addr = VirtAddr::from_ptr(boot_core_locals);

    assert_eq!(
        boot_core_locals.interrupt_depth.count(),
        0,
        "tried to boot a core while in an interrupt!"
    );
    assert_eq!(
        boot_core_locals.exception_depth.count(),
        0,
        "tried to boot a core while in an exception!"
    );
    assert_eq!(
        boot_core_locals
            .interrupts_disable_count
            .load(atomic::Ordering::Relaxed),
        1,
        "tried to boot a core after interrupts have already been enabled!"
    );

    core_id
}

/// Ends the core boot process.
///
/// After this call, `locals!` will return a final, heap-backed [`CoreLocals`]
/// memory region.
///
/// # Safety
///
/// This function must only be called once per CPU core, after [`core_boot`] has
/// been called, and also after memory and logging have been initialized.
pub unsafe fn init(core_id: CoreId) {
    // let apic_id = cpuid::apic_id().into();

    let mut core_local = Box::new(CoreLocals {
        virt_addr: VirtAddr::zero(),
        boot_lock: AtomicU8::new(core_id.0),
        core_id,
        // apic_id
        interrupt_depth: AutoRefCounter::new(0),
        exception_depth: AutoRefCounter::new(0),

        // interrupts_disable_count is 1, because the boot section does not allow
        // for interrupts, after all we have not initialized them.
        interrupts_disable_count: AtomicU64::new(1),
        // apic: unsafe { UnwrapTicketLock::new_non_preemtable_uninit() },

        // #[cfg(feature = "test")]
        // test_local: TestCoreLocals::new(),
    });

    core_local.virt_addr = VirtAddr::from_ptr(core_local.as_ref());
    // assert!(!LockCellInternal::<Apic>::is_preemtable(&core_local.apic));
    log::debug!(
        "Core {}: CoreLocals initialized from boot locals\n{core_local:#?}",
        core_id.0
    );

    // Safety: We're in the kernel's boot process, and we only access our own
    // core's data.
    unsafe {
        assert!(
            CORE_LOCALS_VADDRS[core_id.0 as usize].is_null(),
            "core {} already has an entry in CORE_LOCALS_VADDRS",
            core_id.0
        );
        CORE_LOCALS_VADDRS[core_id.0 as usize] = core_local.virt_addr;
    }

    // Set the GS base to point to this CoreLocals instance. That way we can use the core!
    // macro to access CoreLocals.
    cpu::set_gs_base(core_local.virt_addr.as_u64());

    // Exit the critical boot section and let the next core enter.
    // Safety: At this point we are still in the boot process so we still have
    // unique access to BOOT_CORE_LOCALS.
    unsafe {
        BOOT_CORE_LOCALS
            .boot_lock
            .fetch_add(1, atomic::Ordering::SeqCst);
    }
    CORE_READY_COUNT.fetch_add(1, atomic::Ordering::Release);

    // Don't drop core_local. We want it to live forever on the heap. However,
    // we can't use a static variable - we need 1 per core.
    core::mem::forget(core_local);
    log::trace!("Core {}: Locals initialization done", core_id.0);
}

/// A zero-sized type (ZST) used to access the interrupt state of a core.
///
/// See [InterruptState].
pub struct CoreInterruptState;

impl InterruptState for CoreInterruptState {
    fn in_interrupt() -> bool {
        locals!().in_interrupt()
    }

    fn in_exception() -> bool {
        locals!().in_exception()
    }

    fn core_id() -> CoreId {
        locals!().core_id
    }

    unsafe fn enter_critical_section(disable_interrupts: bool) {
        // #[cfg(feature = "test")]
        // test_locals!().lock_count.fetch_add(1, Ordering::AcqRel);

        if disable_interrupts {
            // Safety: Disabling interrupts is ok for entering critical sections
            unsafe {
                locals!().disable_interrupts();
            }
        }
    }

    unsafe fn exit_critical_section(enable_interrupts: bool) {
        // #[cfg(feature = "test")]
        // test_locals!().lock_count.fetch_sub(1, Ordering::AcqRel);

        if enable_interrupts {
            // Safety: only called once, when a critical section is exited.
            unsafe { locals!().enable_interrupts() }
        }
    }

    fn instance() -> Self {
        CoreInterruptState
    }
}

impl fmt::Debug for CoreInterruptState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoreInterruptState")
            .field("core_id", &Self::core_id())
            .field("in_interrupt", &Self::in_interrupt())
            .field("in_exception", &Self::in_exception())
            .finish()
    }
}

/// The number of cores that have started
pub fn get_started_core_count(ordering: atomic::Ordering) -> u8 {
    CORE_ID_COUNTER.load(ordering)
}

/// The number of cores that have finished booting
pub fn get_ready_core_count(ordering: atomic::Ordering) -> u8 {
    CORE_READY_COUNT.load(ordering)
}

/// Returns the current core's [`CoreLocals`] struct.
///
/// # Safety
/// This assumes that the `GS` segment was initialized by [`init()`] to point to
/// the [`CoreLocals`] struct for this core.
pub unsafe fn get_core_locals() -> &'static CoreLocals {
    // Safety: we assume that GS contains the virtual address of the currently-executing
    // core's CoreLocals struct, which starts with its own address. Therefore, we
    // can access the CoreLocals virtual address using `gs:[0]`.
    unsafe {
        let ptr: usize;
        asm! {
            "mov {0}, gs:[0]",
            out(reg) ptr
        }

        &*(ptr as *const CoreLocals)
    }
}

/// A macro wrapper around [`get_core_locals`] returning this core's [`CoreLocals`] struct.
///
/// # Safety
///
/// This assumes that the `GS` segment was initialized by [`init()`] to point to
/// the [`CoreLocals`] struct for this core.
///
/// This macro includes the necessary unsafe block to allow calling this from safe
/// rust, but it should still be considered unsafe before [core_boot] and [init]
/// have been called for the current core.
#[macro_export]
macro_rules! locals {
    () => {{
        #[allow(unused_unsafe)]
        let locals = unsafe { $crate::core_locals::get_core_locals() };

        locals
    }};
}

pub use locals;
