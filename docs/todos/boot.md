# tpt-k-boot — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: the UEFI loader. It finds the kernel and root images on the ESP, exits boot
services, and jumps to the kernel with a boot info handoff.

Rules: `#![no_std]`, 100% Rust, no C/C++ bindings, no GPL/LGPL/AGPL/MPL or Apache-only
dependencies (dev-dependencies excepted).

---

## Phase 1 — Boot & Hello World

- [x] `tpt-k-boot-efi`: UEFI ABI and wrappers for the boot services the loader uses (2026-11-16); runtime services not yet
- [x] `tpt-k-boot-fs`: minimal read-only FAT32 driver (2026-11-17: boot sector checks, cluster chains with loop detection, 8.3 and ASCII long-name lookup, whole-file reads through a `BlockRead` trait; host-tested. Long names are ASCII only, and the loader does not use it yet: M1 still reads the ESP through the UEFI file protocol)
- [x] `tpt-k-boot`: UEFI app that locates the kernel on the ESP (2026-11-16: `TPTK\KERNEL.ELF`; root server image not yet)
- [x] Load kernel binary into memory, obtain memory map, exit boot services (2026-11-16: ELF64 PT_LOAD segments at their physical addresses)
- [x] Define boot info handoff struct (memory map, framebuffer, ACPI RSDP, etc.) (2026-11-17: a versioned record, passed in rdi, with the memory map; 2026-11-17 version 2 adds the ACPI 2.0 RSDP address and the GOP framebuffer (base, size, width, height, stride, pixel format); the layout is mirrored in tpt-k-kernel-mem::bootmap. Absent firmware facts are recorded as zero. Verified by the QEMU smoke test under OVMF, which reports rsdp 0xf77e014 and a 1280x800 BGRX framebuffer)
- [ ] Jump to kernel entry with correct state (partly: rdi holds the handoff record; firmware paging is replaced by the kernel)

### Verification
- [x] Boot the kernel in QEMU via `tpt-k-boot` (milestone M1, boot side; 2026-11-16, `tpt-kernel/scripts/qemu-smoke.sh`)

## Phase 6 — AArch64

- [ ] UEFI AArch64 boot via `tpt-k-boot`

## Phase 9 — Bare metal

- [ ] Boot on real UEFI x86_64 hardware

## Open decisions

- [x] Choose the handoff struct layout's versioning rule, so the kernel can reject an incompatible loader (2026-11-17: the kernel accepts exactly one version, `bootmap::VERSION`; any layout change bumps it, and the kernel refuses the rest. Documented in `bootmap.rs`)

## Phase 11a — stub cleanup (2026-11-18)

- [x] Loader and kernel docs describe the 80-byte handoff record in `rdi`, not a zero argument (`tpt-k-boot` and its README)
- [x] `tpt-k-boot-fs` README notes that the FAT32 driver is host-tested and not yet used by the loader (already true; checked)
