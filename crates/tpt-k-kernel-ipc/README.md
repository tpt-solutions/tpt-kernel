# tpt-k-kernel-ipc

Kernel-side IPC for tpt-k: the capability table and synchronous endpoints.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 2, implemented:

- `cap.rs`: the capability table. A capability names a kernel object and the rights its holder has on it. Handles carry a generation, so a stale handle fails after its slot is reused. Derivation can only remove rights, and revoking a capability revokes every capability derived from it.
- `endpoint.rs`: synchronous endpoints. A message moves when a sender and a receiver are both present. Queues are bounded, so endpoints need no heap. The module only decides the outcome, and the kernel blocks and wakes threads accordingly.

Syscall entry and the zero-copy path are not done yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

