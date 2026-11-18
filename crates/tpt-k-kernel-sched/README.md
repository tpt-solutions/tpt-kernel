# tpt-k-kernel-sched

The tpt-k kernel scheduler and the x86_64 context switch.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

The crate has a cooperative round-robin scheduler over a fixed table of 16 threads, and the x86_64 context switch. A switched-out thread's callee-saved registers sit on its own stack, and the stack pointer is the only state kept in the thread record.

A priority core is also present in `priority`: 32 levels with round-robin inside each level, and time slices counted in timer ticks (2 to 16 ticks, shorter for higher priorities). It is pure bookkeeping over slot numbers and is not yet wired to the context switch. It has no priority inheritance.

The round-robin scheduler does not preempt: a thread runs until it yields. The kernel binary's timer handler calls `Scheduler::yield_now` on each tick, which makes its demo preemptive. The scheduler is single-core and is not re-entered. Callers must not run `Scheduler::yield_now` from two places at once.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
