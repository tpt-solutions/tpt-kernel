// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k minimal POSIX-style C library core.
//!
//! The crate provides the allocation-free parts a small libc needs: memory and
//! string operations over byte slices, integer parsing, integer formatting into
//! a caller buffer, errno values, and path normalisation. Nothing here allocates
//! or talks to the kernel.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod errno;
pub mod fmt;
pub mod mem;
pub mod num;
pub mod path;
pub mod string;
