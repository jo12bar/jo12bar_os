//! Re-export types defined elsewhere for convenience.

use crate::core_locals::CoreInterruptState;

pub use mem_util::sync::lock_cell::{LockCell, LockCellGuard};

/// A [`TicketLock`][mem_util::sync::ticket_lock::TicketLock] setup with the [`CoreInterruptState`].
pub type TicketLock<T> = mem_util::sync::ticket_lock::TicketLock<T, CoreInterruptState>;

/// A [`RwTicketLock`][mem_util::sync::ticket_lock::RwTicketLock] setup with the [`CoreInterruptState`].
pub type RwTicketLock<T> = mem_util::sync::ticket_lock::RwTicketLock<T, CoreInterruptState>;

/// A [`UnwrapTicketLock`][mem_util::sync::ticket_lock::UnwrapTicketLock] setup with the [`CoreInterruptState`].
pub type UnwrapTicketLock<T> = mem_util::sync::ticket_lock::UnwrapTicketLock<T, CoreInterruptState>;
