# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The `unsafe` blocks state their invariants.
- UEFI loader that reads `TPTK\KERNEL.ELF` from the boot volume.
- Places the kernel's `PT_LOAD` segments at their physical addresses, exits boot services and jumps to the entry point.
- The kernel is entered with the 80-byte handoff record in `rdi` (memory map, RSDP, framebuffer). The module docs described a zero argument; they are corrected.

### Fixed

- A kernel segment whose `paddr + mem_size` wrapped was accepted; the load now fails with `kernel segment address wraps`.
