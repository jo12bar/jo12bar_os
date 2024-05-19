//! # `jo12bar-os-kernel` -- The kernel component of jo12bar_os.

#![no_std]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod framebuffer;
pub mod gdt;
pub mod interrupts;
