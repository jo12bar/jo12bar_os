//! A [ticket lock](https://en.wikipedia.org/wiki/Ticket_lock), which is a form
//! of spinlock with better fairness but higher uncontended latency.
//!
//! [`TicketLock`] is a [`LockCell`] implementation, that can be both preemtable or
//! not depending on how it is created ([`TicketLock::new`] vs
//! [`TicketLock::new_non_preemtable`]).
//!
//! [`UnwrapLock`] is a [`LockCell`] wrapper that allows accessing a
//! `UnwrapLock<MaybeUninit<T>>` as if it is an `LockCell<T>`.

use core::{
    cell::UnsafeCell,
    hint::spin_loop,
    marker::PhantomData,
    sync::atomic::{AtomicI64, AtomicU16, AtomicU64, Ordering},
};

use super::{
    lock_cell::{
        LockCell, LockCellGuard, LockCellInternal, ReadCellGuard, RwCellInternal, RwLockCell,
    },
    InterruptState,
};

/// A [ticket lock](https://en.wikipedia.org/wiki/Ticket_lock) implementation
/// for [`LockCell`].
///
/// - `T` is the type of data stored in the lock.
/// - `I` gives access to the core's interrupt state.
#[derive(Debug)]
pub struct TicketLock<T, I> {
    /// The current ticket that can access the lock.
    current_ticket: AtomicU64,
    /// The next ticket to give out.
    next_ticket: AtomicU64,
    /// The data held by the lock. We use [`UnsafeCell`] because we manually
    /// manage access to the data, respecting Rust's rules.
    data: UnsafeCell<T>,
    /// The current core holding the lock.
    owner: AtomicU16,
    /// `true` if the lock is *not* usable in interrupts.
    pub preemtable: bool,
    /// Act like we own access to the core's interrupt state.
    _interrupt_state: PhantomData<I>,
}

unsafe impl<T: Send, I: InterruptState> Send for TicketLock<T, I> {}
unsafe impl<T: Send, I: InterruptState> Sync for TicketLock<T, I> {}

impl<T, I> TicketLock<T, I> {
    /// Creates a new [`TicketLock`].
    pub const fn new(data: T) -> Self {
        Self {
            current_ticket: AtomicU64::new(0),
            next_ticket: AtomicU64::new(0),
            data: UnsafeCell::new(data),
            owner: AtomicU16::new(!0),
            preemtable: true,
            _interrupt_state: PhantomData,
        }
    }

    /// Creates a new __non-preemtable__ [`TicketLock`].
    ///
    /// This assumes that it is safe to disable interrupts while the lock is held.
    pub const fn new_non_preemtable(data: T) -> Self {
        Self {
            current_ticket: AtomicU64::new(0),
            next_ticket: AtomicU64::new(0),
            data: UnsafeCell::new(data),
            owner: AtomicU16::new(!0),
            preemtable: false,
            _interrupt_state: PhantomData,
        }
    }

    /// Write the "current" state of the ticket lock (not including the guarded data)
    /// to the `writer`.
    ///
    /// All internals are accessed with relaxed loads.
    pub fn write_state<W: core::fmt::Write>(&self, writer: &mut W) -> core::fmt::Result {
        let current = self.current_ticket.load(Ordering::Relaxed);
        let next = self.next_ticket.load(Ordering::Relaxed);
        let owner = self.owner.load(Ordering::Relaxed);
        write!(writer, "[TicketLock(c: {current}, n: {next}, o: {owner})]")
    }
}

impl<T: Send, I: InterruptState> LockCell<T> for TicketLock<T, I> {
    #[track_caller]
    fn lock(&self) -> LockCellGuard<'_, T, Self> {
        assert!(
            !self.preemtable || !I::in_interrupt(),
            "cannot use non-preemtable TicketLock in interrupt"
        );

        unsafe {
            // Safety: disabling interrupts is ok, for preemtable locks
            I::enter_critical_section(!self.preemtable);
        }

        let ticket = self.next_ticket.fetch_add(1, Ordering::SeqCst);

        while self.current_ticket.load(Ordering::SeqCst) != ticket {
            let owner = self.owner.load(Ordering::Acquire);
            if owner != !0 && owner == I::core_id().0 as u16 {
                panic!("TicketLock deadlock detected!")
            }
            spin_loop();
        }

