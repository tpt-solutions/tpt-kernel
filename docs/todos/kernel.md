# tpt-k-kernel — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the microkernel. Memory, scheduling, IPC dispatch, the capability table,
interrupt routing, and the architecture code for x86_64 and later AArch64.

Rules: `#![no_std]`, 100% Rust, no C/C++ bindings. Every `unsafe` block carries a
documented invariant.

---

## Phase 1 — Boot & Hello World

- [x] `tpt-k-kernel`: early entry, stack setup, panic handler (2026-11-16)
- [x] `tpt-k-kernel-arch-x86_64`: GDT, IDT, basic exception handlers (2026-11-16: flat ring-0 GDT with CS reload; IDT for vectors 0-31, breakpoint returns, other exceptions print and stop; checked by the QEMU smoke test)
- [x] Serial (COM1) console output (2026-11-16, polled 16550 in `tpt-k-kernel-arch-x86_64`)
- [ ] VGA/framebuffer text output (optional)
- [x] Identity paging setup (x86_64) (2026-11-16: the kernel builds its own 4-level tables, a 4 GiB identity map with 2 MiB pages, and the QEMU run checks a 4 KiB remap)
- [x] Print "Hello tpt-k" (2026-11-16)

### Verification
- [x] Automated QEMU test asserts "Hello tpt-k" on serial (2026-11-16, `tpt-kernel/scripts/qemu-smoke.sh`, CI job `qemu-smoke.yml`)
- [x] Milestone M1 (kernel side)

## Phase 2 — IPC & the root server

