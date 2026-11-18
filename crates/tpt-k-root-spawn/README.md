# tpt-k-root-spawn

The tpt-k root spawn: an ELF64 load planner and a process lifecycle table. It maps nothing, and the lifecycle is not wired to the kernel yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: ELF64 x86-64 load planner: checks the header and program headers, and returns up to eight `PT_LOAD` segments (address, file range, memory size, R/W/X) and the entry point. Refuses writable+executable, overlapping, misaligned or out-of-image segments, and an entry outside an executable segment. It maps nothing; the caller does that from the plan. Deterministic mutation tests cover the ELF planner.

A fixed table of eight process slots (`lifecycle` module) tracks Free, Loading, Running, Exited and Reclaimed states. Slots carry generations, so a stale id is refused after reuse. Exiting a process re-parents its children to the root. This state machine is host-tested and is not yet wired to the kernel.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

