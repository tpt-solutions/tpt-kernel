# Host timing: endpoint and capability operations

Measured on the development machine (Windows 11, release profile, one million
rounds each):

| Operation | Time |
|---|---|
| Endpoint send with no receiver, then receive (one pair) | 10.8 ns |
| Capability mint, derive, then revoke the root (one round) | 9.6 ns |

Reproduce:

    cargo test --release -p tpt-k-kernel-ipc bench_ -- --ignored --nocapture

## What these numbers are

They time the data-structure operations that a syscall runs, on the host. They
leave out the syscall entry and exit, the page-table switch, the scheduler and
any context switch. So they are not kernel IPC latency, and they say nothing
about bare-metal syscall cost.

The open performance items in `todo.md` still need measurements on the target:
IPC latency including the syscall path, context-switch time, and interrupt to
driver latency. Those need hardware, or at least a timing source the emulator
gives a meaningful answer for.
