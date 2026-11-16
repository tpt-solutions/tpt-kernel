# tpt-k-ipc — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the user-space IPC libraries. Capability tokens, zero-copy serialization,
and the client/server framework that every service uses.

Rules: `#![no_std]`, 100% Rust, no C/C++ bindings.

---

## Phase 2 — User-space IPC libraries

- [ ] `tpt-k-ipc-cap`: capability tokens, rights, unforgeable handles (using `tpt-primitives` CapabilityId/CBOR/SHA-256)
- [ ] `tpt-k-ipc-serde`: zero-copy (de)serialization + checksum (using `tpt-simd`)
- [ ] `tpt-k-ipc`: client/server framework

## Phase 4 — Storage

- [ ] Block-device IPC protocol defined here, used by `tpt-k-drivers` and `tpt-k-userland`

## Phase 6 — AArch64

- [ ] `tpt-simd` NEON paths in `tpt-k-ipc-serde`

## Phase 9 — Hardening

- [ ] Performance benchmarks: IPC latency
- [ ] Fuzz the IPC serde path (feeds Phase 5 in the master plan)
