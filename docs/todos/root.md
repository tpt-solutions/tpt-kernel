# tpt-k-root — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the root server, the first user-space process. It spawns other processes,
generates and revokes capabilities, and makes the hardware reservations and grants.

Rules: `#![no_std]` where it is in the TCB, 100% Rust, no C/C++ bindings.

---

## Phase 2 — The root server

- [ ] `tpt-k-root-spawn`: ELF loader (partly, 2026-11-17: a load planner validates an ELF64 x86-64 image and lists up to 8 PT_LOAD segments and the entry point; it maps nothing yet)
- [ ] `tpt-k-root-spawn`: process lifecycle management (partly, 2026-11-17: a state machine with eight generation-tagged slots, spawn, load, exit, reap and re-parenting to the root; not wired to the kernel and reap does not revoke handles)
  - 2026-11-19: the kernel's `SYS_WAIT` (claim `WAIT-1`) now gives a creator its child's exit code, and frees the child's space. That is in the kernel, not in this state machine, which is still not wired to it.
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
  - 2026-11-19: partly. The demo's root thread revokes its channel handle, with the copies made from it, after its last use, and the smoke test checks that using the revoked handle is refused (the root exits only on that path). Boot self-test `root: a revoked handle is refused`, and claim `ROOT-1` in the assurance graph. Open: the user-space root server (`tpt-k-root`) is still a scaffold, so this is the kernel side of the drop, not a server that starts with less authority than it needs.

## Phase 11a — stub cleanup (2026-11-18)

- [x] `fuzz_tests.rs` renamed to `mutation_tests.rs` in `tpt-k-root-spawn`

## Phase 11c — policy as a manifest (2026-11-18)

- [x] `tpt-k-root-manifest`: compiles a TOML app manifest into grants, refusing any grant above an object's ceiling. Linted by `tpt-k-build lint-manifest`. Property test: claim MAN-1
- [ ] The root server does not read manifests yet, so nothing is granted from them
