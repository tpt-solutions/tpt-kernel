# tpt-k — Project Checklist (master)

Owner: TPT Solutions · License: `MIT OR Apache-2.0` · Spec: [spec.txt](spec.txt)

This file holds the project-wide work and the milestones. Each repo has its own
`todo.md` with the tasks it owns. Keep the two in step: when a task moves or
closes, update both.

| Repo | Owns | Its todo |
|---|---|---|
| `tpt-k-boot` | UEFI loader, boot handoff | [todo](../tpt-k-boot/todo.md) |
| `tpt-k-kernel` | kernel core, memory, scheduler, arch | [todo](../tpt-k-kernel/todo.md) |
| `tpt-k-root` | root server, capability generation | [todo](../tpt-k-root/todo.md) |
| `tpt-k-hal` | ACPI, PCI, FDT, USB parsers | [todo](../tpt-k-hal/todo.md) |
| `tpt-k-ipc` | user-space IPC libraries | [todo](../tpt-k-ipc/todo.md) |
| `tpt-k-drivers` | driver host, virtio, nvme, usb-hid, synth | [todo](../tpt-k-drivers/todo.md) |
| `tpt-k-userland` | shell, libc, network, fs bridge | [todo](../tpt-k-userland/todo.md) |
| `tpt-k-toolchain` | build CLI, ISO and image assembly | [todo](../tpt-k-toolchain/todo.md) |
| `tpt-crypto-rustls` | rustls provider over `tpt-crypto` | [todo](../tpt-crypto-rustls/todo.md) |
| `tpt-crypto` | constant-time crypto substrate | [todo](../tpt-crypto/todo.md) |
| `tpt-mcpbox` | MCP host (TPT dependency) | [todo](../tpt-mcpbox/todo.md) |

Rules that apply to every task:
- Repos are prefixed `tpt-k-`; crates are prefixed with their repo name.
- Kernel/TCB crates are `#![no_std]`, 100% Rust, no C/C++ bindings.
- No GPL/LGPL/AGPL/MPL or Apache-only dependencies. Apache-2.0 alone is allowed in dev-dependencies only. Allowed: MIT, Apache-2.0 only when dual with MIT, BSD, ISC, Zlib, Unlicense/CC0, Unicode, MIT-0, BSL-1.0, and the scoped exceptions in [LICENSE-POLICY.md](LICENSE-POLICY.md).
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
- [x] Allow `MIT-0` and `BSL-1.0` in the dependency allowlist (2026-11-09)
- [x] Record `webpki-roots` (CDLA-Permissive-2.0) as a scoped exception (2026-11-09)
- [x] Restrict Apache-2.0-alone dependencies to dev-dependencies only (2026-11-10)
- [x] Replace `aws-lc-sys` in mcpbox (2026-11-10) with `tpt-crypto-rustls`, and `sync_wrapper` in the shipped graph with hyper (per mcpbox todo, Phase 14)

### TLS crypto (see the `tpt-crypto` and `tpt-crypto-rustls` todos)
- [x] Create the `tpt-crypto-rustls` repo (2026-11-10), with 36 TLS 1.3 and 6 TLS 1.2 handshake tests passing
- [ ] Create the GitHub remote for `tpt-crypto-rustls` and push (`gh` is not installed)
- [ ] Get an external audit of `tpt-crypto` and `tpt-crypto-rustls` before either carries real traffic

### Repositories
- [x] Create repo `tpt-k-kernel` (local, no remote yet)
- [x] Create repo `tpt-k-root` (local, no remote yet)
- [x] Create repo `tpt-k-hal` (local, no remote yet)
- [x] Create repo `tpt-k-ipc` (local, no remote yet)
- [x] Create repo `tpt-k-drivers` (local, no remote yet)
- [x] Create repo `tpt-k-boot` (local, no remote yet)
- [x] Create repo `tpt-k-userland` (local, no remote yet)
- [x] Create repo `tpt-k-toolchain` (local, no remote yet)
- [ ] Meta/coordination: script to clone all 8 repos side by side (+ existing TPT repos and `tpt-crypto`, `tpt-crypto-rustls`, which are path-linked)
- [x] Per-repo: Cargo workspace, README, license files, `.gitignore`, `rust-toolchain.toml`
- [x] Per-repo: `todo.md` (2026-11-10)

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
- [x] Install/script QEMU + OVMF for local boot testing
- [x] QEMU test harness (boot, capture serial, pass/fail exit codes)
- [x] Configure `tpt-semadiff` as default git difftool
- [ ] Confirm `tpt-cargo-vitals` runs locally and in <100ms (measured ~5–7 s on an empty repo: fixed overhead in vitals)
- [ ] Verify existing TPT deps build `no_std` and are reachable via git/path: `tpt-primitives`, `tpt-simd`, `tpt-fabric`, `tpt-mcpbox`, `tpt-concord`, `tpt-filesystem`
- [ ] Audit licenses of TPT deps and their transitive deps (incl. WASM runtime used by `tpt-mcpbox`)

### CI
- [x] GitHub Actions workflow per repo: fmt, clippy, build, tests
- [x] CI gate: `tpt-cargo-vitals check --enforce-license MIT --ci-mode` (run with the policy allowlist; see DEP-AUDIT.md)
- [ ] CI: QEMU boot smoke test job
- [x] CI: `no_std` / no-C-bindings check on TCB crates
- [ ] CI: reusable workflow shared across the 8 repos

---

## Milestones

Each milestone is closed by the tasks in the owning repos. Tick it here only when
those repo todos are done.

- [ ] **M1 (Phase 1):** custom kernel boots in QEMU via `tpt-k-boot` (repos: `tpt-k-boot`, `tpt-k-kernel`)
- [ ] **M2 (Phase 2):** microkernel with context switching and secure message passing (repos: `tpt-k-kernel`, `tpt-k-root`, `tpt-k-ipc`)
- [ ] **M3 (Phase 3):** kernel talks to hardware safely; driver crashes do not panic the kernel (repos: `tpt-k-kernel`, `tpt-k-root`, `tpt-k-hal`, `tpt-k-drivers`)
- [ ] **M4 (Phase 4):** system persists and reads state from a virtual disk (repos: `tpt-k-drivers`, `tpt-k-ipc`, `tpt-k-userland`)
- [ ] **M5 (Phase 6):** parity: Phases 1–4 milestones pass on AArch64 (repos: `tpt-k-kernel`, `tpt-k-hal`, `tpt-k-boot`, `tpt-k-ipc`)
- [ ] **M6 (Phase 7):** bootable QEMU-ready image artifact in CI, with networking (repos: `tpt-k-toolchain`, `tpt-k-userland`, `tpt-k-drivers`)
- [ ] **M7 (Phase 8):** USB input and dynamically generated micro-drivers under review (repos: `tpt-k-hal`, `tpt-k-drivers`)

---

## Phase 5 — Formal Assurance & Automation (ongoing, starts alongside Phase 2)

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
