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
- [ ] `tpt-k-boot-fs`: minimal read-only FAT32 driver (deferred: M1 reads the ESP through the UEFI file protocol; `tpt-k-boot-fs::elf` is the ELF64 parser, host-tested)
- [x] `tpt-k-boot`: UEFI app that locates the kernel on the ESP (2026-11-16: `TPTK\KERNEL.ELF`; root server image not yet)
- [x] Load kernel binary into memory, obtain memory map, exit boot services (2026-11-16: ELF64 PT_LOAD segments at their physical addresses)
- [ ] Define boot info handoff struct (memory map, framebuffer, ACPI RSDP, etc.) (partly, 2026-11-17: a versioned 40-byte record with the memory map, passed in rdi; framebuffer and ACPI RSDP are open; the layout is mirrored in tpt-k-kernel-mem::bootmap)
- [ ] Jump to kernel entry with correct state (partly: rdi holds the handoff record; firmware paging is replaced by the kernel)

### Verification
- [x] Boot the kernel in QEMU via `tpt-k-boot` (milestone M1, boot side; 2026-11-16, `tpt-kernel/scripts/qemu-smoke.sh`)

## Phase 6 — AArch64

- [ ] UEFI AArch64 boot via `tpt-k-boot`

## Phase 9 — Bare metal

- [ ] Boot on real UEFI x86_64 hardware

## Open decisions

- [ ] Choose the handoff struct layout's versioning rule, so the kernel can reject an incompatible loader
