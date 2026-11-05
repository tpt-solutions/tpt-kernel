# tpt-k — Project Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0` · Spec: [spec.txt](spec.txt)

Rules that apply to every task:
- Repos are prefixed `tpt-k-`; crates are prefixed with their repo name.
- Kernel/TCB crates are `#![no_std]`, 100% Rust, no C/C++ bindings.
- No GPL/LGPL/AGPL/MPL or Apache-only dependencies. Allowed: MIT, Apache-2.0 only when dual with MIT, BSD, ISC, Zlib, Unlicense/CC0, Unicode.
- x86_64 + QEMU first. AArch64 and real hardware come later.
- Existing TPT repos are consumed as git/path deps (already `MIT OR Apache-2.0`).

---

## Phase 0 — Foundations & Setup

### Licensing & legal
- [x] Add `LICENSE-MIT` (copyright TPT Solutions) template
- [x] Add `LICENSE-APACHE` (Apache-2.0 full text) template
- [x] Define SPDX header `// SPDX-License-Identifier: MIT OR Apache-2.0` for all source files
- [ ] Set `license = "MIT OR Apache-2.0"` and authors/org in every `Cargo.toml`
- [x] Write license policy doc (allowlist/denylist above)
- [x] Write CONTRIBUTING note: contributions licensed under the same dual license
- [ ] Decide trademark/NOTICE handling for "tpt-k" / TPT Solutions

### Repositories
- [x] Create repo `tpt-k-kernel` (local, no remote yet)
- [x] Create repo `tpt-k-root` (local, no remote yet)
- [x] Create repo `tpt-k-hal` (local, no remote yet)
- [x] Create repo `tpt-k-ipc` (local, no remote yet)
- [x] Create repo `tpt-k-drivers` (local, no remote yet)
- [x] Create repo `tpt-k-boot` (local, no remote yet)
- [x] Create repo `tpt-k-userland` (local, no remote yet)
- [x] Create repo `tpt-k-toolchain` (local, no remote yet)
- [ ] Meta/coordination: script to clone all 8 repos side by side (+ existing TPT repos)
- [x] Per-repo: Cargo workspace, README, license files, `.gitignore`, `rust-toolchain.toml`

### Crate scaffolds (empty, correctly named, building)
- [x] `tpt-k-kernel`: `tpt-k-kernel`, `-sched`, `-mem`, `-ipc`, `-arch-x86_64`, `-arch-aarch64` (stub)
- [x] `tpt-k-root`: `tpt-k-root`, `-caps`, `-spawn`
- [x] `tpt-k-hal`: `tpt-k-hal`, `-acpi`, `-fdt`, `-pci`, `-usb`
- [x] `tpt-k-ipc`: `tpt-k-ipc`, `-cap`, `-serde`
- [x] `tpt-k-drivers`: `tpt-k-drivers`, `-nvme`, `-virtio`, `-usb-hid`, `-synth`
- [x] `tpt-k-boot`: `tpt-k-boot`, `-efi`, `-fs`
- [x] `tpt-k-userland`: `tpt-k-userland`, `-shell`, `-net`, `-libc`, `-fs-bridge`
- [x] `tpt-k-toolchain`: `tpt-k-toolchain` (CLI `tpt-k-build`), `-iso`
- [ ] Verify all kernel-side crates are `#![no_std]` and CI-checked for it

### Toolchain & dev environment
- [x] Pin Rust toolchain; add targets (`x86_64-unknown-none`, `x86_64-unknown-uefi`, later `aarch64-*`)
- [ ] Install/script QEMU + OVMF for local boot testing
- [ ] QEMU test harness (boot, capture serial, pass/fail exit codes)
- [ ] Configure `tpt-semadiff` as default git difftool
- [ ] Confirm `tpt-cargo-vitals` runs locally and in <100ms
- [ ] Verify existing TPT deps build `no_std` and are reachable via git/path: `tpt-primitives`, `tpt-simd`, `tpt-fabric`, `tpt-mcpbox`, `tpt-concord`, `tpt-filesystem`
- [ ] Audit licenses of TPT deps and their transitive deps (incl. WASM runtime used by `tpt-mcpbox`)

### CI
- [ ] GitHub Actions workflow per repo: fmt, clippy, build, tests
- [ ] CI gate: `tpt-cargo-vitals check --enforce-license MIT --ci-mode`
- [ ] CI: QEMU boot smoke test job
- [ ] CI: `no_std` / no-C-bindings check on TCB crates
- [ ] CI: reusable workflow shared across the 8 repos

---

## Phase 1 — Boot & Hello World (Weeks 1–4)

