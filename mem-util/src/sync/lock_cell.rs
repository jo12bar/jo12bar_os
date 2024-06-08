//! Provides the [`LockCell`] trait, which is a cell type that provides
//! synchronized dynamic mutation using interior mutability and locks.
//!
//! [`LockCell`] gaurds simultaneous access (read _or_ write) to a value, while
//! [`RwLockCell`] allows for either simultaneous read access to a value or a
//! single write access.

use core::{
    fmt::Display,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

/// A trait representing a lock cell that guards simultaneous access to a value.
pub trait LockCell<T>
where
    Self: LockCellInternal<T> + Send + Sync,
{
    /// Get access to the value of this lock. Blocks until access is granted.
    fn lock(&self) -> LockCellGuard<'_, T, Self>;

    /// Attempt to acquire a lock without blocking.
    ///
    /// If the lock could not be acquired at this time, then `None` is returned.
    fn try_lock(&self) -> Option<LockCellGuard<'_, T, Self>>;
}

/// A trait representing a read-write lock that allows for either simultaneous
/// read access to a value or a single write access.
pub trait RwLockCell<T>
where
    Self: LockCell<T> + LockCellInternal<T> + RwCellInternal<T> + Send + Sync,
{
    /// Get read-only access to the value of this lock. Blocks until access is granted.
    fn read(&self) -> ReadCellGuard<'_, T, Self>;

    /// Get mutable access to the value of this lock. Blocks until access is granted.
    ///
    /// The default implementation just calls [`<Self as LockCell<T>>::lock()`][LockCell::lock()].
    fn write(&self) -> LockCellGuard<'_, T, Self> {
        self.lock()
    }
}

/// Unsafe internals used by the [LockCell]s and the [LockCellGuard].
///
/// Normally this shouldn't be used unless if you're implementing a [LockCell].
pub trait LockCellInternal<T> {
    /// Returns a reference to the data behind a mutex.
    ///
    /// # Safety
    /// The current thread must have ownership of the lock.
    unsafe fn get(&self) -> &T;

    /// Returns a mutable reference to the data behind a mutex.
    ///
    /// # Safety
    /// The current thread must have ownership of the lock.
    #[allow(clippy::mut_from_ref)]
    unsafe fn get_mut(&self) -> &mut T;

    /// Unlock the mutex.
    ///
    /// # Safety
    /// This should only be called when the [`LockCellGuard`] corresponding to
    /// this [`LockCell`] is dropped.
    unsafe fn unlock<'s, 'l: 's>(&'s self, guard: &mut LockCellGuard<'l, T, Self>);

    /// Forces the mutex open, without needing access to the guard
    ///
    /// FIXME: yeah this is bullshit. this is just "dropping" a guard without access to it
    ///
    /// # Safety
    /// The caller ensures that there is no active guard.
    /// You probably only want to use this in the global panic handler, when the
    /// entire application is shutting down.
    unsafe fn force_unlock(&self);

    /// Returns `true` if the LockCell is currently unlocked.
    ///
    /// NOTE: The caller can't rely on this fact, since some other
    /// core/interrupt etc could take the lock during or right after this call
    /// finishes.
    fn is_unlocked(&self) -> bool;

    /// Returns `true` if the lock is preemtable.
    ///
    /// In that case the lock is useable within interrupts, but must disable
    /// additional interrupts while being held.
    fn is_preemtable(&self) -> bool;
}

/// A RAII lock guard that takes care of unlocking its associated lock when dropped.
///
/// This allows safe access to the value inside of a [`LockCell`]. When this is
/// dropped, the [`LockCell`] is dropped again.
///
/// This can be obtained from [`LockCell::lock`].
#[derive(Debug)]
pub struct LockCellGuard<'l, T, M>
where
    M: ?Sized + LockCellInternal<T>,
{
    /// The [`LockCell`] that is guarded by `self`.
    pub(super) lockcell: &'l M,
    /// Phantom data for the type `T`.
    pub(super) _phantom: PhantomData<T>,
}