### Kernel core
- [x] `tpt-k-kernel-mem`: physical page frame allocator (2026-11-16: bitmap over caller-supplied storage, 7 host tests; wired to the loader's memory map on 2026-11-17 through `bootmap::allocator_from_map`)
- [ ] `tpt-k-kernel-mem`: virtual memory management, per-process address spaces (partly, 2026-11-17: page tables with 4 KiB mapping and 2 MiB splitting, and address spaces made by forking the boot space; two user processes map the same virtual address to different frames and read back their own values in the QEMU run; the tables are handed out by the frame allocator, and the process handle tables are open) Page tables are allocated through `FrameAllocator` (2026-11-17) from a static pool, which is fixed in size; frames come from the loader's memory map (2026-11-17), and the free count is reported at boot.
- [ ] `tpt-k-kernel-mem`: page clearing via `tpt-simd` (scalar fallback) (partly, 2026-11-17: `clear_frame` zeroes a frame with a scalar fill and refuses other lengths; the `tpt-simd` path is deferred)
  - 2026-11-19: the top-level `todo.md` closes the kernel-wide `tpt-simd` optimisation item as not applicable. This item is left open here, because the decision is not recorded in this crate and `tpt-k-kernel-mem` has no `tpt-simd` dependency. Needs a decision: close it the same way, or add the dependency.
- [ ] `tpt-k-kernel-arch-x86_64`: APIC/timer, SYSCALL/SYSRET entry (partly, 2026-11-17: the local APIC timer drives preemption at 100 Hz, calibrated against the PIT, with the APIC page mapped uncached and the PIC fully masked; SYSCALL/SYSRET enter and leave the kernel. 2026-11-17: the IO-APIC routes the RTC (GSI 8) to vector 40 on the bootstrap CPU, and the QEMU smoke test sees two interrupts. Still open: delivering a device interrupt to a user-space driver, and SMP bring-up)
- [x] `tpt-k-kernel-sched`: thread structures, context switching (2026-11-16: callee-saved switch on each thread's stack; QEMU smoke test shows two threads interleaving)
- [ ] `tpt-k-kernel-sched`: preemptive priority-based scheduler (preemptive round-robin works, timer-driven; partly, 2026-11-17: a 32-level priority run queue with time slices and block/unblock is host-tested in `priority.rs`, but it is not wired to the context switch and has no priority inheritance)
- [ ] `tpt-k-kernel-ipc`: syscall entry/exit and dispatch (partly, 2026-11-17: `SYS_SPAWN_IMAGE` spawns a child from a program image in the caller's user pages, mapped only in the child's space; one page, raw code, two slots; ring-3 threads call `write`, `exit`, `send` and `recv` through `syscall`; the entry keeps the user stack pointer on each thread's kernel stack, so a syscall can block while other threads run; capability handles cross the boundary as raw words and are checked by the table; the root server and per-process handle tables are open)
- [ ] `tpt-k-kernel-ipc`: capability table + capability translation (partly, 2026-11-17: a table per scheduler slot, so a handle is valid only in its owner's table; syscalls translate through the caller's table; the boot-time check refuses a handle from one thread in another's table; handle transfer works through a grant syscall (`SYS_GRANT`: the source needs GRANT, the rights may only narrow, and the copy lands in the target's table; the boot check runs it through the dispatcher); a granted copy records its origin, and `SYS_REVOKE` with `revoke_across` revokes the copies made from a capability in every table, transitively; host-tested and checked at boot)
- [ ] `tpt-k-kernel-ipc`: zero-copy, capability-routed message passing (endpoints) (partly, 2026-11-17: a memory capability is granted to another thread, which maps the same physical page with `SYS_MAP`; the sender's write is seen by the receiver, and the kernel copies no page contents. 2026-11-17: memory objects are general now, with 1 to 4 pages from a run-time frame allocator, mapped in order at a page-aligned address in the memory window (`SYS_MEM_CREATE`, `SYS_MAP`). 2026-11-17: a capability can travel with a message (`SYS_SEND_CAP`, `SYS_RECV_CAP`), so the receiver maps a memory object it was sent. 2026-11-17: `SYS_SEND_BULK` carries a payload of up to four pages, copied at send time into a memory object the receiver gets as a capability. Still open: freeing objects, and a receive that copies the payload out; the 4-word messages are still copied)

### Verification
- [x] Tests: IPC round-trip and context switching (2026-11-16: the QEMU smoke test runs a request/reply and a capability refusal, and checks the worker sequence; capability revocation is host-tested in `tpt-k-kernel-ipc`, the root-server tests come with `tpt-k-root`)
- [ ] Milestone M2 (kernel side)
- [x] Bounded exhaustive checks of the capability table and of endpoints (2026-11-18: `CAP-1` to `CAP-8` and `IPC-1` to `IPC-6` in `tpt-k-kernel-ipc`, listed in `docs/assurance/graph.json` and described in `docs/assurance/invariants.md`). They run in `cargo test`, in `scripts/check.sh`, and in the CI `tcb-nostd` group (host tests, added 2026-11-18).

## Phase 3 — Hardware & interrupts

- [ ] Interrupt routing: map vector → user-space driver notification channel and yield (partly, 2026-11-17: the RTC line reaches a user-space driver thread. The driver holds an interrupt object, and `SYS_IRQ_WAIT` blocks it until the handler records an interrupt and wakes it. Still open: a general interrupt object per GSI, and drivers started from a spawn rather than by the bootstrap)
- [ ] IOMMU page table management in `tpt-k-kernel-mem`
- [ ] Capabilities for MMIO ranges, IRQ lines, DMA memory

### Verification
- [ ] Milestone M3 (kernel side): driver crashes do not panic the kernel

## Phase 6 — AArch64

- [ ] `tpt-k-kernel-arch-aarch64`: exception levels (EL1), vectors, context switch
- [ ] MMU / page table setup
- [ ] GIC interrupt controller + timer
- [ ] `tpt-simd` NEON paths in `tpt-k-kernel-mem` (page clearing)
- [ ] QEMU `virt` machine test harness + CI job
- [ ] Parity: Phases 1–4 milestones pass on AArch64

## Phase 9 — Hardening

- [ ] Root server drops supreme privileges after bootstrap (kernel side of the test)
  - 2026-11-19: partly. The demo's root thread revokes its channel handle, with the copies made from it, after its last use, and the smoke test checks that using the revoked handle is refused (the root exits only on that path). Boot self-test `root: a revoked handle is refused`, and claim `ROOT-1` in the assurance graph. Open: the user-space root server (`tpt-k-root`) is still a scaffold, so this is the kernel side of the drop, not a server that starts with less authority than it needs.
- [ ] Performance benchmarks: context switch, interrupt-to-driver latency
  - 2026-11-19: host timing of endpoint and capability operations is in `docs/perf/host-ipc.md` (`bench_tests.rs` in `tpt-k-kernel-ipc`). It excludes the syscall entry, the page-table switch and context switches. Context switch and interrupt-to-driver latency are still open
- [x] Security review of the syscall interface and capability code
  - 2026-11-19: read-only review, 12 findings. Fixed: spawn refused after its thread existed halted the kernel; children inherited window mappings; DF not cleared on syscall entry. Open: `SYS_RECV`/`SYS_RECV_CAP` buffer check after dequeue; refused blocking send/receive leave the endpoint changed (IPC-1, IPC-4); child spaces share the root's user stack; `send_bulk` dead origin; `grant` without target consent; `SYS_MAP` over another object's address. Full list in the top-level `todo.md`. 2026-11-19, later: fixed receive buffer check, blocking-send/receive refusal before state changes, bulk temporary deleted without revoking, `SYS_MAP` address-in-use refusal, register scrub on return, and fresh stacks for spawned children. 2026-11-19, decided: grant consent is built (a thread may grant only to threads it created), and every space except the boot space now has its own user stack. Found and fixed: the frame allocator could hand out frames below 1 MiB
- [x] Review all `unsafe` blocks; document invariants
  - 2026-11-19: done for every crate and for `main.rs` (91 sites). Fixed: loader segment-end overflow; unaligned user word accesses now refused (`within_words`). Open, and not fixed: the spawn paths in `main.rs` (near lines 2678 and 2768) still hold a `&mut` to `KERNEL_TABLES` across `give_fresh_user_stack`. Their own comments say "NOT SOUND AS WRITTEN". The scheduler's `yield_now` now takes a raw pointer, but the wider `&mut` borrows across context switches still need restructuring

## CI

- [ ] Verify all kernel-side crates are `#![no_std]` and CI-checked for it
  - 2026-11-19: open. The six kernel crates are `#![no_std]` (checked 2026-11-16). CI checks `nostd_crates` in `.github/workflows/ci.yml`, which has the `-mem`, `-sched`, `-ipc` and `-arch-x86_64` crates but not the `tpt-k-kernel` binary crate. The `kernel-image` job runs `scripts/build-kernel.sh` and does not run the check.
- [x] CI: QEMU boot smoke test job
  - 2026-11-16: `.github/workflows/qemu-smoke.yml` exists, and runs on push. Not yet run on GitHub. The local `scripts/qemu-smoke.sh` passes.
- [ ] CI: reusable workflow shared across the repos
  - 2026-11-19: open. `.github/workflows/reusable-ci.yml` is in this repo only, and no sibling repo calls it yet.

## Phase 11a — stub cleanup (2026-11-18)

- [x] Docs: the page-table bullet (line 31) no longer contradicts itself. Page tables come from a fixed static pool, and frames come from the loader's memory map
- [x] `tpt-k-kernel-arch-aarch64` removed from the CI `tcb-nostd` group until it has code
- [x] Ring-3 faults kill only the faulting thread, with a smoke case (2026-11-18: `user: fault vector 14`; the kernel runs on)
- [x] Real `SYS_EXIT` teardown (2026-11-18: an exiting thread deletes its handles without revoking, seL4-style, and the derivation tree is re-linked (`cap::delete_across`). Unreferenced memory objects are reclaimed, which returns their frames. 2026-11-19: the parent is notified. `SYS_WAIT` (`wait_child` in `main.rs`) returns the child's exit code, or `Fault`, to its creator (claim `WAIT-1`; a boot check waits on a faulted child, `main.rs` near line 2027). It then releases the child's space (`release_child_space`, claim `MEM-2`, commits `71a8245` and `69892e4`). A child that is never waited on keeps its space, which is bounded by the two spawn slots)
- [x] `SYS_MEM_FREE` and freeing on revoke (2026-11-18: `SYS_MEM_FREE` frees an unmapped object, revokes its handles in every table, and returns its frames. Freeing on the last revoke or delete, and reclaim at thread exit, are in. 2026-11-19: `SYS_UNMAP` removes a mapping made by `SYS_MAP`, with up to 4 recorded mappings per object (boot self-test `mem: unmapped object was freed`). Reclaim removes an object's mappings when no handle is left (claim `MEM-1`, boot self-test `mem: revoking a mapped object removed its mapping`). An exited child's page tables are released when its creator collects the exit (claim `MEM-2`))
- [x] Distinct negative error codes instead of `u64::MAX` (2026-11-18: negated errno, checked by a boot self-test; receive buffers keep `u64::MAX` as "no capability")
