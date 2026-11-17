# tpt-k-boot

The tpt-k UEFI loader. It reads the kernel image from the EFI System Partition, places its `PT_LOAD` segments at their physical addresses, exits boot services and jumps to the kernel entry point.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 1, implemented. The loader reads `TPTK\KERNEL.ELF` from the boot volume. The kernel is entered with an 80-byte handoff record in `rdi`. It carries the UEFI memory map (address, byte length and descriptor size), the ACPI RSDP address and the GOP framebuffer. The layout is `BootInfo` in `src/main.rs`, and it is mirrored in `tpt-k-kernel-mem::bootmap`, so the two must agree.

## Build

This crate is a UEFI application, so build it for its target:

```sh
cargo build -p tpt-k-boot --target x86_64-unknown-uefi
```

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

