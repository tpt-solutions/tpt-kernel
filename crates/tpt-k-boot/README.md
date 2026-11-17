# tpt-k-boot

The tpt-k UEFI loader. It reads the kernel image from the EFI System Partition, places its `PT_LOAD` segments at their physical addresses, exits boot services and jumps to the kernel entry point.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 1, implemented. The loader reads `TPTK\KERNEL.ELF` from the boot volume. The handoff argument is 0 for now, and the memory map is not passed to the kernel yet.

## Build

This crate is a UEFI application, so build it for its target:

```sh
cargo build -p tpt-k-boot --target x86_64-unknown-uefi
```

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

