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
