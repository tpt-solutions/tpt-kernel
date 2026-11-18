# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `PageTables::unmap_4k` and `PagingError::NotMapped`: remove one 4 KiB mapping and return its frame. Huge mappings give `Unsupported`.
- `set_user_fault_handler`: exceptions raised in ring 3 (CS RPL 3) go to the handler, which must not return. Ring-0 exceptions still print and stop the VM.

- Port I/O, COM1 serial console, and QEMU debug-exit device.
- Flat GDT, TSS, and IDT with CPU exception handlers and remapped IRQ vectors.
- Legacy PIC and PIT timer, and nestable interrupt-state sections.
- Local APIC timer (calibrated against the PIT) and IO-APIC routing.
- CMOS RTC periodic interrupt for testing the IO-APIC route.
- 4-level paging with a 2 MiB identity map and 4 KiB remapping.
- `SYSCALL`/`SYSRET` entry and transfer into user mode.

### Fixed

- The syscall entry clears the direction flag (`FMASK` now holds IF and DF).
