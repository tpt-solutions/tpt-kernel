# tpt-k-toolchain — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the build CLI `tpt-k-build`, and the assembly of bootable images.

---

## Phase 7 — Toolchain

- [ ] `tpt-k-toolchain` (`tpt-k-build`): resolve deps, cross-compile all workspaces, link binaries
- [ ] `tpt-k-toolchain-iso`: assemble bootloader, kernel, root server, and root FS into a UEFI ISO or raw disk image
- [ ] CI produces a bootable QEMU-ready image artifact
- [ ] Milestone M6 (toolchain side)

## CI

- [ ] Reusable workflow shared across the repos (see master plan)
