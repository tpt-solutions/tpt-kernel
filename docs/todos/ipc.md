# tpt-k-ipc — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the user-space IPC libraries. Capability tokens, zero-copy serialization,
and the client/server framework that every service uses.

Rules: `#![no_std]`, 100% Rust, no C/C++ bindings.

---

## Phase 2 — User-space IPC libraries

- [x] `tpt-k-ipc-cap`: capability tokens, rights, unforgeable handles (2026-11-17: HMAC-SHA256 tags under a server key, using the SHA-256 of `tpt-primitives`, checked against the RFC 4231 vector; verified attenuation. The `CapabilityId` and CBOR types are not used yet)
- [x] `tpt-k-ipc-serde`: zero-copy (de)serialization + checksum (2026-11-17: framed words and payload, read in place, with CRC-32C checked against the standard value. The checksum is scalar; `tpt-simd` is not used yet)
- [x] `tpt-k-ipc`: client/server framework (2026-11-17: capability-carrying requests and status replies; `serve` checks the token and rights before the service runs. There is no transport yet)

## Phase 4 — Storage

- [x] Block-device IPC protocol defined here, used by `tpt-k-drivers` and `tpt-k-userland` (2026-11-17: read, write, flush and get-info frames in the `block` module, with `serve_block` checking bounds before the device runs; not yet used by drivers or userland)

## Phase 6 — AArch64

- [ ] `tpt-simd` NEON paths in `tpt-k-ipc-serde`

## Phase 9 — Hardening

- [ ] Performance benchmarks: IPC latency
- [ ] Fuzz the IPC serde path (feeds Phase 5 in the master plan) (partly, 2026-11-17: deterministic mutation tests on the frame decoder; not coverage-guided fuzzing)
  - 2026-11-18: the endpoint and capability-token invariants that IPC relies on are checked in `tpt-k-kernel-ipc` and `tpt-k-ipc-cap` (see `docs/assurance/invariants.md`). Token and service claims are waived as sampled-only. No coverage-guided campaign has been run.

## Phase 11a — stub cleanup (2026-11-18)

- [x] `fuzz_tests.rs` renamed to `mutation_tests.rs` in `tpt-k-ipc-serde`. Real cargo-fuzz stays open
