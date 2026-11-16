// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k boot-time image readers. Phase 1 has the ELF64 parser only. The
//! FAT32 driver is not needed yet: the loader reads the EFI System Partition
//! through the UEFI file protocol.

#![no_std]

pub mod elf;
