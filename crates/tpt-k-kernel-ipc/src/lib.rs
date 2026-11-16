// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k kernel IPC: kernel-side routing, syscall entry/exit, capability translation.
//!
//! Phase 2 has the capability table and synchronous endpoints. Syscall entry
//! and the zero-copy path come later.

#![no_std]

pub mod cap;
pub mod endpoint;

pub use cap::{CapError, CapId, CapTable, ObjectId, Rights};