impl<'l, T, M> LockCellGuard<'l, T, M>
where
    M: ?Sized + LockCellInternal<T>,
{
    /// Create a new guard. This should only be called if you're implementing a [`LockCell`].
    ///
    /// # Safety
    /// The caller must ensure that only 1 [`LockCellGuard`] exists for any given
    /// [`LockCell`] at a time.
    pub unsafe fn new(lockcell: &'l M) -> Self {
        LockCellGuard {
            lockcell,
            _phantom: PhantomData,
        }
    }

    /// Allows to execute a simple snippet of code in a expression chain.
    /// Mostly used for debug puropses.
    ///
    /// # Example usage
    /// ```no_run
    /// # let lock = todo!();
    /// lock.lock().also(|_| { info!("lock acuired"); } ).something();
    /// ```
    pub fn also<F: FnOnce(&mut Self)>(mut self, f: F) -> Self {
        f(&mut self);
        self
    }
}

impl<T, M: ?Sized + LockCellInternal<T>> !Sync for LockCellGuard<'_, T, M> {}

impl<'l, T, M> Deref for LockCellGuard<'l, T, M>
where
    M: ?Sized + LockCellInternal<T>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: There will always only be 1 guard for a given mutex, so this is safe.
        unsafe { self.lockcell.get() }
    }
}

impl<'l, T, M> DerefMut for LockCellGuard<'l, T, M>
where
    M: ?Sized + LockCellInternal<T>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: There will always only be 1 guard for a given mutex, so this is safe.
        unsafe { self.lockcell.get_mut() }
    }
}

impl<'l, T, M> AsRef<T> for LockCellGuard<'l, T, M>
where
    M: ?Sized + LockCellInternal<T>,
{
    fn as_ref(&self) -> &T {
        // Safety: There will always only be 1 guard for a given mutex, so this is safe.
        unsafe { self.lockcell.get() }
    }
}

impl<'l, T, M> AsMut<T> for LockCellGuard<'l, T, M>
where
    M: ?Sized + LockCellInternal<T>,
{
    fn as_mut(&mut self) -> &mut T {
        // Safety: There will always only be 1 guard for a given mutex, so this is safe.
        unsafe { self.lockcell.get_mut() }
    }
}

impl<'l, T, M> Display for LockCellGuard<'l, T, M>
where
    T: Display,
    M: ?Sized + LockCellInternal<T>,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T, M: ?Sized + LockCellInternal<T>> Drop for LockCellGuard<'_, T, M> {
    fn drop(&mut self) {
        unsafe { self.lockcell.unlock(self) }
    }
}

/// Unsafe internals used by [`RWLockCell`] and [`ReadCellGuard`].
///
/// Normally this shouldn't be used unless you're implementing [`RWLockCell`]
pub trait RwCellInternal<T>: LockCellInternal<T> {
    /// Releases a read guard of the lock.
    /// If there are no read guards left, the lock is unlocked.
    ///
    /// # Safety
    ///
    /// This should only be called when the [`ReadCellGuard`] corresponding to
    /// this [`RWLockCell`] is dropped.
    unsafe fn release_read<'s, 'l: 's>(&'s self, guard: &mut ReadCellGuard<'l, T, Self>);

    /// Release a [`ReadCellGuard`] without access to the actual guard.
    ///
    /// This is used to implement locks based on other locks, requiring their guards
    /// to return a guard alias. However it is not possible to store the internal
    /// locks guard, so we need a way to release anyways.
    ///
    /// It is valid to implement this using a panic.
    ///
    /// # Safety
    ///
    /// - The caller ensures that the simulated guard is no longer accessible.
    /// - The caller also ensures that this function is only used on implementations
    ///   that support this.
    unsafe fn force_release_read(&self) {}

    /// Returns `true` if the [`RWLockCell`] is currently lockable by reads, meaning
    /// that either no one has a lock or a read has a lock.
    ///
    /// However the caller can't rely on this fact, since some other
    /// core/interrupt etc could take the lock during or right after this call
    /// finishes.
    fn open_to_read(&self) -> bool;
}

/// A guard structure that is used to guard read access to a lock.
///
/// This allows safe "read" access to the value inside of a [`RWLockCell`].
/// When this is dropped, the [`RWLockCell`] will unlock again, if there are no other
/// [`ReadCellGuard`]s for the lock.
///
/// This can be obtained from [`RWLockCell::read`]
#[derive(Debug)]
pub struct ReadCellGuard<'l, T, M: ?Sized + RwCellInternal<T>> {
    pub(super) rw_cell: &'l M,
    pub(super) _phantom: PhantomData<T>,
}

