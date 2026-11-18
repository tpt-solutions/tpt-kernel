# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- UEFI loader that reads `TPTK\KERNEL.ELF` from the boot volume. It tries the image's own device first, then every file-system handle, so partitioned disks work.
- Places the kernel's `PT_LOAD` segments at their physical addresses, leaves boot services and jumps to the entry point.
- The kernel is entered with an 80-byte handoff record in `rdi` (`BootInfo`, magic `TPTKBOOT`, version 2). It holds the UEFI memory map (address, byte length and descriptor size), the ACPI 2.0 RSDP address and the GOP framebuffer. An absent RSDP or framebuffer is recorded as zero.

### Fixed

- A kernel segment whose `paddr + mem_size` wrapped was accepted; the load now fails with `kernel segment address wraps`.
