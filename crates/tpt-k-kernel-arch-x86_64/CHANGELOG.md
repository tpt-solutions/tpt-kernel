# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Port I/O, COM1 serial console, and QEMU debug-exit device.
- Flat GDT, TSS, and IDT with CPU exception handlers and remapped IRQ vectors.
- Legacy PIC and PIT timer, and nestable interrupt-state sections.
- Local APIC timer (calibrated against the PIT) and IO-APIC routing.
- CMOS RTC periodic interrupt, used to test the IO-APIC route and by the kernel's driver demo.
- 4-level paging with a 2 MiB identity map, 4 KiB remapping, and `PageTables::fork` to copy an address space.
- `PageTables::unmap_4k` and `PagingError::NotMapped`: remove one 4 KiB mapping and return its frame. Huge mappings give `Unsupported`.
- `PageTables::release_root`: frees the tables of a forked address space, but not the leaf frames, which their owner frees. The boot space and the active space are refused.
- `set_user_fault_handler`: exceptions raised in ring 3 (CS RPL 3) go to the handler, which must not return. Ring-0 exceptions still print and stop the VM.
- `SYSCALL`/`SYSRET` entry and transfer into user mode.

### Fixed

- The syscall entry clears the direction flag (`FMASK` now holds IF and DF), and clears the argument registers before returning to ring 3.
- The syscall entry preserves the caller's callee-saved registers (rbx, rbp, r12 to r15), since a handler may block and another thread then runs.
