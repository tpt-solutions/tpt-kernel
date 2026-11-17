# tpt-k-kernel

The tpt-k microkernel binary. It covers early boot entry, paging, the syscall boundary, address spaces, and the panic handler. It also runs the Phase 2 demo.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 2, implemented. The demo runs these threads under the scheduler:

- Two workers that print tagged letters and spin between prints, so the PIT tick preempts them.
- A kernel server that receives one request and replies.
- A ring-3 user thread. It writes a message, checks that a receive-only handle is refused, sends a request and receives the reply through capability handles, then exits with the reply's first word.
- Two ring-3 processes, each in its own address space, that check isolation by reading back their own value from a shared virtual address.

The kernel is single-core. Kernel state is touched only inside `interrupts::without` sections or during a syscall, which runs with interrupts off.

## Build

The kernel is bare-metal and has no host runtime. Use the build script, which passes the linker script (`linker.ld`) to the kernel link only:

```sh
scripts/build-kernel.sh
```

The equivalent workspace build target is `x86_64-unknown-none`.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

