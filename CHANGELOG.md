# Changelog

All notable changes to tpt-k are recorded here. Each crate also keeps its own
`CHANGELOG.md`. This file covers the repo as a whole: the workspace, the
assurance gate and the release process.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The repo has no release yet, so versions follow [release.md](docs/release.md).

## [Unreleased]

### Added

- `SYS_WAIT` (number 18) lets a thread wait for a child it created, and returns the child's exit code. A child the kernel ended after a fault makes the wait fail with `Fault`. Only the creator may wait, with one waiter at a time, and a wait that would block with no other thread ready fails with `Deadlock`. The boot self-test `wait_fault_self_test` checks the fault case, and the demo root waits for its image child.
- `WAIT-1` (`SYS_WAIT`) and `FUZZ-1` (the five mutation suites) in the assurance graph, with their spec lines. The campaign logs are in `docs/assurance/campaigns/`.
- `RIGHTS-1`, proved with z3 in `docs/assurance/proofs/rights.py`: for 64-bit rights words, derivation, grant and token attenuation never give a right the source does not hold. The recorded verdicts are in `rights.py.log`. The proof is over a model of the kernel's rights bits, not over the Rust code.
- Host timing of endpoint and capability operations (`bench_endpoint_send_then_receive`, `bench_capability_mint_derive_revoke`, ignored tests), recorded in `docs/perf/host-ipc.md`. These time the data-structure operations only: about 10.8 ns per endpoint pair and 9.6 ns per capability round. They are not kernel IPC latency.
- `SYS_UNMAP` (number 17) removes a mapping made by `SYS_MAP`. Each memory object records up to 4 mappings, and `SYS_MEM_FREE` refuses while any is recorded (boot self-test `mem: unmapped object was freed`).
- Every `unsafe` block outside `tpt-k-kernel/src/main.rs` (98 sites, still open) now states its invariants. `replay/golden.txt` is regenerated for the new self-test.
- Capability revocation at exit, seL4-style. An exiting thread deletes its handles without revoking the copies other threads hold, and the derivation tree is re-linked (`cap::delete_across`). Memory objects that nothing refers to are then reclaimed, which returns their frames. Revoking the last handle to an object reclaims it too.
- Capability inspector: the kernel prints the live capability graph at boot (`caps: begin` … `caps: end`), and `tpt-k-build caps` renders it as text or DOT.
- Policy as a manifest: `tpt-k-root-manifest` compiles a TOML manifest into grants, refusing any grant above an object's ceiling. `tpt-k-build lint-manifest` lints it.
- Assurance dashboard: `tpt-k-build assure --html <file>` writes one static HTML page.
- Boot image: `tpt-k-build image --run` and `tpt-k-build run` build the kernel and loader, stage the ESP, and boot it in QEMU with serial on stdio.
- `cargo xtask` (`crates/tpt-k-toolchain-xtask`): check, build-kernel, qemu-smoke and fetch-deps in Rust. The shell scripts in `scripts/` are thin wrappers.
- Deterministic replay: `tpt-k-build replay --record|--golden` boots the kernel built with the `replay` feature (a fixed APIC timer count) under QEMU with a fixed instruction-counter shift. Two runs give the same transcript; the golden trace is `replay/golden.txt`.
- Workspace version `0.1.0`, set once in `[workspace.package]` and inherited by all 34 crates (decided 2026-11-18, see `docs/release.md`).
- The license gate (`xtask check`, the CI license job) allows `MIT-0` and `BSL-1.0`, as `LICENSE-POLICY.md` already did. `CDLA-Permissive-2.0` is not in the gate: it stays a scoped exception for `webpki-roots` only.
- `semadiff` is installed from `tpt-semadiff`, so the review workflow in `docs/developer-guide.md` runs on this machine.
- `tpt-cargo-vitals` license gate: dev-only dependencies may use `Apache-2.0` alone (`--dev-allow`), optional dependencies no feature enables are not checked, `WITH` licenses are judged whole, and a recorded exception applies to one crate (`--exception`). Its master has these as `088deb6` and `b718a87`. Seven of eight sibling repos pass; see `DEP-AUDIT.md`.
- CI: `.github/workflows/image.yml` builds the boot image and the dashboard on push to master. `.devcontainer/` gives the pinned toolchain, both targets, QEMU and OVMF.

