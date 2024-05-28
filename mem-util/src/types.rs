//! Common type definitions.
//!
//! Could probably be its own crate, but it works fine-enough here.

use core::{
    fmt,
    ops::{Deref, DerefMut},
};

/// Contains the ID for a given CPU core.
///
/// Largely based off of the implementation in [Wasabi375/WasabiOS](https://github.com/Wasabi375/WasabiOS/blob/f4520ca20b0e0a2595f5c218701c32e01551820f/shared/src/lib.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CoreId(pub u8);

impl CoreId {
    /// Whether this core is used as the bootstrap processor used for
    /// initialization of global systems.
    pub const fn is_bsp(&self) -> bool {
        self.0 == 0
    }
}

impl Deref for CoreId {
    type Target = u8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CoreId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<u8> for CoreId {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<CoreId> for u8 {
    fn from(val: CoreId) -> Self {
        val.0
    }
}

impl fmt::Display for CoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