impl<'l, T, M: ?Sized + RwCellInternal<T>> ReadCellGuard<'l, T, M> {
    /// creates a new guard. This should only be called if you implement a [RWLockCell].
    ///
    /// # Safety
    ///
    /// The caller must ensure that only 1 [LockCellGuard] exists for any given
    /// `rw_cell` at a time or multiple [ReadCellGuard]s
    pub unsafe fn new(rw_cell: &'l M) -> Self {
        ReadCellGuard {
            rw_cell,
            _phantom: PhantomData,
        }
    }
}

impl<'l, T, M: ?Sized + RwCellInternal<T>> !Sync for ReadCellGuard<'l, T, M> {}

impl<'l, T, M: ?Sized + RwCellInternal<T>> Deref for ReadCellGuard<'l, T, M> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: While the guard exists there can't be any mut access to the lock
        // and we only give out immutable access
        unsafe { self.rw_cell.get() }
    }
}

impl<'l, T: Display, M: ?Sized + RwCellInternal<T>> Display for ReadCellGuard<'l, T, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

impl<'l, T, M: ?Sized + RwCellInternal<T>> Drop for ReadCellGuard<'l, T, M> {
    fn drop(&mut self) {
        unsafe {
            self.rw_cell.release_read(self);
        }
    }
}

/// A wrapper for a [`LockCell`] of an `MaybeUninit<T>`.
///
/// Unlike a normal [`LockCell`], [`UnwrapLock::lock`] will return `T` or panic
/// if the value was not initialized.
pub struct UnwrapLockCell<T: Send, L: LockCell<MaybeUninit<T>>> {
    /// Inner [`LockCell`] that holds the `MaybeUninit<T>`.
    pub lockcell: L,
    _phantom: PhantomData<T>,
}

/// Creates an [`UnwrapLockCell`] wrapping type for the given [`LockCell`].
macro_rules! unwrap_lock_wrapper {
    (
        $(#[$outer:meta])*
        $lock_type:ident
    ) => {
        paste::paste! {
            $(#[$outer])*
            pub type [<Unwrap $lock_type>]<T, I> = crate::sync::lock_cell::UnwrapLockCell<T, $lock_type<::core::mem::MaybeUninit<T>, I>>;

            impl<T: Send, I: InterruptState> [<Unwrap $lock_type>]<T, I> {
                /// Create a new [`Self`] that is uninitialized.
                ///
                /// # Safety
                /// Caller must ensure that the [`UnwrapLockCell`] is initialized before it is accessed.
                pub const unsafe fn new_uninit() -> Self {
                    unsafe {
                        crate::sync::lock_cell::UnwrapLockCell::new(
                            $lock_type::new(::core::mem::MaybeUninit::uninit())
                        )
                    }
                }

                /// Create a new non-preemtable [`Self`] that is uninitialized.
                ///
                /// # Safety
                /// Caller must ensure that the [`UnwrapLockCell`] is initialized before it is accessed.
                pub const unsafe fn new_non_preemtable_uninit() -> Self {
                    unsafe {
                        crate::sync::lock_cell::UnwrapLockCell::new(
                            $lock_type::new_non_preemtable(::core::mem::MaybeUninit::uninit())
                        )
                    }
                }
            }
        }
    };
}

pub(crate) use unwrap_lock_wrapper;

impl<T: Send, L: LockCell<MaybeUninit<T>>> Drop for UnwrapLockCell<T, L> {
    fn drop(&mut self) {
        unsafe {
            self.lock_uninit().assume_init_drop();
        }
    }
}

impl<T: Send, L: LockCell<MaybeUninit<T>> + Default> Default for UnwrapLockCell<T, L> {
    fn default() -> Self {
        Self {
            lockcell: Default::default(),
            _phantom: Default::default(),
        }
    }
}

unsafe impl<T: Send, L: LockCell<MaybeUninit<T>>> Send for UnwrapLockCell<T, L> {}
unsafe impl<T: Send, L: LockCell<MaybeUninit<T>>> Sync for UnwrapLockCell<T, L> {}

impl<T: Send, L: LockCell<MaybeUninit<T>>> UnwrapLockCell<T, L> {
    /// Creates a new [`UnwrapLockCell`] from the given `inner` [`LockCell`].
    ///
    /// # Safety
    /// The caller must ensure that the lock is initialized with a value __before__
    /// [`UnwrapLockCell::lock`] is called.
    pub const unsafe fn new(inner: L) -> Self {
        Self {
            lockcell: inner,
            _phantom: PhantomData,
        }
    }

    /// Gives access to the locked [`MaybeUninit`]. Blocks until the lock is accessible.
    ///
    /// This is intended for initialization of the [`UnwrapLockCell`].
    pub fn lock_uninit(&self) -> LockCellGuard<'_, MaybeUninit<T>, Self> {
        let inner_guard = self.lockcell.lock();
        core::mem::forget(inner_guard);
        unsafe { LockCellGuard::new(self) }
    }
}

impl<T: Send, L: LockCell<MaybeUninit<T>>> LockCell<T> for UnwrapLockCell<T, L> {
    fn lock(&self) -> LockCellGuard<'_, T, Self> {
        let inner_guard = self.lockcell.lock();
        core::mem::forget(inner_guard);
        unsafe { LockCellGuard::new(self) }
    }

    fn try_lock(&self) -> Option<LockCellGuard<'_, T, Self>> {
        if let Some(inner_guard) = self.lockcell.try_lock() {
            core::mem::forget(inner_guard);
            unsafe { Some(LockCellGuard::new(self)) }
        } else {
            None
        }
    }
}