- `tpt-k-build doctor`: checks the Rust toolchain and targets, QEMU, OVMF, the sibling repos and the git hooks path. It prints the fix for each problem, for Windows and Linux.
- Ring-3 exceptions end only the faulting thread. The kernel prints `user: fault vector N` and goes on. A smoke-test thread checks this.
- `SYS_MEM_FREE` frees an unmapped memory object, revokes every handle to it in every table, and returns its frames. Mapped objects return `EBUSY` until they are unmapped with `SYS_UNMAP`.
- Root `README.md`: a quickstart, the layer map and a status table.
- C-named exports of the string and memory helpers in `tpt-k-userland-libc`, behind its `c-abi` feature.
- A `Spawner` trait in `tpt-k-userland`, so external commands and, later, pipes and redirections can be built and tested against a fake.
- Bounded exhaustive checks of the capability table (`CAP-1` to `CAP-8`) and of
  endpoints (`IPC-1` to `IPC-6`), in `tpt-k-kernel-ipc`. They run every operation
  sequence up to a stated depth, and check the invariants after each step.
- The assurance graph, `docs/assurance/graph.json`, which links each claim to
  its evidence. The invariants are described in `docs/assurance/invariants.md`.
- `tpt-k-build assure`: the assurance gate. It checks that the evidence exists,
  reports each claim as bounded-proven, tested, waived or unproven, and fails on
  an unproven critical claim with no waiver. The gate runs in `scripts/check.sh`
  and in `cargo test`. The generated report is `docs/assurance/report.md`.
- `docs/architecture.md`, `docs/release.md`, and the semadiff review workflow in
  the developer guide.

### Changed

- The QEMU smoke test reads the serial transcript from QEMU's stdout pipe, not from a file. The file dropped kernel output on this host. The smoke test also checks the capability dump.
- The bootstrap thread disables interrupts after the RTC check, so it finishes setting up the demo before any demo thread runs.
- `SYS_WAIT` releases the child's address space once it collects the exit: the child's page tables, and the frames it owns (recorded at spawn). Leaves it shares with its template are kept. `PageTables::release_root` frees the tables of a forked address space and refuses the boot and active spaces. A child that is never waited on keeps its space, which is bounded by the two spawn slots.
- The proof gate needs a recorded verdict. A proof item must have a log beside its script (`<script>.log`) with a line `<property>: unsat` for each property. The gate reads the record and does not run the solver.
- Mutation tests read `TPT_FUZZ_ITERATIONS` when it is set, and keep 2000 as the default. A seeded campaign of one million iterations per parser (ACPI, FDT, PCI, USB and IPC frames) passed. It is not coverage-guided.

- The layer docs in `docs/layers/` no longer call the crates empty placeholders.
  They point to the layer checklists and crate READMEs, which are kept current.
- Capability slots compare for equality, so the invariant checks can see whether
  a refused operation changed the table.
- The `fuzz_tests` modules in `tpt-k-hal-acpi`, `-fdt`, `-pci`, `-usb`,
  `tpt-k-ipc-serde` and `tpt-k-root-spawn` are now `mutation_tests`. They were
  never coverage-guided fuzzing. The assurance graph points at the new names.
- The shell binary writes its `> ` prompt only when standard input is a terminal.
  Piped transcripts contain only command output.
- System calls that fail return a negated errno, `-4095..=-1`, instead of
  `u64::MAX`. The numbers are those of `tpt-k-userland-libc::errno`, and a boot
  self-test checks three of them. The demo's user programs now test for an error
  with a range check, not `== -1`. Receive buffers still use `u64::MAX` for "no
  capability", which is data and not an error.
- The `tcb-nostd` CI group no longer lists `tpt-k-kernel-arch-aarch64`, which is
  an empty stub. It is added back when it has code.
- Module docs in `tpt-k-boot` and its README describe the 80-byte handoff record
  in `rdi`, not a zero argument.

### Fixed

- A spawn refused after its child thread existed halted the kernel; the grant is now checked first.
- Spawned children inherited the boot space's window mappings with no record; they now fork from a template taken before any object is mapped.
- The syscall entry did not clear DF; `FMASK` now clears it with IF.
- The loader's kernel segment end (`paddr + mem_size`) could wrap; it is now `checked_add`.
- The syscall entry did not preserve the caller's callee-saved registers (`rbx`, `rbp`, `r12` to `r15`). A handler that blocked let another thread run, and that thread's values could reach the caller. The entry now saves and restores them.

### Not yet

- Coverage-guided fuzzing (cargo-fuzz or libFuzzer). The seeded mutation campaigns are done.
- SMT over unbounded trees: `RIGHTS-1` covers the rights bit model, but revocation over unbounded capability trees is open. See `docs/assurance/invariants.md`.
- Context switch and interrupt latency timings. The host timings cover only the data structures.
- The external TPT repos do not all pass the license gate yet. See `DEP-AUDIT.md`.
