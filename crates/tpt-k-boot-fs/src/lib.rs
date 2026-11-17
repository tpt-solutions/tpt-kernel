// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k boot-time image readers: the ELF64 parser for the kernel image, and a
//! read-only FAT32 driver for loading images from a volume without UEFI.
//! The loader still reads the EFI System Partition through the UEFI file
//! protocol; the FAT32 driver is the fallback and the host-tested reference.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod elf;
pub mod fat32;