### tpt-k-boot
- [ ] `tpt-k-boot-efi`: safe wrappers for UEFI boot services and runtime services
- [ ] `tpt-k-boot-fs`: minimal read-only FAT32 driver
- [ ] `tpt-k-boot`: UEFI app that locates kernel (and root server) images on the ESP
- [ ] Load kernel binary into memory, obtain memory map, exit boot services
- [ ] Define boot info handoff struct (memory map, framebuffer, ACPI RSDP, etc.)
- [ ] Jump to kernel entry with correct state

### tpt-k-kernel
- [ ] `tpt-k-kernel`: early entry, stack setup, panic handler
- [ ] `tpt-k-kernel-arch-x86_64`: GDT, IDT, basic exception handlers
- [ ] Serial (COM1) console output
- [ ] VGA/framebuffer text output (optional)
- [ ] Identity paging setup (x86_64)
- [ ] Print "Hello tpt-k"

### Verification
- [ ] Automated QEMU test asserts "Hello tpt-k" on serial
- [ ] **Milestone:** custom kernel boots in QEMU via tpt-k-boot

---

## Phase 2 — IPC & The Root Server (Weeks 5–10)

### Kernel core
- [ ] `tpt-k-kernel-mem`: physical page frame allocator
- [ ] `tpt-k-kernel-mem`: virtual memory management, per-process address spaces
- [ ] `tpt-k-kernel-mem`: page clearing via `tpt-simd` (scalar fallback)
- [ ] `tpt-k-kernel-arch-x86_64`: APIC/timer, SYSCALL/SYSRET entry
- [ ] `tpt-k-kernel-sched`: thread structures, context switching
- [ ] `tpt-k-kernel-sched`: preemptive priority-based scheduler
- [ ] `tpt-k-kernel-ipc`: syscall entry/exit and dispatch
- [ ] `tpt-k-kernel-ipc`: capability table + capability translation
- [ ] `tpt-k-kernel-ipc`: zero-copy, capability-routed message passing (endpoints)

### User-space IPC libraries
- [ ] `tpt-k-ipc-cap`: capability tokens, rights, unforgeable handles (using `tpt-primitives` CapabilityId/CBOR/SHA-256)
- [ ] `tpt-k-ipc-serde`: zero-copy (de)serialization + checksum (using `tpt-simd`)
- [ ] `tpt-k-ipc`: client/server framework

### Root server
- [ ] `tpt-k-root-spawn`: ELF loader
- [ ] `tpt-k-root-spawn`: process lifecycle management
- [ ] `tpt-k-root-caps`: capability generation, delegation, revocation
- [ ] `tpt-k-root`: main root server binary, first user-space process
- [ ] Kernel spawns root server and hands it initial capabilities
- [ ] Integrate `tpt-fabric` model: Intent → Reservation → Execution → Trace
- [ ] Root server pass a simple capability token via IPC, verified with `tpt-primitives`

### Verification
- [ ] Tests: IPC round-trip, capability denial, revocation, context switching
- [ ] **Milestone:** microkernel with context switching and secure message passing

---

## Phase 3 — Hardware & Interrupts (Weeks 11–16)

### Kernel
- [ ] Interrupt routing: map vector → user-space driver notification channel and yield
- [ ] IOMMU page table management in `tpt-k-kernel-mem`
- [ ] Capabilities for MMIO ranges, IRQ lines, DMA memory

### HAL (written from scratch, no GPL parsers)
- [ ] `tpt-k-hal`: core hardware data structures
- [ ] `tpt-k-hal-acpi`: MADT, MCFG, DSDT/SSDT parsing
- [ ] `tpt-k-hal-pci`: PCIe config space enumeration + BAR parsing
- [ ] Unit/fuzz tests for each parser against real-world table dumps

### Root server hardware role
- [ ] Parse hardware topology via `tpt-k-hal`
- [ ] Evaluate hardware Intents, make IOMMU/PCIe Reservations, grant Execution capabilities to drivers
- [ ] Trace all grants via `tpt-primitives`

### Drivers & sandbox
- [ ] Driver-host process embedding `tpt-mcpbox` (WASM)
- [ ] Capability-checked WASM host functions: `mmio_read/write`, `dma_map`, `irq_wait`
- [ ] `tpt-k-drivers-virtio`: VirtIO console (or keyboard) driver
- [ ] Driver receives interrupts via IPC notification
- [ ] `tpt-k-drivers`: driver manager that monitors health and restarts crashed drivers

### Verification
- [ ] Test: misbehaving driver traps/crashes without kernel panic
- [ ] Test: driver cannot access memory/IRQs outside granted capabilities
- [ ] **Milestone:** kernel talks to hardware safely; driver crashes do not panic the kernel

---

