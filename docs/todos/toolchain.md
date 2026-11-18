# tpt-k-toolchain — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the build CLI `tpt-k-build`, and the assembly of bootable images.

---

## Phase 7 — Toolchain

- [ ] `tpt-k-toolchain` (`tpt-k-build`): resolve deps, cross-compile all workspaces, link binaries (partly, 2026-11-17: a planner that groups crates by layer and target and prints the cargo commands for `plan`, `check` and `image`; it does not run cargo)
- [ ] `tpt-k-toolchain-iso`: assemble bootloader, kernel, root server, and root FS into a UEFI ISO or raw disk image (partly, 2026-11-17: a FAT32 boot-volume writer, round-tripped through `tpt-k-boot-fs`, wrapped in a GPT disk image with a protective MBR and an ESP entry; no ISO 9660 wrapper)
- [ ] CI produces a bootable QEMU-ready image artifact
- [ ] Milestone M6 (toolchain side)

## CI

- [ ] Reusable workflow shared across the repos (see master plan)

## Phase 11b — adoption tooling (2026-11-18)

- [x] `tpt-k-build doctor`: checks Rust 1.95.0 and targets, QEMU, OVMF, sibling repos and the git hooks path, and prints the fix for each problem (Windows and Linux)
- [x] `tpt-k-build run` and `image --run`: build, stage the ESP, boot QEMU with serial on stdio (tested here: PASS)
- [x] `cargo xtask` (`crates/tpt-k-toolchain-xtask`): check, build-kernel, qemu-smoke, fetch-deps in Rust; scripts/ are thin wrappers
- [x] `.devcontainer/`: written, not built here (no container runtime)
- [x] CI `image.yml`: builds the boot image and the assurance dashboard on push to master, uploads both. Not run on GitHub yet

## Phase 11c — innovative features (2026-11-18)

- [x] Capability inspector: the kernel dumps `cap:` lines at boot; `tpt-k-build caps --serial` renders text or DOT
- [x] Deterministic replay: `tpt-k-build replay` boots the kernel built with the `replay` feature (fixed APIC timer count) under QEMU with a fixed icount shift. Two runs match the golden trace in `replay/golden.txt` (2026-11-18). The shift was the second source of variation: `shift=auto` tracks host load.
- [x] Assurance dashboard: `tpt-k-build assure --html <file>`. Open: linking claims to spec.txt sections
