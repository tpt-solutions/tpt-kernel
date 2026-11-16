# tpt-k-root — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the root server, the first user-space process. It spawns other processes,
generates and revokes capabilities, and makes the hardware reservations and grants.

Rules: `#![no_std]` where it is in the TCB, 100% Rust, no C/C++ bindings.

---

## Phase 2 — The root server

- [ ] `tpt-k-root-spawn`: ELF loader
- [ ] `tpt-k-root-spawn`: process lifecycle management
- [ ] `tpt-k-root-caps`: capability generation, delegation, revocation
- [ ] `tpt-k-root`: main root server binary, first user-space process
- [ ] Kernel spawns root server and hands it initial capabilities
- [ ] Integrate `tpt-fabric` model: Intent → Reservation → Execution → Trace
- [ ] Root server passes a simple capability token via IPC, verified with `tpt-primitives`

### Verification
- [ ] Tests: capability denial and revocation
- [ ] Milestone M2 (root side)

## Phase 3 — Hardware role

- [ ] Parse hardware topology via `tpt-k-hal`
- [ ] Evaluate hardware Intents, make IOMMU/PCIe Reservations, grant Execution capabilities to drivers
- [ ] Trace all grants via `tpt-primitives`

### Verification
- [ ] Milestone M3 (root side)

## Phase 9 — Hardening

- [ ] Root server drops supreme privileges after bootstrap (verified by test + Assurance Graph)