        self.owner.store(I::core_id().0 as u16, Ordering::Release);

        LockCellGuard {
            lockcell: self,
            _phantom: PhantomData,
        }
    }

    #[track_caller]
    fn try_lock(&self) -> Option<LockCellGuard<'_, T, Self>> {
        if self.owner.load(Ordering::Acquire) == !0 {
            Some(self.lock())
        } else {
            None
        }
    }
}

impl<T, I: InterruptState> LockCellInternal<T> for TicketLock<T, I> {
    unsafe fn get(&self) -> &T {
        unsafe { &*self.data.get() }
    }

    unsafe fn get_mut(&self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }

    unsafe fn unlock<'s, 'l: 's>(&'s self, guard: &mut LockCellGuard<'l, T, Self>) {
        assert!(
            core::ptr::eq(self, guard.lockcell),
            "attempted to use a LockCellGuard to unlock a TicketLock that doesn't actually own the TicketLock"
        );

        // Safety: we checked that the LockCellGuard actually owns this TicketLock.
        unsafe {
            self.force_unlock();
        }
    }

    unsafe fn force_unlock(&self) {
        self.owner.store(!0, Ordering::Release);
        self.current_ticket.fetch_add(1, Ordering::SeqCst);

        // Safety: this will restore the interrupt state from when we called
        // enter_critical_section, so this is safe
        unsafe {
            I::exit_critical_section(!self.preemtable);
        }
    }

    fn is_unlocked(&self) -> bool {
        self.owner.load(Ordering::Acquire) == !0
    }

    fn is_preemtable(&self) -> bool {
        self.preemtable
    }
}

impl<T: Default, I> Default for TicketLock<T, I> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T: Default, I> TicketLock<T, I> {
    /// Creates a new non-preemtable [`TicketLock`] with `data` initialized to its default value.
    ///
    /// This assumes that it is safe to disable interrupts while the lock is held.
    pub fn default_non_preemtable() -> Self {
        Self::new_non_preemtable(Default::default())
    }
}

/// A [`RwLockCell`] implementation using a ticketing system.
pub struct RwTicketLock<T, I> {
    /// If positive, this is the number of readers that currently hold a guard.
    ///
    /// - If 0, no one holds a guard, neither read nor write.
    /// - If -1, there is a writer with a guard.
    access_count: AtomicI64,
    /// The data guarded by this lock
    data: UnsafeCell<T>,
    /// Set if the lock is usable in interrupts.
    pub preemtable: bool,
    /// Act like we own access to the core's interrupt state.
    _interrupt_state: PhantomData<I>,
}

unsafe impl<T: Send, I: InterruptState> Send for RwTicketLock<T, I> {}
unsafe impl<T: Send, I: InterruptState> Sync for RwTicketLock<T, I> {}

impl<T, I> RwTicketLock<T, I> {
    /// Creates a new [`RwTicketLock`].
    pub const fn new(data: T) -> Self {
        Self {
            access_count: AtomicI64::new(0),
            data: UnsafeCell::new(data),
            preemtable: true,
            _interrupt_state: PhantomData,
        }
    }

    /// creates a new non-preemtable [`RwTicketLock`].
    ///
    /// This assumes that it is safe to disable interrupts while the lock is held.
    pub const fn new_non_preemtable(data: T) -> Self {
        Self {
            access_count: AtomicI64::new(0),
            data: UnsafeCell::new(data),
            preemtable: false,
            _interrupt_state: PhantomData,
        }
    }
}

impl<T: Default, I> Default for RwTicketLock<T, I> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T: Default, I> RwTicketLock<T, I> {
    /// Creates a new non-preemtable [`RwTicketLock`] with `data` initialized to its default value.
    ///
    /// This assumes that it is safe to disable interrupts while the lock is held.
    pub fn default_non_preemtable() -> Self {
        Self::new_non_preemtable(Default::default())
    }
}

impl<T: Send, I: InterruptState> RwLockCell<T> for RwTicketLock<T, I> {
    fn read(&self) -> ReadCellGuard<'_, T, Self> {
        // NOTE: Because there can be multiple readers, RwLock is allowed in
        // interrupts even if preemtable.
        // Safety: Disabling interrupts is ok for preemtable locks.
        unsafe {
            I::enter_critical_section(false);
        }

