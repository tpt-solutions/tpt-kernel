# tpt-k-kernel-sched

The tpt-k kernel scheduler and the x86_64 context switch.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 2, implemented: a cooperative round-robin scheduler over a fixed thread table, and the x86_64 context switch. A switched-out thread's callee-saved registers sit on its own stack, and the stack pointer is the only state kept in the thread record.

The scheduler itself does not preempt: a thread runs until it yields. The kernel binary's demo also uses timer preemption on top of it. The scheduler is single-core and is not re-entered. Callers must not run `Scheduler::yield_now` from two places at once.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

