//! # `mem_util` - Utilities for working with kernel memory.

#![no_std]
#![feature(negative_impls)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod sync;
pub mod types;

/// Converts its argument from kibibytes to bytes.
#[macro_export]
macro_rules! KiB {
    ($v:expr) => {
        $v * 1024
    };
}

/// Converts its argument from mebibytes to bytes.
#[macro_export]
macro_rules! MiB {
    ($v:expr) => {
        $v * 1024 * 1024
    };
}

/// Converts its argument from gibibytes to bytes.
#[macro_export]
macro_rules! GiB {
    ($v:expr) => {
        $v * 1024 * 1024 * 1024
    };
}
