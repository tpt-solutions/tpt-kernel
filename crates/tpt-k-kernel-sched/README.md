# tpt-k-kernel-sched

The tpt-k kernel scheduler and the x86_64 context switch.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 2, implemented: a cooperative round-robin scheduler over a fixed thread table, and the x86_64 context switch. A switched-out thread's callee-saved registers sit on its own stack, and the stack pointer is the only state kept in the thread record.

A preemptive priority core is also present in `priority`: 32 levels with round-robin inside each level, and time slices counted in timer ticks (2 to 16 ticks, shorter for higher priorities). It is pure bookkeeping over slot numbers and is not yet wired to the context switch. It has no priority inheritance.

The round-robin scheduler above does not preempt: a thread runs until it yields. The kernel binary's demo also uses timer preemption on top of it. The scheduler is single-core and is not re-entered. Callers must not run `Scheduler::yield_now` from two places at once.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

