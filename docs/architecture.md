# tpt-k architecture

tpt-k is a capability-based microkernel. The kernel does only what must run
in privileged mode: memory, scheduling, IPC, capability tables and interrupt
routing. Everything else runs in user space and gets its authority from
capabilities. The full design is in [spec.txt](../spec.txt). This page is a map
of what exists in this repo and how the pieces connect.

Status: the kernel boots in QEMU and runs its smoke test. Many user-space parts
are host-tested libraries. The root server binary is a stub: its capability
service is host-tested, but it has no kernel handoff yet. Drivers and the
networking daemon are not running yet. [todo.md](../todo.md) is the authoritative list of what is
open.

## Layers

The repo is one Cargo workspace. Each crate is named `tpt-k-<layer>[-<part>]`.

| Layer | Role | Runs in |
|---|---|---|
| `boot` | UEFI loader; builds the handoff record (memory map, RSDP, framebuffer) | UEFI firmware |
| `kernel` | Microkernel core: frames, page tables, scheduler, IPC, capability tables, x86_64 arch code | ring 0 |
| `root` | Planned first user process. Creates, grants and revokes capabilities for others. Host-tested library; not running | user space |
| `hal` | Parsers for ACPI, FDT, PCI and USB descriptors | user space (host-tested) |
| `ipc` | Client and server framework, capability tokens, block protocol | user space libraries |
| `drivers` | Driver host pieces: restart policy, NVMe, VirtIO and USB HID encodings | user space |
| `userland` | Shell, libc, network codecs, filesystem bridge | user space |
| `toolchain` | `tpt-k-build` CLI: plans builds, stages the boot image, runs QEMU and replays, checks the assurance graph and manifests, and runs `doctor`. The `xtask` runner drives `check`, `build-kernel`, `qemu-smoke` and `fetch-deps` | host |

The kernel-side crates (`tpt-k-kernel-*`, and the TCB crates they depend on)
are `#![no_std]` with no C or C++ bindings. Host-side crates may use `std`.
The rule is in [CONTRIBUTING.md](../CONTRIBUTING.md) and the
[developer guide](developer-guide.md).

## Boot flow

1. UEFI firmware starts `tpt-k-boot`, which reads `TPTK\KERNEL.ELF` from the
   boot volume and places its `PT_LOAD` segments at their physical addresses.
2. The loader fills an 80-byte handoff record with the UEFI memory map, the ACPI
   RSDP and the GOP framebuffer, leaves boot services, and jumps to the kernel
   entry with the record's address in `rdi`.
3. The kernel checks the record, loads its GDT and IDT, builds its own paging
   (see Memory and scheduling), and starts the APIC timer. It then runs its
   built-in bootstrap threads and the QEMU smoke checks on serial.

The loader loads the kernel only. The root server is not loaded from the boot
volume yet, so the kernel does not start it.

External TPT libraries, such as `tpt-primitives`, `tpt-simd` and `tpt-fabric`,
are separate repos consumed by path or git. `scripts/fetch-deps.sh` clones them.
They are not part of this repo's trusted base, but the parts the kernel uses
must build `no_std` (checked in CI).

## Trust boundary

- **Kernel (ring 0):** frame allocator, page tables and address spaces, thread
  scheduling and context switches, the endpoint and capability code, and the
  interrupt entry points. It is the trusted computing base, and the most
  checked code in the repo.
- **Root server and services (ring 3):** hold capabilities and act on them. The
  root server is not running yet (see Status). A bug here can break the service it runs, but it cannot widen rights the kernel
  did not give it.
- **Drivers (ring 3):** the design has them reach hardware only through
  capabilities for MMIO ranges, interrupt lines and DMA memory. Those capability
  types are not built yet (see `docs/todos/kernel.md`, Phase 3). A driver crash
  is meant to restart the driver, not panic the kernel. The restart policy is in
  `tpt-k-drivers`, and it is host-tested.

## Capabilities

A capability is an object plus a set of rights (`READ`, `WRITE`, `GRANT`).

- **Rights only narrow.** Deriving a capability, or copying one to another
  thread, yields at most the rights the source has. Only a capability with
  `GRANT` can be derived from or copied.
