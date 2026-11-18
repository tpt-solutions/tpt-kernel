# tpt-k-kernel-ipc

Kernel-side IPC for tpt-k: the capability table and synchronous endpoints.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

- `cap.rs`: the capability table. A capability names a kernel object and the rights its holder has on it. Handles carry a generation, so a stale handle fails after its slot is reused. Derivation can only remove rights, and revoking a capability revokes every capability derived from it.
- `endpoint.rs`: synchronous endpoints. A message moves when a sender and a receiver are both present, and it can carry one capability. Queues are bounded, so endpoints need no heap. The module only decides the outcome, and the kernel blocks and wakes threads accordingly.

The syscall entry is not in this crate: it lives in the kernel binary and `tpt-k-kernel-arch-x86_64`. Bulk messages are copied by the kernel into a new memory object, so a zero-copy path is not done.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
