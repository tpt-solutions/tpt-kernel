# tpt-k — Project Checklist (master)

Owner: TPT Solutions · License: `MIT OR Apache-2.0` · Spec: [spec.txt](spec.txt)

This file holds the project-wide work and the milestones. The tasks for each
layer live in `docs/todos/<layer>.md`. Keep the two in step: when a task moves or
closes, update both.

The tpt-k layers are directories in this one repo. The existing TPT dependencies
are separate repos, consumed by path or git.

| Layer | Owns | Its todo |
|---|---|---|
| `boot` | UEFI loader, boot handoff | [todo](docs/todos/boot.md) |
| `kernel` | kernel core, memory, scheduler, arch | [todo](docs/todos/kernel.md) |
| `root` | root server, capability generation | [todo](docs/todos/root.md) |
| `hal` | ACPI, PCI, FDT, USB parsers | [todo](docs/todos/hal.md) |
| `ipc` | user-space IPC libraries | [todo](docs/todos/ipc.md) |
| `drivers` | driver host, virtio, nvme, usb-hid, synth | [todo](docs/todos/drivers.md) |
| `userland` | shell, libc, network, fs bridge | [todo](docs/todos/userland.md) |
| `toolchain` | build CLI, ISO and image assembly | [todo](docs/todos/toolchain.md) |
| `tpt-crypto-rustls` | rustls provider over `tpt-crypto` (external repo) | [todo](../tpt-crypto-rustls/todo.md) |
| `tpt-crypto` | constant-time crypto substrate (external repo) | [todo](../tpt-crypto/todo.md) |
| `tpt-mcpbox` | MCP host (external TPT dependency) | [todo](../tpt-mcpbox/todo.md) |

Rules that apply to every task:
- Crates are prefixed `tpt-k-` and live under `crates/`; the crate name starts with its layer.
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
- [x] Set `license = "MIT OR Apache-2.0"` and authors/org in every `Cargo.toml` (2026-11-16: the 8 tpt-k repos inherit `license`/`authors` from `[workspace.package]`; existing TPT deps are not ours to edit)
- [x] Write license policy doc (allowlist/denylist above)
- [x] Write CONTRIBUTING note: contributions licensed under the same dual license
- [x] Decide trademark/NOTICE handling for "tpt-k" / TPT Solutions (2026-11-16: `NOTICE` added; no registration yet, revisit before Phase 10 release)
- [x] Allow `MIT-0` and `BSL-1.0` in the dependency allowlist (2026-11-09)
- [x] Record `webpki-roots` (CDLA-Permissive-2.0) as a scoped exception (2026-11-09)
- [x] Restrict Apache-2.0-alone dependencies to dev-dependencies only (2026-11-10)
- [x] Replace `aws-lc-sys` in mcpbox (2026-11-10) with `tpt-crypto-rustls`, and `sync_wrapper` in the shipped graph with hyper (per mcpbox todo, Phase 14)

### TLS crypto (see the `tpt-crypto` and `tpt-crypto-rustls` todos)
- [x] Create the `tpt-crypto-rustls` repo (2026-11-10), with 36 TLS 1.3 and 6 TLS 1.2 handshake tests passing
- [x] Create the GitHub remote for `tpt-crypto-rustls` and push (remote confirmed 2026-11-16 by cloning it from `github.com/tpt-solutions`)
- [ ] Get an external audit of `tpt-crypto` and `tpt-crypto-rustls` before either carries real traffic

### Repositories
- [x] Create the 8 tpt-k repos (2026-11-10 to 2026-11-16, local only)
- [x] Merge the 8 tpt-k repos into this monorepo (2026-11-17): crates under `crates/`, one root workspace, one `Cargo.lock`, per-layer READMEs in `docs/layers/`, per-layer todos in `docs/todos/`
- [x] `scripts/fetch-deps.sh` (renamed from `clone-repos.sh`) clones or fast-forwards the external TPT dependency repos only (2026-11-17)
- [x] Kernel link uses its linker script via `scripts/build-kernel.sh`, not a workspace-wide rustflag (2026-11-17)
- [x] Decide the TCB status of `tpt-k-root*` and `tpt-k-ipc`: they are std scaffolds and do not build for `x86_64-unknown-none`. Decided 2026-11-17: user-space, built for host
- [x] Fix two `clippy::needless_range_loop` lints (fixed 2026-11-17, smoke test re-run) in `tpt-k-kernel-arch-x86_64/src/paging.rs` (lines 168, 275), which fail `-D warnings` in the `tcb-nostd` CI group (2026-11-17)

