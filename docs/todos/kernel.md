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
- [x] `tpt-k-kernel-mem`: physical page frame allocator (2026-11-16: bitmap over caller-supplied storage, 7 host tests; not yet wired to the boot memory map)
- [ ] `tpt-k-kernel-mem`: virtual memory management, per-process address spaces (partly, 2026-11-17: page tables with 4 KiB mapping and 2 MiB splitting, and address spaces made by forking the boot space; two user processes map the same virtual address to different frames and read back their own values in the QEMU run; the tables are handed out by the frame allocator (over a static pool; a boot memory map is still open), and the process handle tables are open) Page tables are allocated through `FrameAllocator` (2026-11-17), over a static pool; frames come from the loader's memory map (2026-11-17), and the free count is reported at boot.
- [ ] `tpt-k-kernel-mem`: page clearing via `tpt-simd` (scalar fallback)
- [ ] `tpt-k-kernel-arch-x86_64`: APIC/timer, SYSCALL/SYSRET entry (partly, 2026-11-17: the local APIC timer drives preemption at 100 Hz, calibrated against the PIT, with the APIC page mapped uncached and the PIC fully masked; SYSCALL/SYSRET enter and leave the kernel. 2026-11-17: the IO-APIC routes the RTC (GSI 8) to vector 40 on the bootstrap CPU, and the QEMU smoke test sees two interrupts. Still open: delivering a device interrupt to a user-space driver, and SMP bring-up)
- [x] `tpt-k-kernel-sched`: thread structures, context switching (2026-11-16: callee-saved switch on each thread's stack; QEMU smoke test shows two threads interleaving)
- [ ] `tpt-k-kernel-sched`: preemptive priority-based scheduler (preemptive round-robin works, timer-driven; priorities are not done)
- [ ] `tpt-k-kernel-ipc`: syscall entry/exit and dispatch (partly, 2026-11-17: `SYS_SPAWN_IMAGE` spawns a child from a program image in the caller's user pages, mapped only in the child's space; one page, raw code, two slots; ring-3 threads call `write`, `exit`, `send` and `recv` through `syscall`; the entry keeps the user stack pointer on each thread's kernel stack, so a syscall can block while other threads run; capability handles cross the boundary as raw words and are checked by the table; the root server and per-process handle tables are open)
- [ ] `tpt-k-kernel-ipc`: capability table + capability translation (partly, 2026-11-17: a table per scheduler slot, so a handle is valid only in its owner's table; syscalls translate through the caller's table; the boot-time check refuses a handle from one thread in another's table; handle transfer works through a grant syscall (`SYS_GRANT`: the source needs GRANT, the rights may only narrow, and the copy lands in the target's table; the boot check runs it through the dispatcher); a granted copy records its origin, and `SYS_REVOKE` with `revoke_across` revokes the copies made from a capability in every table, transitively; host-tested and checked at boot)
- [ ] `tpt-k-kernel-ipc`: zero-copy, capability-routed message passing (endpoints) (partly, 2026-11-17: a memory capability is granted to another thread, which maps the same physical page with `SYS_MAP`; the sender's write is seen by the receiver, and the kernel copies no page contents. 2026-11-17: memory objects are general now, with 1 to 4 pages from a run-time frame allocator, mapped in order at a page-aligned address in the memory window (`SYS_MEM_CREATE`, `SYS_MAP`). 2026-11-17: a capability can travel with a message (`SYS_SEND_CAP`, `SYS_RECV_CAP`), so the receiver maps a memory object it was sent. 2026-11-17: `SYS_SEND_BULK` carries a payload of up to four pages, copied at send time into a memory object the receiver gets as a capability. Still open: freeing objects, and a receive that copies the payload out; the 4-word messages are still copied)

### Verification
- [x] Tests: IPC round-trip and context switching (2026-11-16: the QEMU smoke test runs a request/reply and a capability refusal, and checks the worker sequence; capability revocation is host-tested in `tpt-k-kernel-ipc`, the root-server tests come with `tpt-k-root`)
- [ ] Milestone M2 (kernel side)

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
- [ ] Performance benchmarks: context switch, interrupt-to-driver latency
- [ ] Security review of the syscall interface and capability code
- [ ] Review all `unsafe` blocks; document invariants

## CI

- [ ] Verify all kernel-side crates are `#![no_std]` and CI-checked for it
- [ ] CI: QEMU boot smoke test job
- [ ] CI: reusable workflow shared across the repos
