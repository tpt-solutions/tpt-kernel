# tpt-k-kernel-arch-x86_64

The tpt-k kernel x86_64 primitives. This crate covers the descriptor tables, interrupt controllers, timers, paging, and the syscall entry.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

The modules are:

- `lib.rs`: port I/O, the COM1 serial console, and the QEMU debug-exit device used by the test harness.
- `gdt.rs`, `tss.rs`: flat 64-bit ring-0 and ring-3 segments and the task state segment.
- `idt.rs`: exception and IRQ entries (vectors 0 to 47). Exceptions raised in ring 3 go to a user fault handler when one is set.
- `pic.rs`, `pit.rs`: the legacy 8259 PICs, and the 8254 PIT, which calibrates the APIC timer.
- `interrupts.rs`: nestable interrupt-state critical sections.
- `apic.rs`, `ioapic.rs`: the local APIC timer and the IO-APIC routing.
- `rtc.rs`: the CMOS real-time clock's periodic interrupt.
- `paging.rs`: 4-level paging, with a 2 MiB identity map, 4 KiB mapping and unmapping, and copying and releasing of address spaces.
- `syscall.rs`: `SYSCALL`/`SYSRET` entry and transfer to user mode.

Every `unsafe` block states the invariant it relies on.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