- **Revocation is transitive.** Revoking a capability revokes everything derived
  from it, and every copy made from it in any thread's table.
- **Handles are checked.** A handle carries a slot index and a generation. A
  handle to a revoked slot is refused even after the slot is reused. Each
  thread's table is separate, so a handle from one thread names nothing in
  another's table unless it was granted.
- **Tokens for services.** User-space services check an HMAC-SHA256 token under
  a server key before checking rights (`tpt-k-ipc-cap`). A token made under
  another key is refused.

The exact invariants, and the checks behind them, are in
[assurance/invariants.md](assurance/invariants.md).

## IPC

Endpoints are synchronous. A message moves when a sender and a receiver are both
present. A party that arrives first waits in a bounded FIFO queue, and the next
party to arrive completes the transfer and wakes it. Messages are four words. A
capability can travel with a message, which moves a copy of it to the receiver.
Larger payloads go through memory objects, which are shared pages the receiver
maps. The 4-word messages are still copied by the kernel; a zero-copy path is
not done.

Bulk payloads are copied once, at send time, into a new memory object of up to
four pages (`SYS_SEND_BULK`). The receiver gets a capability to that object.

System calls are entered through `syscall`. The kernel defines 18 numbered calls
(`SYS_WRITE` to `SYS_WAIT` in `tpt-k-kernel`): output and exit, endpoint
send/receive (with variants that carry a capability or a bulk payload), grant,
revoke and yield, memory objects (create, map, unmap, free), spawning a child or
an image, interrupt wait, and `SYS_WAIT` for a child's exit code. Handles cross
the boundary as raw words and are checked against the caller's own table.

## Memory and scheduling

- Physical frames come from a run-time allocator built from the loader's memory
  map. Page tables are allocated through the same allocator.
- The kernel identity-maps 4 GiB with 2 MiB pages, and maps user memory in a
  separate window. Each spawned process gets its own address space, forked from a
  template taken from the boot space.
- A child's address space is released when its creator collects the exit with
  `SYS_WAIT`. `SYS_UNMAP` removes a mapping, and a memory object's frames are
  returned by `SYS_MEM_FREE`.
- Scheduling is timer-driven round-robin at 100 Hz, using the local APIC
  calibrated against the PIT. A 32-level priority run queue is host-tested but is
  not yet wired into the context switch.
- The IO-APIC routes the RTC interrupt to a user-space driver thread through an
  interrupt object (`SYS_IRQ_WAIT`). General interrupt routing is open.

## Assurance

The assurance graph (`docs/assurance/graph.json`) links each claim to the tests,
exhaustive checks and fuzz targets that back it. `tpt-k-build assure` checks it.
The gate runs in `scripts/check.sh`, as `tpt-k-build assure`. Some claims are
backed by z3 proofs over a bit-level model (`docs/assurance/proofs/`), and by
seeded mutation campaigns (`docs/assurance/campaigns/`). The generated
[report](assurance/report.md) says which claims are bounded-proven, which are
only tested, and which are waived. A fuzz campaign or a sampled test is never
reported as a proof.

## Build and test

- `scripts/check.sh`: formatting, clippy with warnings denied, host tests, the
  assurance gate, and the license and CVE gate when `tpt-cargo-vitals` is on
  `PATH`.
- `scripts/build-kernel.sh`: builds the kernel with its linker script for
  `x86_64-unknown-none`.
- `scripts/qemu-smoke.sh`: boots the loader and kernel under QEMU and OVMF, and
  checks the serial output and the exit code. It runs in CI as a separate job.
- `cargo xtask <task>` runs the same tasks as the scripts, on Windows and Linux.
- CI runs the same checks per build group, plus the `no_std` checks on the
  kernel-side crates.

## Not built yet

The root server's kernel handoff and start-up, loading the root from the boot
volume, the WASM driver host, device I/O for NVMe, VirtIO and USB, process
spawning in the shell, the networking daemon, AArch64 (its crate has stubs only),
SMP, and running on real hardware.
The per-layer checklists in [todos/](todos/) say what each piece still needs.