        let mut cur_count = self.access_count.load(Ordering::Acquire);
        loop {
            while cur_count < 0 {
                spin_loop();
                cur_count = self.access_count.load(Ordering::Acquire);
            }
            match self.access_count.compare_exchange(
                cur_count,
                cur_count + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(previous) => {
                    assert!(
                        previous >= 0,
                        "attempted to take a read lock through RwTicketLock event though the reader count is less than 0"
                    );
                    break;
                }
                Err(new_current) => cur_count = new_current,
            }
        }

        ReadCellGuard {
            rw_cell: self,
            _phantom: PhantomData,
        }
    }
}

impl<T: Send, I: InterruptState> LockCell<T> for RwTicketLock<T, I> {
    #[track_caller]
    fn lock(&self) -> LockCellGuard<'_, T, Self> {
        assert!(
            !self.preemtable || !I::in_interrupt(),
            "cannot use non-preemtable RwTicketLock in interrupt"
        );

        unsafe {
            // Safety: For preemtable locks, disabling interrupts is ok.
            I::enter_critical_section(!self.preemtable);
        }

        loop {
            match self
                .access_count
                .compare_exchange(0, -1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(prev) => {
                    assert_eq!(
                        prev, 0,
                        "attempted to lock a RwTicketLock for writing while read locks exist"
                    );
                    break;
                }
                Err(_) => spin_loop(),
            }
        }

        LockCellGuard {
            lockcell: self,
            _phantom: PhantomData,
        }
    }

    #[track_caller]
    fn try_lock(&self) -> Option<LockCellGuard<'_, T, Self>> {
        if self.access_count.load(Ordering::SeqCst) == 0 {
            Some(self.lock())
        } else {
            None
        }
    }
}

impl<T, I: InterruptState> RwCellInternal<T> for RwTicketLock<T, I> {
    unsafe fn release_read<'s, 'l: 's>(&'s self, guard: &mut ReadCellGuard<'l, T, Self>) {
        assert!(
            core::ptr::eq(self, guard.rw_cell),
            "attempted to use a ReadCellGuard to release a RwTicketLock's read lock that doesn't actually own the RwTicketLock"
        );

        // Safety: We check above that the guard actually owns this lock
        unsafe {
            self.force_release_read();
        }
    }

    unsafe fn force_release_read(&self) {
        let previous_count = self.access_count.fetch_sub(1, Ordering::SeqCst);
        assert!(
            previous_count >= 1,
            "attempted to forcibly release a read lock for a RwTicketLock when no read locks exist"
        );
        // Safety: This will restore the interrupt state from when we called
        // enter_critical_section, so this is safe.
        unsafe {
            I::exit_critical_section(!self.preemtable);
        }
    }

    fn open_to_read(&self) -> bool {
        self.access_count.load(Ordering::SeqCst) >= 0
    }
}

impl<T, I: InterruptState> LockCellInternal<T> for RwTicketLock<T, I> {
    unsafe fn get(&self) -> &T {
        unsafe { &*self.data.get() }
    }

    unsafe fn get_mut(&self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }

    unsafe fn unlock<'s, 'l: 's>(&'s self, guard: &mut LockCellGuard<'l, T, Self>) {
        assert!(
            core::ptr::eq(self, guard.lockcell),
            "attempted to use a ReadCellGuard to release a RwTicketLock's write lock that doesn't actually own the RwTicketLock"
        );

        // Safety: We check above that the guard actually owns this lock
        unsafe { self.force_unlock() }
    }

    unsafe fn force_unlock(&self) {
        self.access_count.store(0, Ordering::SeqCst);

        // Safety: This will restore the interrupt state from when we called
        // enter_critical_section, so this is safe.
        unsafe {
            I::exit_critical_section(!self.preemtable);
        }
    }

    fn is_unlocked(&self) -> bool {
        self.access_count.load(Ordering::SeqCst) == 0
    }

    fn is_preemtable(&self) -> bool {
        self.preemtable
    }
}

super::lock_cell::unwrap_lock_wrapper! {
    /// A [`UnwrapLock`][super::lock_cell::UnwrapLock] wrapper for [`TicketLock`].
    TicketLock
}
