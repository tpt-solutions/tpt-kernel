# tpt-k-root — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the root server, the first user-space process. It spawns other processes,
generates and revokes capabilities, and makes the hardware reservations and grants.

Rules: `#![no_std]` where it is in the TCB, 100% Rust, no C/C++ bindings.

---

## Phase 2 — The root server

- [ ] `tpt-k-root-spawn`: ELF loader (partly, 2026-11-17: a load planner validates an ELF64 x86-64 image and lists up to 8 PT_LOAD segments and the entry point; it maps nothing yet)
- [ ] `tpt-k-root-spawn`: process lifecycle management
- [x] `tpt-k-root-caps`: capability generation, delegation, revocation (2026-11-17: fixed-capacity table with generation counters; delegation only narrows and needs GRANT; revoke needs REVOKE and cascades to descendants; host-tested)
- [ ] `tpt-k-root`: main root server binary, first user-space process (partly, 2026-11-17: a capability service over the IPC framework, with MINT, DELEGATE, REVOKE and ATTENUATE gated by rights; `main.rs` is still a placeholder and the server is not the kernel's first process)
- [ ] Kernel spawns root server and hands it initial capabilities
- [ ] Integrate `tpt-fabric` model: Intent → Reservation → Execution → Trace
- [ ] Root server passes a simple capability token via IPC, verified with `tpt-primitives` (partly, 2026-11-17: tokens are issued and verified over the IPC framework with the primitives' SHA-256; no boot-time handoff yet)

### Verification
- [ ] Tests: capability denial and revocation (partly, 2026-11-17: host tests cover denial and revocation in the table and in the service; the kernel-side test comes with M2)
- [ ] Milestone M2 (root side)

## Phase 3 — Hardware role

- [ ] Parse hardware topology via `tpt-k-hal`
- [ ] Evaluate hardware Intents, make IOMMU/PCIe Reservations, grant Execution capabilities to drivers
- [ ] Trace all grants via `tpt-primitives`

### Verification
- [ ] Milestone M3 (root side)

## Phase 9 — Hardening

- [ ] Root server drops supreme privileges after bootstrap (verified by test + Assurance Graph)