## Phase 4 — Storage & Filesystem (Weeks 17–24)

- [ ] `tpt-k-drivers-virtio`: VirtIO-blk driver
- [ ] `tpt-k-drivers-nvme`: user-space NVMe driver
- [ ] Block-device IPC protocol defined in `tpt-k-ipc`
- [ ] Run `tpt-filesystem` as privileged user-space daemon
- [ ] `tpt-k-userland-fs-bridge`: translate IPC block/byte requests to `tpt-filesystem` API
- [ ] IPC/FUSE-style bridge for exposing filesystem to the rest of the system
- [ ] Mount filesystem and read a file from user space
- [ ] Persistence test (write, reboot, read back)
- [ ] **Milestone:** system persists and reads state from a virtual disk (NVMe/VirtIO-blk)

---

## Phase 5 — Formal Assurance & Automation (Ongoing, starts alongside Phase 2)

- [ ] Define core IPC + capability invariants in `tpt-concord` (e.g., "capability check cannot be bypassed")
- [ ] Property tests linked to each invariant
- [ ] Fuzzing campaigns: syscall interface, IPC serde, ACPI/PCI/FDT/USB parsers
- [ ] SMT/model-checking artifacts for capability + IPC invariants
- [ ] Build Assurance Graph; link proofs/tests/fuzzing to kernel specs
- [ ] CI gate fails when graph shows unproven critical claims
- [ ] Wire `tpt-cargo-vitals` into daily local workflow (pre-commit/pre-push)
- [ ] Wire `tpt-semadiff` into review workflow for generated/refactored code
- [ ] Report: what is proven vs unproven (published per release)

---

## Phase 6 — AArch64 Port

- [ ] `tpt-k-kernel-arch-aarch64`: exception levels (EL1), vectors, context switch
- [ ] MMU / page table setup
- [ ] GIC interrupt controller + timer
- [ ] `tpt-k-hal-fdt`: Flattened Device Tree parser
- [ ] UEFI AArch64 boot via `tpt-k-boot`
- [ ] `tpt-simd` NEON paths in mem and ipc-serde
- [ ] QEMU `virt` machine test harness + CI job
- [ ] Parity: Phases 1–4 milestones pass on AArch64

---

## Phase 7 — Userland, Network & Toolchain

### Toolchain
- [ ] `tpt-k-toolchain` (`tpt-k-build`): resolve deps, cross-compile all workspaces, link binaries
- [ ] `tpt-k-toolchain-iso`: assemble bootloader + kernel + root server + root FS into UEFI ISO / raw disk image
- [ ] CI produces bootable QEMU-ready image artifact

### Userland
- [ ] `tpt-k-userland-shell`: command parser, job control
- [ ] `tpt-k-userland`: meta-crate and interactive shell binary
- [ ] `tpt-k-userland-libc`: minimal POSIX shim over `tpt-k-ipc`
- [ ] `tpt-k-userland-net`: user-space TCP/IP stack daemon
- [ ] VirtIO-net driver in `tpt-k-drivers-virtio`
- [ ] Zero-copy packet path NIC driver → network stack → applications
- [ ] Network integration tests in QEMU (ping, TCP echo)

---

## Phase 8 — Driver Expansion & Automatic Drivers

- [ ] `tpt-k-hal-usb`: USB descriptor parser
- [ ] `tpt-k-drivers-usb-hid`: USB HID driver (keyboard/mouse)
- [ ] `tpt-k-drivers-synth`: hardware-ID matching against known layouts
- [ ] `tpt-k-drivers-synth`: generate/load WASM micro-drivers dynamically
- [ ] Sandbox + capability review for generated drivers
- [ ] Datasheet-driven generation pipeline (AI/tooling boilerplate reviewed via `tpt-semadiff`)

---

## Phase 9 — Bare Metal & Hardening

- [ ] Boot on real UEFI x86_64 hardware
- [ ] NVMe driver on real hardware
- [ ] ACPI/PCI compatibility fixes from real machine dumps
- [ ] Root server drops supreme privileges after bootstrap (verified by test + Assurance Graph)
- [ ] Performance benchmarks: IPC latency, context switch, interrupt-to-driver latency
- [ ] `tpt-simd` optimisation pass (AVX2/NEON with scalar fallbacks)
- [ ] Security review of syscall interface and capability code
- [ ] Review all `unsafe` blocks; document invariants

---

## Phase 10 — Documentation & Release

- [ ] Architecture docs and per-repo READMEs
- [ ] Developer guide: build, run in QEMU, add a driver
- [ ] Versioning and release process across the 8 repos
- [ ] Final license audit across all repos (CI-green with `tpt-cargo-vitals`)
- [ ] Publish repos / first tagged release