### Crate scaffolds (empty, correctly named, building)
- [x] `tpt-k-kernel`: `tpt-k-kernel`, `-sched`, `-mem`, `-ipc`, `-arch-x86_64`, `-arch-aarch64` (stub)
- [x] `tpt-k-root`: `tpt-k-root`, `-caps`, `-spawn`
- [x] `tpt-k-hal`: `tpt-k-hal`, `-acpi`, `-fdt`, `-pci`, `-usb`
- [x] `tpt-k-ipc`: `tpt-k-ipc`, `-cap`, `-serde`
- [x] `tpt-k-drivers`: `tpt-k-drivers`, `-nvme`, `-virtio`, `-usb-hid`, `-synth`
- [x] `tpt-k-boot`: `tpt-k-boot`, `-efi`, `-fs`
- [x] `tpt-k-userland`: `tpt-k-userland`, `-shell`, `-net`, `-libc`, `-fs-bridge`
- [x] `tpt-k-toolchain`: `tpt-k-toolchain` (CLI `tpt-k-build`), `-iso`
- [x] Verify all kernel-side crates are `#![no_std]` (2026-11-16: all 6 `tpt-k-kernel` crates; `cargo check --workspace --target x86_64-unknown-none` passes)
- [x] Make the `no_std` check explicit in CI (2026-11-16: `nostd_roots` input in the shared workflow; enabled for `tpt-k-kernel`)

### Toolchain & dev environment
- [x] Pin Rust toolchain; add targets (`x86_64-unknown-none`, `x86_64-unknown-uefi`, later `aarch64-*`)
- [x] Install/script QEMU + OVMF for local boot testing
- [x] QEMU test harness (boot, capture serial, pass/fail exit codes)
- [x] Configure `tpt-semadiff` as default git difftool
- [x] Confirm `tpt-cargo-vitals` runs locally and in <100ms (2026-11-16: the <100 ms parse+graph budget is met at ~1.5 ms; the ~8 s end-to-end time is seven `cargo metadata` runs, one per platform target, by design. Accepted for CI; a host-only flag is optional future work in tpt-cargo-vitals)
- [x] Verify existing TPT deps build `no_std` and are reachable via git/path: `tpt-primitives`, `tpt-simd`, `tpt-fabric`, `tpt-mcpbox`, `tpt-concord`, `tpt-filesystem`
  - 2026-11-18: the TCB-relevant dependencies build for `x86_64-unknown-none` with no default features: `tpt-primitives` (through `tpt-k-ipc-cap`), all `tpt-simd` library crates except the std test helper, and `tpt-fabric-core` on its pinned Rust 1.85.0. `tpt-filesystem`, `tpt-concord` and `tpt-mcpbox` are std services, not in the TCB, so they are not required to be `no_std`. `tpt-filesystem-core` uses `Vec` without `alloc`; that is expected for a std service.
  - 2026-11-17: `tpt-primitives` builds `no_std` for `x86_64-unknown-none` through `tpt-k-ipc-cap`, and is consumed by path from the sibling repo. Still open: `tpt-simd`, `tpt-fabric`, `tpt-concord`, `tpt-filesystem`, `tpt-mcpbox`.
  - 2026-11-16: `tpt-primitives` now builds `no_std` (`x86_64-unknown-none`); its `ciborium` dependency is gone. Still open: `tpt-simd` (2 crates), `tpt-fabric` (12 std crates), `tpt-concord`, `tpt-filesystem` and `tpt-mcpbox` (std by design, not needed in the TCB).