impl<T: Send, L: LockCell<MaybeUninit<T>>> LockCellInternal<T> for UnwrapLockCell<T, L> {
    unsafe fn get(&self) -> &T {
        unsafe { self.lockcell.get().assume_init_ref() }
    }

    unsafe fn get_mut(&self) -> &mut T {
        unsafe { self.lockcell.get_mut().assume_init_mut() }
    }

    unsafe fn unlock<'s, 'l: 's>(&'s self, guard: &mut LockCellGuard<'l, T, Self>) {
        assert!(
            core::ptr::eq(self, guard.lockcell),
            "attempted to use a LockCellGuard to release a read lock for a UnwrapLockCell that doesn't actually own the UnwrapLockCell"
        );
        unsafe { self.lockcell.force_unlock() }
    }

    unsafe fn force_unlock(&self) {
        unsafe { self.lockcell.force_unlock() }
    }

    fn is_unlocked(&self) -> bool {
        self.lockcell.is_unlocked()
    }

    fn is_preemtable(&self) -> bool {
        self.lockcell.is_preemtable()
    }
}

impl<T: Send, L: LockCell<MaybeUninit<T>>> LockCellInternal<MaybeUninit<T>>
    for UnwrapLockCell<T, L>
{
    unsafe fn get(&self) -> &MaybeUninit<T> {
        unsafe { self.lockcell.get() }
    }

    unsafe fn get_mut(&self) -> &mut MaybeUninit<T> {
        unsafe { self.lockcell.get_mut() }
    }

    unsafe fn unlock<'s, 'l: 's>(&'s self, _guard: &mut LockCellGuard<'l, MaybeUninit<T>, Self>) {
        // TODO: add an assert to make sure the guard owns this lock
        unsafe { self.lockcell.force_unlock() }
    }

    unsafe fn force_unlock(&self) {
        unsafe { self.lockcell.force_unlock() }
    }

    fn is_unlocked(&self) -> bool {
        self.lockcell.is_unlocked()
    }

    fn is_preemtable(&self) -> bool {
        self.lockcell.is_preemtable()
    }
}

impl<T: Send, L: RwLockCell<MaybeUninit<T>>> RwLockCell<T> for UnwrapLockCell<T, L> {
    fn read(&self) -> ReadCellGuard<'_, T, Self> {
        let inner_guard = self.lockcell.read();
        core::mem::forget(inner_guard);
        unsafe { ReadCellGuard::new(self) }
    }
}

impl<T: Send, L: RwLockCell<MaybeUninit<T>>> RwCellInternal<T> for UnwrapLockCell<T, L> {
    unsafe fn release_read<'s, 'l: 's>(&'s self, guard: &mut ReadCellGuard<'l, T, Self>) {
        assert!(
            core::ptr::eq(self, guard.rw_cell),
            "attempted to use a LockCellGuard to release a write lock for a UnwrapLockCell that doesn't actually own the UnwrapLockCell"
        );
        unsafe {
            self.force_release_read();
        }
    }

    fn open_to_read(&self) -> bool {
        self.lockcell.open_to_read()
    }
}
