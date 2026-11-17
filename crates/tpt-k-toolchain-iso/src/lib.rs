// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k boot image assembly: a FAT32 writer for the boot partition.
//!
//! The writer produces the volume that the loader reads: the loader image and
//! the kernel and root images, in 8.3 names. The reader in `tpt-k-boot-fs`
//! checks what it writes, and the tests read every image back through it.
//! Building ISO and GPT wrappers is not done yet.

#![forbid(unsafe_code)]

pub mod fat;

pub use fat::{build, sectors_per_cluster_for, ImageError};