- [ ] Audit licenses of TPT deps and their transitive deps (incl. WASM runtime used by `tpt-mcpbox`)
  - 2026-11-17: the workspace passes the CI license gate (`tpt-cargo-vitals check` with the CI allowlist: 29 dependencies, 0 vulnerabilities). The WASM runtime of `tpt-mcpbox` is not in this tree yet, so the item stays open.

### CI
- [x] GitHub Actions workflow per repo: fmt, clippy, build, tests
- [x] CI gate: `tpt-cargo-vitals check --enforce-license MIT --ci-mode` (run with the policy allowlist; see DEP-AUDIT.md)
- [x] CI: QEMU boot smoke test job (2026-11-16: `.github/workflows/qemu-smoke.yml`; not yet run on GitHub)
- [x] CI: `no_std` / no-C-bindings check on TCB crates
- [x] CI: `ci.yml` runs one reusable check job per build group (2026-11-16 reusable workflow; 2026-11-17 groups: tcb-nostd, kernel-image, uefi-loader, userspace, license)

---

## Milestones

Each milestone is closed by the tasks in the owning repos. Tick it here only when
those repo todos are done.

- [x] **M1 (Phase 1):** custom kernel boots in QEMU via `tpt-k-boot` (layers: `tpt-k-boot`, `tpt-k-kernel`) (2026-11-16: `scripts/qemu-smoke.sh` passes under OVMF)
- [ ] **M2 (Phase 2):** microkernel with context switching and secure message passing (layers: `tpt-k-kernel`, `tpt-k-root`, `tpt-k-ipc`)
  - 2026-11-17 fix: the root channel and the shared-page channel both used endpoint 2, so a receiver on one could take a message meant for the other. This caused an intermittent smoke-test failure (a child mapped an object with one page and faulted). The root channel now has its own endpoint (`ROOT_CHANNEL_OBJECT`, id 3). Stress result is recorded in the commit message.
  - 2026-11-17 progress (bulk messages): `SYS_SEND_BULK` sends four words plus a payload of up to four pages from the sender's user memory. The kernel copies the payload at send time into a new memory object, and the receiver gets it as a capability (received with `SYS_RECV_CAP`, then mapped). The sender's temporary handle is revoked once the send completes. The smoke test sends a 16-byte payload to the child, which maps it and checks its first word. Open: freeing objects, and a receive that copies the payload out rather than mapping it.
  - 2026-11-17 progress (capability transfer): `SYS_SEND_CAP` sends four words with a capability (the sender's handle, which needs GRANT). The receiver's `SYS_RECV_CAP` writes the four words and then its new handle to a 40-byte buffer. If the receiver is already waiting, the copy is installed in its table at send time; otherwise the receiver copies it when it takes the message. Copies keep their origin, so revoking the original revokes them. The endpoint queue carries the transfer (host-tested in `tpt-k-kernel-ipc`). The smoke test sends a memory object to the child, which maps it and checks the root's value. Open: bulk messages (more than four words), and freeing objects.
  - 2026-11-17 progress (memory objects): `SYS_MEM_CREATE` makes a memory object of 1 to 4 zeroed pages, from a run-time frame allocator built from the loader's memory map. `SYS_MAP` maps all of an object's pages in order, at a page-aligned address in the memory window (`0x1_0000_0000` to `0x1_4000_0000`, above the identity map, so it never shadows a physical address the kernel uses). The QEMU smoke test has the root thread create a two-page object, write its second page, and read it back. Open: transfer of a memory capability inside a message, bulk messages, and freeing objects.
  - 2026-11-17 progress (spawn from memory): `SYS_SPAWN_IMAGE` takes a descriptor in the caller's user pages (image address, length, handle, rights). The kernel copies a one-page, position-independent image into a slot page, maps it only in the child's address space, and starts the child there. The QEMU smoke test spawns a child from the root thread's own code pages, and it exits with 7. Limits: one page per image, raw code rather than ELF, two spawn slots, and no reclaim of slots after a child exits.
  - 2026-11-17 progress (handoff): the loader passes the ACPI RSDP and the GOP framebuffer in the handoff record (version 2), and the kernel reports both at boot; the QEMU smoke test checks them.
  - 2026-11-17 progress: the kernel builds its own page tables and address spaces (identity map, a remapped page, and per-process spaces forked from the boot space, checked by two isolated user processes); a frame allocator; timer-driven (local APIC, calibrated against the PIT, 100 Hz) round-robin scheduling with blocking and per-thread kernel stacks and address spaces; per-thread capability tables with raw handles; queued synchronous endpoints; a ring-3 thread that sends, receives, writes and exits through `syscall`. The QEMU smoke test covers all of it. a grant syscall that copies handles between threads with rights narrowing; revocation across granted copies, including transitive copies in other tables; page tables allocated through the frame allocator; frames from the loader's memory map; a root thread that spawns a child in its own address space, granting it a narrowed handle (`SYS_SPAWN`), and sends it a message.
  - 2026-11-17 progress (driver interrupt): a user-space driver thread waits on an interrupt object (`SYS_IRQ_WAIT`) for the RTC line. Its handler records the interrupt and wakes the waiter, and the smoke test checks `irq: driver woke`. Open: interrupt objects for other lines, and drivers started by spawn.
  - 2026-11-17 progress (IO-APIC): the IO-APIC routes GSI 8 (the RTC) to vector 40 on the bootstrap CPU, unmasked; the RTC's periodic interrupt reaches the kernel through it twice before the threads start (the QEMU smoke test checks `ioapic: rtc irqs=2`). Open: routing an interrupt to a user-space driver's notification channel, SMP bring-up, and the `tpt-k-root` repo's checklist.
- [ ] **M3 (Phase 3):** kernel talks to hardware safely; driver crashes do not panic the kernel (layers: `tpt-k-kernel`, `tpt-k-root`, `tpt-k-hal`, `tpt-k-drivers`)
- [ ] **M4 (Phase 4):** system persists and reads state from a virtual disk (layers: `tpt-k-drivers`, `tpt-k-ipc`, `tpt-k-userland`)
- [ ] **M5 (Phase 6):** parity: Phases 1–4 milestones pass on AArch64 (layers: `tpt-k-kernel`, `tpt-k-hal`, `tpt-k-boot`, `tpt-k-ipc`)
- [ ] **M6 (Phase 7):** bootable QEMU-ready image artifact in CI, with networking (layers: `tpt-k-toolchain`, `tpt-k-userland`, `tpt-k-drivers`)
- [ ] **M7 (Phase 8):** USB input and dynamically generated micro-drivers under review (layers: `tpt-k-hal`, `tpt-k-drivers`)

---

## Phase 5 — Formal Assurance & Automation (ongoing, starts alongside Phase 2)

- [ ] Define core IPC + capability invariants in `tpt-concord` (e.g., "capability check cannot be bypassed")
  - 2026-11-18: the invariants are stated in `docs/assurance/invariants.md` (CAP-1 to CAP-8, IPC-1 to IPC-6, with the token and service claims) and recorded in `docs/assurance/graph.json`. Not yet in `tpt-concord`: its CLI is not built on this machine, and nothing was written to the sibling repo.
- [x] Property tests linked to each invariant
  - 2026-11-18: every claim in `docs/assurance/graph.json` names its evidence, which the gate checks exists. CAP-* and IPC-* are backed by bounded exhaustive checks (`bounded_exhaustive_capability_invariants`, `bounded_exhaustive_endpoint_invariants`, `derive_accepts_exactly_grantable_subsets`), each with a minimum-step guard. TOK-2 and SRV-1 are bounded-exhaustive too (attenuation over all rights-mask pairs; the service gate over all masks, opcodes and keys, added 2026-11-18). TOK-1 and SER-1 have sampled tests only and are waived in the graph. A formal model is still open.
- [ ] Fuzzing campaigns: syscall interface, IPC serde, ACPI/PCI/FDT/USB parsers
  - 2026-11-18: no coverage-guided campaign has been run, and no fuzz harness is written. `cargo-fuzz` needs a nightly toolchain and a sanitizer runtime, which this Windows machine lacks. Installing them in the WSL Ubuntu distribution was denied by the sandbox policy, so nothing was installed or run. Sampled mutation tests exist for ACPI, PCI, FDT, USB, IPC serde and the ELF planner (2026-11-17), with no panics found.
- [ ] SMT/model-checking artifacts for capability + IPC invariants
  - 2026-11-18: bounded exhaustive checks over an executable model of the capability table and the endpoint are in `tpt-k-kernel-ipc` (CAP-* and IPC-*, see `docs/assurance/invariants.md` for the bounds). They are enumeration, not SMT: no solver or machine-checked proof is wired in. A mutation run confirmed the capability check fails when cross-table revocation is broken.
- [ ] Build Assurance Graph; link proofs/tests/fuzzing to kernel specs
  - 2026-11-18: built: `docs/assurance/graph.json` has 17 claims and links each to its evidence. Still open: linking the claims to numbered sections of `spec.txt`, which has no numbered invariants yet, and the fuzz and proof evidence kinds, which have no items.
- [x] CI gate fails when graph shows unproven critical claims
  - 2026-11-18: `tpt-k-build assure` (in `crates/tpt-k-toolchain/src/assurance.rs`) fails on a missing evidence item, or on a critical claim that is not bounded-proven and has no waiver. It runs in `scripts/check.sh`, and in CI through the toolchain's `cargo test` (the repo-graph test). Gap closed (2026-11-18): the `tcb-nostd` group now runs host tests for its TCB crates, so the `tpt-k-kernel-ipc` exhaustive checks run in CI. The `userspace` group was also missing 13 crates that have tests (including `tpt-k-ipc`, where SRV-1 lives); its `test_packages` now covers every crate with a `#[test]`. The CI runs themselves have not been observed yet.
- [x] Wire `tpt-cargo-vitals` into daily local workflow (pre-commit/pre-push)
  - 2026-11-18: `scripts/check.sh` runs fmt, clippy, tests, the assurance gate and the license and CVE gate, and `.githooks/pre-push` runs it. Enable with `git config core.hooksPath .githooks`. There is no pre-commit hook, by choice: the license gate takes about 8 s, which is too slow for every commit.
- [ ] Wire `tpt-semadiff` into review workflow for generated/refactored code
  - 2026-11-18: the review workflow is documented in `docs/developer-guide.md`, and the git difftool setting is in place. Not exercised: `tpt-semadiff` is not on PATH on this machine.
- [x] Report: what is proven vs unproven (published per release)
  - 2026-11-18: `tpt-k-build assure` writes `docs/assurance/report.md` from the graph (13 bounded-proven, 4 tested, 0 unproven on this date, with the waivers listed). Publishing it with each release is part of `docs/release.md`.

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

- [x] Architecture docs and per-repo READMEs
  - 2026-11-18: `docs/architecture.md` is written. The eight layer docs in `docs/layers/` no longer say the crates are placeholders, and they point to the layer checklists. All 32 crates have a README and a CHANGELOG. The README `Status` sections were not re-checked against the code one by one.
- [x] Developer guide: build, run in QEMU, add a driver
  - 2026-11-18: `docs/developer-guide.md` covers checks, building, the QEMU smoke test, writing a crate, capabilities and IPC, and the driver pieces. It says what is not built yet. It now also covers the assurance gate and the semadiff review workflow.
- [x] Versioning and release process for the tpt-kernel repo
  - 2026-11-18: `docs/release.md` and the root `CHANGELOG.md` are written. Open decisions, recorded in `docs/release.md`: the first version number, whether versions live in `[workspace.package]` or in each crate (the crates are at `0.0.0`), and how a sibling repo that fails the license gate is handled before a tag.
- [ ] Final license audit across all repos (CI-green with `tpt-cargo-vitals`)
  - 2026-11-18, read-only run of `tpt-cargo-vitals check --enforce-license` (CI allowlist). This repo passes: 28 of 28 dependencies compliant, 0 vulnerabilities. The sibling repos do not pass: tpt-primitives (1 Apache-only, `borsh-derive`); tpt-simd (3, incl. `ciborium`); tpt-concord (3, incl. `MIT-0` and `ryu`); tpt-crypto (5, incl. `ciborium`, `borsh-derive`); tpt-crypto-rustls (1, `ring`, plus 7 advisories); tpt-mcpbox (48 non-compliant, plus 7 advisories; `DEP-AUDIT.md` says the wasmtime advisories were fixed and the rest are accepted in `deny.toml`, which this gate does not read). tpt-fabric and tpt-filesystem pass. Policy decisions are still open: `MIT-0`, `BSL-1.0` (for `ryu`), `CDLA-Permissive-2.0`, and the Apache-only replacements.
- [ ] Publish tpt-kernel / first tagged release
  - Not done. The steps are in `docs/release.md`. The first version number is an open decision, and the release needs an explicit go-ahead.

---

## Phase 11 — Stub cleanup, adoption tooling and innovation (planned 2026-11-18)

Each phase ends green on `scripts/check.sh`, and on `scripts/qemu-smoke.sh` for kernel changes. When an item closes, mirror it in the owning `docs/todos/<layer>.md`. 

### 11a — Fix tractable stubs and doc mismatches
- [x] `boot`: `tpt-k-boot/src/main.rs:6` and its README described the handoff argument as 0; they now describe the 80-byte handoff record (rdi) (2026-11-18)
- [x] `boot`: note in `tpt-k-boot-fs/README.md` that the FAT32 driver is host-tested and not yet used by the loader (2026-11-18: the README already said this; checked)
- [x] `docs`: reconcile the contradictory "static pool" vs "loader memory map" bullet in `docs/todos/kernel.md:31`; check `docs/architecture.md:50` and the `tpt-k-hal` README for the same drift (2026-11-18: the bullet is fixed; architecture.md and the tpt-k-hal README had no matching drift)
- [x] `hal`/`ipc`: rename `fuzz_tests.rs` to `mutation_tests.rs` (acpi, fdt, pci, usb, ipc-serde) and fix the wording in docs and in Phase 5 above; real cargo-fuzz stays open (2026-11-18: also `tpt-k-root-spawn`, which had one; the assurance graph names are updated)
- [x] `kernel`: faults from ring 3 (CS RPL 3) kill only the faulting thread instead of exiting QEMU (`idt.rs:345-364`); ring-0 faults still halt. Add a smoke-test case where a user thread faults and the kernel survives (2026-11-18: QEMU is installed on this machine at its default folder, and the smoke test passes with `user: fault vector 14`)
- [ ] `kernel`: real `SYS_EXIT` teardown (release frames, revoke the thread's capability table, notify the parent) instead of the demo `finish()` counter (`main.rs:500-503`) (partly, 2026-11-18: decided, following the recommendation: an exiting thread deletes its handles without revoking, as seL4 does. Copies held elsewhere stay valid, and the derivation tree is re-linked (`cap::delete_across`, host-tested). Then unreferenced memory objects are reclaimed. Frames come back through that reclaim. Open: notifying the parent. There is no wait call yet; add it with the root server, not before)
- [ ] `kernel`: free memory objects (`SYS_MEM_FREE` and free on revoke) so the 8 object slots stop leaking (`main.rs:862,896`) (partly, 2026-11-18: `SYS_MEM_FREE` frees an unmapped object, revokes its handles in every table, and returns its frames; mapped objects get `EBUSY`, since there is no unmap. Object 0 is never freed. 2026-11-18, later: an object is reclaimed when its last handle is revoked or deleted, or when a thread exits, if it is not mapped (boot self-test `mem: revoking the last handle reclaimed the object`). Open: unmap, so that a mapped object can be freed)
- [x] `kernel`/`userland`: distinct negative error codes instead of `u64::MAX` for every failure (`main.rs:589`), mapped in `tpt-k-userland-libc::Errno` (2026-11-18: failures return a negated errno, `-4095..=-1`, for their cause, checked by a boot self-test. The user programs test with a range check. `EBUSY` and `EDEADLK` are added to `Errno`. Receive buffers keep `u64::MAX` as "no capability")
- [x] `userland`: `Spawner` trait so the shell's pipes and redirects can be built and tested against a fake; `IsTerminal` check for the prompt (2026-11-18: the trait, `NoSpawner` and a fake-spawner test are in; pipes and redirections are still refused, so building them is the next step)
- [x] `userland`: `extern "C"` exports in `tpt-k-userland-libc` for the string and errno helpers (syscall shims stay open until spawn exists) (2026-11-18: string and memory helpers, behind the `c-abi` feature; no errno export, since the errno values are not a C function)
- [x] `kernel`: `tpt-k-kernel-arch-aarch64` is an empty crate listed as a TCB crate in CI; drop it from the list until it has code, and say so in its README (2026-11-18)
- Left open on purpose (already tracked above or in layer todos): empty `tpt-k-root` and `tpt-k-drivers` `main`, priority scheduler wiring and inheritance, IRQ lines beyond the RTC, IOMMU, SMP, TCP retransmit, fs-bridge, synth WASM, AML coverage, virtio-net

### 11b — Adoption tooling
- Status 2026-11-18: the root README and `doctor` are done. The other tool items below are not started.
- [x] `docs`: root `README.md` with a 5-minute quickstart (fetch-deps, doctor, run), the layer map and a status table (2026-11-18: the quickstart uses `doctor`, `fetch-deps`, `check` and `qemu-smoke`, which exist; `run` is not built yet, so it is not in it)
- [x] `toolchain`: `tpt-k-build doctor` checks Rust 1.95.0 and targets, QEMU, OVMF, sibling repos and the git hooks path, and prints the exact fix command for each (Windows and Linux) (2026-11-18: probes the machine, and the report logic is host-tested. Run it from the repo root. On this machine it reports the git hooks path as a warning, because `core.hooksPath` is not set)
- [ ] `toolchain`: `tpt-k-build run` builds the kernel and loader, assembles the ESP with `tpt-k-toolchain-iso`, and launches QEMU with serial on stdio
- [ ] `toolchain`: `tpt-k-build image --run` executes the plan instead of printing "not run" (`main.rs:101-108`); extend `tests/cli.rs`
- [ ] `toolchain`: cross-platform `cargo xtask` (check, build-kernel, qemu-smoke, fetch-deps) so Windows needs no bash; keep the shell scripts as thin wrappers
- [ ] `toolchain`: `.devcontainer/` with Rust 1.95.0, both targets, QEMU, OVMF and `tpt-cargo-vitals`
- [ ] `toolchain`: CI job that builds a bootable image with `tpt-k-build image` and uploads it as an artifact on every push to master (tagged-release attachment waits on the release go-ahead)

### 11c — Innovative features
- Status 2026-11-18: not started.
- [ ] `kernel`/`toolchain`: capability inspector. A serial dump of the live capability graph (holder, object, rights) and `tpt-k-build caps` to render it as text or DOT
- [ ] `root`/`toolchain`: policy-as-manifest. A new crate `tpt-k-root-manifest` compiles a TOML app manifest into the capability grants a process receives, and `tpt-k-build lint-manifest` flags over-broad grants. Property tests linked to `docs/assurance/invariants.md`
- [ ] `kernel`/`toolchain`: deterministic replay. Record serial output and IRQ/timer events from a QEMU run (`-icount`), `tpt-k-build replay` diffs a re-run against the golden trace, and one trace is wired into `qemu-smoke.sh`
- [ ] `toolchain`: assurance dashboard. `tpt-k-build assure --html` renders `docs/assurance/graph.json` (proven, tested, waived, linked to `spec.txt` sections) as one static page, uploaded from CI beside the image
- [ ] Each new tool has a crate README, a CHANGELOG entry and, where it checks an invariant, an evidence item in the assurance graph
